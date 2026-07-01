//! Per-eye render-target hashing diagnostic for the "stronger in one eye" stereo artifacts.
//!
//! Some shadow / darkening effects appear in both eyes but are visibly stronger (darker / longer) in
//! one. The leading hypothesis is a buffer that accumulates across the two per-eye Draws -- written
//! once per dispatch but cleared/reset only once per frame, so the second eye reads ~2x (the same
//! shape as the already-fixed SSAO temporal-history bug).
//!
//! This module pins that down empirically. Run with `stereo.cameras` off so both eyes share one
//! camera: every intermediate render target should then be byte-identical between eye 0 and eye 1.
//! After each eye's Draw completes, [`hash_engine_rts`] copies a curated set of engine RTs to a
//! staging texture, hashes the bytes, and records the per-eye hash into the active render trace as a
//! [`TraceEvent::RtHash`]. Any RT whose eye-0 and eye-1 hashes differ (with identical cameras) is
//! carrying per-eye state across the two Draws.
//!
//! A finding from the first traces shapes how to read the output: `GBuffer0` is *aliased and reused*
//! as the post-effects fullscreen scratch/composite pool (`CGraphicsEngine::CreateRenderSetups`
//! creates `GBuffer0/2/3` linear/sRGB aliases that `CPostEffectsManager` adopts; `GBuffer1`, the
//! normals, is never aliased). So a `GBuffer0` mismatch is a *downstream symptom* -- by end of Draw it
//! holds the final post image, which derives from `MainColor`. Treat **`MainColor`** as the real
//! signal; the leading sources of its per-eye divergence are the SSR previous-scene capture (content
//! regenerated every Draw -- `stereo.skip_ssr` tests it) and the unrestored `RenderEngine` per-Draw
//! constant-buffer ring (`stereo.restore_cb_ring` tests it).
//!
//! It is gated by `stereo.diagnose_rt_hashes` and only runs while a trace is collecting, so it costs
//! nothing in normal play.

use anyhow::Context as _;
use jc3gi::graphics_engine::{graphics_engine::GraphicsEngine, texture::Texture};
use windows::{
    Win32::{
        Graphics::Direct3D11::{
            D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC,
            D3D11_USAGE_STAGING, ID3D11Texture2D,
        },
        System::Threading::{EnterCriticalSection, LeaveCriticalSection},
    },
    core::Interface,
};

use crate::{
    config::Config,
    debug::trace::{RtKind, TraceEvent, TraceState, tracing_active},
};

/// Hash the curated engine render targets for the eye that just finished drawing, recording each into
/// the active trace. The eye index is attached automatically by [`TraceState::record_eye`] from the
/// live stereo state, so the caller only needs to invoke this once per eye after its Draw drains.
///
/// No-op unless `stereo.diagnose_rt_hashes` is set and a trace is collecting.
pub fn hash_engine_rts() {
    if !tracing_active() || !Config::lock_query(|c| c.stereo.diagnose_rt_hashes) {
        return;
    }
    if let Err(e) = unsafe { hash_inner() } {
        tracing::warn!("rt_hash: {e:#}");
    }
}

/// # Safety
/// Dereferences engine singletons; must be called on the game thread after `WaitForCPUDrawToFinish`,
/// when the render worker is idle and the engine device, context, and RTs are live.
unsafe fn hash_inner() -> anyhow::Result<()> {
    let ge = unsafe { GraphicsEngine::get() }.context("graphics engine unavailable")?;
    let device = unsafe { ge.m_Device.as_ref() }.context("graphics device unavailable")?;
    let d3d = &device.m_Device;
    let context = unsafe { device.m_Context.as_ref() }.context("graphics context unavailable")?;
    let ctx = &context.m_Context;

    // Depth targets are deliberately excluded: a CPU readback (staging copy + map) of a depth/stencil
    // or typeless-format texture is an awkward and fault-prone D3D path under the translation layers,
    // and depth is geometry-deterministic anyway (so it tracks GBuffer1/2/3). Velocity is a plain
    // colour format and is the informative new target -- it carries the previous-frame view-projection,
    // the suspected per-eye divergence source.
    let targets: [(RtKind, *mut Texture); 7] = [
        (RtKind::MainColor, ge.m_MainColorBuffer),
        (RtKind::GBuffer0, ge.m_GBufferTexture[0]),
        (RtKind::GBuffer1, ge.m_GBufferTexture[1]),
        (RtKind::GBuffer2, ge.m_GBufferTexture[2]),
        (RtKind::GBuffer3, ge.m_GBufferTexture[3]),
        (RtKind::Velocity, ge.m_VelocityBufferTexture),
        (RtKind::BackBufferLinear, ge.m_BackBufferLinear),
    ];

    for (kind, tex_ptr) in targets {
        let Some(tex) = (unsafe { tex_ptr.as_ref() }) else {
            continue;
        };
        // The engine `Texture::m_Texture` is already an `ID3D11Resource`. Cast it to a 2D texture to
        // read its descriptor and build a matching CPU-readable staging copy.
        let src2d: ID3D11Texture2D = tex.m_Texture.cast().context("RT is not a 2D texture")?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { src2d.GetDesc(&mut desc) };
        // MSAA targets cannot be mapped directly; skip rather than resolve (none of the curated set is
        // multisampled in practice).
        if desc.SampleDesc.Count > 1 {
            continue;
        }

        let staging_desc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            ..desc
        };
        let mut staging: Option<ID3D11Texture2D> = None;
        unsafe { d3d.CreateTexture2D(&staging_desc, None, Some(&mut staging)) }
            .context("creating an RT staging texture")?;
        let staging = staging.context("RT staging texture not created")?;

        // CopyResource + Map drive the immediate context: serialise with the render thread via the
        // engine context mutex, exactly as the capture path does.
        let hash = unsafe {
            EnterCriticalSection(context.m_Mutex);
            ctx.CopyResource(&staging, &tex.m_Texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            let hash = if ctx
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .is_ok()
            {
                let len = mapped.RowPitch as usize * desc.Height as usize;
                let bytes = std::slice::from_raw_parts(mapped.pData.cast::<u8>(), len);
                let hash = hash_bytes(bytes);
                ctx.Unmap(&staging, 0);
                Some(hash)
            } else {
                None
            };
            LeaveCriticalSection(context.m_Mutex);
            hash
        };

        if let Some(hash) = hash {
            TraceState::record_eye(TraceEvent::RtHash { rt: kind, hash });
        }
    }

    Ok(())
}

/// FNV-1a over 64-bit words (with a byte tail), fast enough to hash several multi-megabyte targets per
/// frame during a short trace. Both eyes hash the same staging layout, so row-pitch padding cancels in
/// the comparison.
fn hash_bytes(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut chunks = data.chunks_exact(8);
    for chunk in &mut chunks {
        let word = u64::from_le_bytes(chunk.try_into().expect("chunks_exact(8) yields 8 bytes"));
        h = (h ^ word).wrapping_mul(0x0000_0100_0000_01b3);
    }
    for &b in chunks.remainder() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}
