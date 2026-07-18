//! Single-pass stereo (experimental): render the G-buffer geometry once, emitting both eyes via
//! instancing + `SV_ViewportArrayIndex` routing into a double-wide render target, instead of the
//! double-draw (two full `game.Draw` walks, one per eye). See `docs/mod/single-pass-stereo.md` for
//! the design and phased plan.
//!
//! This module owns the mod-side state that the double-draw path does not need:
//! - the DXVK viewport-routing **capability probe** ([`probe`] / [`capability`]);
//! - the vertex-shader rewrite **census** ([`record_patch_outcome`] and the `*_count` getters), which
//!   the `CreateVertexProgram` hook feeds so the debug UI can report how the rewriter fared against
//!   the game's real shader set.
//!
//! The rest of the pipeline (cb13 dual-eye upload, the double-wide render-setup re-init, the
//! draw-doubling) is built out under [`crate::config::StereoConfig::single_pass`]; until it lands,
//! [`crate::config::StereoConfig::single_pass_patch_dryrun`] runs the census with no rendering change.

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use dxbc_stereo::DxbcError;
use jc3gi::graphics_engine::graphics_engine::GraphicsEngine;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_FEATURE_D3D11_OPTIONS3, D3D11_FEATURE_DATA_D3D11_OPTIONS3, ID3D11Device,
};

/// The result of the DXVK viewport-routing capability probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Not yet probed (no device seen, or the probe has not run this session).
    Unprobed,
    /// `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer` is supported: a vertex shader may write
    /// `SV_ViewportArrayIndex` directly, so single-pass routing is possible.
    Supported,
    /// The capability is absent; single-pass must fall back to double-draw.
    Unsupported,
}

/// Probe the D3D11 device for `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer` (the D3D11.3 feature
/// that lets a vertex shader write `SV_ViewportArrayIndex`), caching the result. Idempotent and
/// cheap; safe to call every frame. `CheckFeatureSupport` on the device is free-threaded, so no
/// context lock is needed.
pub fn probe(device: &ID3D11Device) -> Capability {
    let mut options = D3D11_FEATURE_DATA_D3D11_OPTIONS3::default();
    let ok = unsafe {
        device.CheckFeatureSupport(
            D3D11_FEATURE_D3D11_OPTIONS3,
            std::ptr::from_mut(&mut options).cast(),
            std::mem::size_of::<D3D11_FEATURE_DATA_D3D11_OPTIONS3>() as u32,
        )
    };
    let capability = if ok.is_ok()
        && options
            .VPAndRTArrayIndexFromAnyShaderFeedingRasterizer
            .as_bool()
    {
        Capability::Supported
    } else {
        Capability::Unsupported
    };
    CAPABILITY.store(capability as u8, Ordering::Relaxed);
    capability
}

/// Probe the capability using the live engine device, if one is available and the probe has not run
/// yet. Returns the (now cached) result. Called from the debug UI and the frame driver so the probe
/// happens as soon as a device exists.
pub fn probe_if_needed() -> Capability {
    let cached = capability();
    if cached != Capability::Unprobed {
        return cached;
    }
    // SAFETY: `GraphicsEngine::get` returns the live singleton or `None`; the device pointer is stable
    // once the engine has initialised.
    let Some(device) = (unsafe { GraphicsEngine::get() }) else {
        return Capability::Unprobed;
    };
    let Some(device) = (unsafe { device.m_Device.as_ref() }) else {
        return Capability::Unprobed;
    };
    probe(&device.m_Device)
}

/// The cached capability-probe result.
pub fn capability() -> Capability {
    match CAPABILITY.load(Ordering::Relaxed) {
        x if x == Capability::Supported as u8 => Capability::Supported,
        x if x == Capability::Unsupported as u8 => Capability::Unsupported,
        _ => Capability::Unprobed,
    }
}

/// Record the outcome of running [`dxbc_stereo::patch_vertex_shader`] on one vertex shader, for the
/// census the debug UI reports. Classifies into: successfully patched, no per-eye references (the
/// baked-WVP / no-position families that are left double-drawn -- expected, not a failure), and
/// genuinely errored (an unexpected shape the rewriter could not handle -- worth investigating).
pub fn record_patch_outcome(outcome: &Result<Vec<u8>, DxbcError>) {
    match outcome {
        Ok(_) => &PATCHED,
        Err(DxbcError::NoPerEyeReferences) => &NO_REFS,
        Err(_) => &ERRORED,
    }
    .fetch_add(1, Ordering::Relaxed);
}

/// Vertex shaders successfully rewritten for single-pass since injection.
pub fn patched_count() -> usize {
    PATCHED.load(Ordering::Relaxed)
}

/// Vertex shaders with no per-eye `cb0` references -- the baked-WVP / no-position families left
/// double-drawn. Expected, not a failure.
pub fn no_refs_count() -> usize {
    NO_REFS.load(Ordering::Relaxed)
}

/// Vertex shaders the rewriter could not handle for an unexpected reason (a shape it does not yet
/// support). A non-zero count flags shaders to investigate.
pub fn errored_count() -> usize {
    ERRORED.load(Ordering::Relaxed)
}

/// Reset the census counters (on a shader reload, so the reported numbers reflect one clean pass over
/// the shader set rather than accumulating across reloads).
pub fn reset_census() {
    PATCHED.store(0, Ordering::Relaxed);
    NO_REFS.store(0, Ordering::Relaxed);
    ERRORED.store(0, Ordering::Relaxed);
}

static CAPABILITY: AtomicU8 = AtomicU8::new(Capability::Unprobed as u8);
static PATCHED: AtomicUsize = AtomicUsize::new(0);
static NO_REFS: AtomicUsize = AtomicUsize::new(0);
static ERRORED: AtomicUsize = AtomicUsize::new(0);
