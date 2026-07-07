//! Safe-ish Rust bindings to FidelityFX Super Resolution 2 (FSR2) with the native DirectX 11 backend.
//!
//! Wraps the vendored `optiscaler/FidelityFX-FSR2-DX11` submodule (MIT) through a thin C shim
//! (`shim/fsr_shim.{h,cpp}`), so the by-value Ffx structs stay on the C++ side. The payload drives one
//! [`Context`] per eye: create it at the per-eye resolution, [`Context::dispatch`] each frame with the
//! engine's color/depth/velocity and our output target, and drop it to tear down. See `docs/mod/fsr.md`.
//!
//! The backend records onto the device's immediate context, so dispatch must happen on the render
//! thread under the engine's context mutex, like the other D3D11 work in the payload.

use windows::{
    Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext, ID3D11Resource},
    core::Interface,
};

mod ffi {
    use std::ffi::c_void;

    /// Opaque per-eye FSR context (heap-allocated by the shim).
    #[repr(C)]
    pub struct FsrContext {
        _opaque: [u8; 0],
    }

    #[repr(C)]
    pub struct FsrDispatchParams {
        pub context: *mut c_void,
        pub color: *mut c_void,
        pub depth: *mut c_void,
        pub motion_vectors: *mut c_void,
        pub exposure: *mut c_void,
        pub output: *mut c_void,
        pub render_width: u32,
        pub render_height: u32,
        pub jitter_x: f32,
        pub jitter_y: f32,
        pub motion_vector_scale_x: f32,
        pub motion_vector_scale_y: f32,
        pub enable_sharpening: bool,
        pub sharpness: f32,
        pub frame_time_delta_ms: f32,
        pub pre_exposure: f32,
        pub reset: bool,
        pub camera_near: f32,
        pub camera_far: f32,
        pub camera_fov_angle_vertical: f32,
    }

    unsafe extern "C" {
        pub fn fsr_context_create(
            device: *mut c_void,
            flags: u32,
            max_render_width: u32,
            max_render_height: u32,
            display_width: u32,
            display_height: u32,
        ) -> *mut FsrContext;

        pub fn fsr_context_dispatch(ctx: *mut FsrContext, params: *const FsrDispatchParams)
        -> bool;

        pub fn fsr_context_destroy(ctx: *mut FsrContext);

        pub fn fsr_jitter_phase_count(render_width: i32, display_width: i32) -> i32;

        pub fn fsr_jitter_offset(out_x: *mut f32, out_y: *mut f32, index: i32, phase_count: i32);
    }
}

/// Context initialization flags (subset of `FfxFsr2InitializationFlagBits` the integration uses).
/// OR these together into the `flags` mask passed to [`Context::new`].
pub mod init_flags {
    /// The input color is high-dynamic-range (pre-tonemap). Omit for tonemapped LDR input.
    pub const HIGH_DYNAMIC_RANGE: u32 = 1 << 0;
    /// The depth buffer is inverted (reverse-Z, near=1/far=0) -- JC3's convention.
    pub const DEPTH_INVERTED: u32 = 1 << 3;
    /// The depth buffer uses an infinite far plane.
    pub const DEPTH_INFINITE: u32 = 1 << 4;
    /// FSR computes exposure itself instead of being handed an exposure texture.
    pub const AUTO_EXPOSURE: u32 = 1 << 5;
}

/// A 2D resolution in pixels.
#[derive(Copy, Clone, Debug)]
pub struct Extent {
    pub width: u32,
    pub height: u32,
}

/// Per-frame dispatch inputs. Textures are borrowed engine/our resources; `exposure` is optional.
pub struct DispatchInfo<'a> {
    /// The immediate context FSR records its passes onto (the engine's `ID3D11DeviceContext`).
    pub context: &'a ID3D11DeviceContext,
    pub color: &'a ID3D11Resource,
    pub depth: &'a ID3D11Resource,
    pub motion_vectors: &'a ID3D11Resource,
    pub exposure: Option<&'a ID3D11Resource>,
    pub output: &'a ID3D11Resource,
    /// The resolution the inputs were rendered at (== `display` for native AA).
    pub render_size: Extent,
    /// Subpixel camera jitter applied this frame (from [`jitter_offset`]).
    pub jitter: (f32, f32),
    /// Scale mapping the velocity buffer into FSR's motion-vector convention.
    pub motion_vector_scale: (f32, f32),
    pub sharpening: Option<f32>,
    /// Milliseconds since the previous frame.
    pub frame_time_delta_ms: f32,
    /// Pre-exposure value (must be > 0).
    pub pre_exposure: f32,
    /// Set on a camera cut / discontinuity so FSR discards history.
    pub reset: bool,
    pub camera_near: f32,
    pub camera_far: f32,
    /// Vertical field of view, in radians.
    pub camera_fov_vertical: f32,
}

/// One FSR2 upscaling/AA context. The payload keeps one per eye.
pub struct Context {
    raw: *mut ffi::FsrContext,
}

// The context wraps a heap allocation the payload owns and only touches on the render thread; it is
// not internally synchronized, so it is `Send` (moved into thread-owned state) but not `Sync`.
unsafe impl Send for Context {}

impl Context {
    /// Create a context for one eye at the given resolutions (`render == display` for native AA).
    /// `flags` is a mask of [`init_flags`]. Returns `None` if the backend failed to initialize.
    pub fn new(device: &ID3D11Device, flags: u32, render: Extent, display: Extent) -> Option<Self> {
        // SAFETY: `device` is a live ID3D11Device; the shim only borrows it for the call.
        let raw = unsafe {
            ffi::fsr_context_create(
                device.as_raw(),
                flags,
                render.width,
                render.height,
                display.width,
                display.height,
            )
        };
        (!raw.is_null()).then_some(Context { raw })
    }

    /// Record one FSR2 dispatch onto the device's immediate context. Returns `false` on backend
    /// failure. Must run on the render thread under the engine's context mutex.
    pub fn dispatch(&mut self, info: &DispatchInfo<'_>) -> bool {
        let params = ffi::FsrDispatchParams {
            context: info.context.as_raw(),
            color: info.color.as_raw(),
            depth: info.depth.as_raw(),
            motion_vectors: info.motion_vectors.as_raw(),
            exposure: info.exposure.map_or(std::ptr::null_mut(), |e| e.as_raw()),
            output: info.output.as_raw(),
            render_width: info.render_size.width,
            render_height: info.render_size.height,
            jitter_x: info.jitter.0,
            jitter_y: info.jitter.1,
            motion_vector_scale_x: info.motion_vector_scale.0,
            motion_vector_scale_y: info.motion_vector_scale.1,
            enable_sharpening: info.sharpening.is_some(),
            sharpness: info.sharpening.unwrap_or(0.0),
            frame_time_delta_ms: info.frame_time_delta_ms,
            pre_exposure: info.pre_exposure,
            reset: info.reset,
            camera_near: info.camera_near,
            camera_far: info.camera_far,
            camera_fov_angle_vertical: info.camera_fov_vertical,
        };
        // SAFETY: `self.raw` is a live context; `params` borrows resources live for this call.
        unsafe { ffi::fsr_context_dispatch(self.raw, &params) }
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: `self.raw` was created by `fsr_context_create` and is destroyed exactly once. The
        // caller must have ensured the GPU is idle (see `docs/mod/fsr.md`).
        unsafe { ffi::fsr_context_destroy(self.raw) };
    }
}

/// The jitter-sequence length for the given render/display widths (FSR's Halton phase count).
pub fn jitter_phase_count(render_width: i32, display_width: i32) -> i32 {
    // SAFETY: pure arithmetic in the backend.
    unsafe { ffi::fsr_jitter_phase_count(render_width, display_width) }
}

/// The subpixel jitter offset for `index` within `phase_count` (drive the camera with this and feed
/// the same value to [`DispatchInfo::jitter`]).
pub fn jitter_offset(index: i32, phase_count: i32) -> (f32, f32) {
    let mut x = 0.0;
    let mut y = 0.0;
    // SAFETY: writes two floats through valid pointers.
    unsafe { ffi::fsr_jitter_offset(&mut x, &mut y, index, phase_count) };
    (x, y)
}
