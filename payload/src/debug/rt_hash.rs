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
        Graphics::{
            Direct3D11::{
                D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
                D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, ID3D11Texture2D,
            },
            Dxgi::Common::{
                DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
                DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
            },
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
    if !tracing_active() {
        return;
    }
    let (want_hashes, want_shots) = Config::lock_query(|c| {
        (
            c.stereo.diagnose_rt_hashes,
            c.stereo.diagnose_rt_screenshots,
        )
    });
    if !want_hashes && !want_shots {
        return;
    }
    if let Err(e) = unsafe { hash_inner(want_hashes, want_shots) } {
        tracing::warn!("rt_hash: {e:#}");
    }
}

/// # Safety
/// Dereferences engine singletons; must be called on the game thread after `WaitForCPUDrawToFinish`,
/// when the render worker is idle and the engine device, context, and RTs are live.
unsafe fn hash_inner(want_hashes: bool, want_shots: bool) -> anyhow::Result<()> {
    let ge = unsafe { GraphicsEngine::get() }.context("graphics engine unavailable")?;
    let device = unsafe { ge.m_Device.as_ref() }.context("graphics device unavailable")?;
    let d3d = &device.m_Device;
    let context = unsafe { device.m_Context.as_ref() }.context("graphics context unavailable")?;
    let ctx = &context.m_Context;
    let eye = crate::stereo::draw_index();

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
        // Screenshot each eye's final BackBufferLinear; hashing (when on) covers the whole set. Skip a
        // target's readback entirely when neither consumer needs it.
        let shot_this = want_shots && matches!(kind, RtKind::BackBufferLinear);
        if !want_hashes && !shot_this {
            continue;
        }
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
        // engine context mutex, exactly as the capture path does. The screenshot pixels are copied out
        // of the mapping under the lock but written to disk after it releases, so the render thread is
        // never blocked on file I/O.
        let (hash, shot) = unsafe {
            EnterCriticalSection(context.m_Mutex);
            ctx.CopyResource(&staging, &tex.m_Texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            let out = if ctx
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .is_ok()
            {
                let len = mapped.RowPitch as usize * desc.Height as usize;
                let bytes = std::slice::from_raw_parts(mapped.pData.cast::<u8>(), len);
                let hash = want_hashes.then(|| hash_bytes(bytes));
                let shot = shot_this
                    .then(|| to_rgba8(bytes, mapped.RowPitch, desc.Width, desc.Height, desc.Format))
                    .flatten();
                ctx.Unmap(&staging, 0);
                (hash, shot)
            } else {
                (None, None)
            };
            LeaveCriticalSection(context.m_Mutex);
            out
        };

        if let Some(hash) = hash {
            TraceState::record_eye(TraceEvent::RtHash { rt: kind, hash });
        }
        if let Some(rgba) = shot {
            write_png_frame(eye, desc.Width, desc.Height, rgba);
        }
    }

    Ok(())
}

/// Convert the mapped staging bytes of a supported 8-bit-per-channel target into a tightly-packed
/// (row-pitch-stripped) RGBA8 buffer, swizzling BGRA to RGBA. `None` for any other format (skipped with
/// a log rather than writing a mislabeled image); the diagnostic only shoots `BackBufferLinear`, a
/// presentable 8-bit target in practice.
fn to_rgba8(
    bytes: &[u8],
    row_pitch: u32,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
) -> Option<Vec<u8>> {
    let swap_rb = match format {
        DXGI_FORMAT_R8G8B8A8_UNORM | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => false,
        DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB => true,
        _ => {
            tracing::warn!(
                "rt_hash: screenshot skipped, unsupported format {}",
                format.0
            );
            return None;
        }
    };
    let tight = (width * 4) as usize;
    let pitch = row_pitch as usize;
    let mut out = Vec::with_capacity(tight * height as usize);
    for y in 0..height as usize {
        let row = bytes.get(y * pitch..y * pitch + tight)?;
        if swap_rb {
            for px in row.chunks_exact(4) {
                out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        } else {
            out.extend_from_slice(row);
        }
    }
    Some(out)
}

/// Encode one frame's RGBA8 pixels to a PNG in this trace's `traces/<stamp>/` folder, named by frame
/// index and eye so the sequence reassembles and aligns 1:1 with the per-frame trace events.
fn write_png_frame(eye: usize, width: u32, height: u32, rgba: Vec<u8>) {
    let Some((dir, frame)) = TraceState::screenshot_target() else {
        return;
    };
    let Some(image) = image::RgbaImage::from_raw(width, height, rgba) else {
        tracing::warn!("rt_hash: screenshot buffer size mismatch");
        return;
    };
    let path = dir.join(format!("frame{frame:03}_eye{eye}.png"));
    if let Err(e) = image.save_with_format(&path, image::ImageFormat::Png) {
        tracing::warn!("rt_hash: screenshot PNG write failed: {e}");
    }
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
