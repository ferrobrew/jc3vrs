//! FSR anti-aliasing / upscaling integration.
//!
//! Runs FSR2 (via [`fsr_sys`]) in place of the engine's SMAA: per eye, it resolves the post-tonemap
//! scene color (plus the engine's depth and velocity) into our own output texture, which is then
//! copied back into the post-effect chain's working slot. The engine AA is suppressed while FSR is
//! active. See `docs/fsr.md` for the dispatch-point rationale and the AA-first/upscaler-later plan.
//!
//! State is one [`fsr_sys::Context`] and one output texture per eye, lazily (re)created whenever the
//! render resolution changes -- the same compare-and-recreate pattern the debug captures use, which
//! self-heals after the engine resizes (`Graphics::Reset` -> `CreateRenderSetups`) rather than racing
//! it. All of this lives on the render thread, behind [`FSR_STATE`]; dispatch runs under the engine's
//! context mutex like the other D3D11 work.

use fsr_sys::{Context, DispatchInfo, Extent, init_flags};
use jc3gi::{
    graphics_engine::{device::Device, texture::Texture},
    types::math::Matrix4,
};
use parking_lot::Mutex;
use windows::{
    Win32::Graphics::{
        Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_UNORDERED_ACCESS, D3D11_TEXTURE2D_DESC,
            D3D11_USAGE_DEFAULT, ID3D11Resource, ID3D11Texture2D,
        },
        Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC},
    },
    core::Interface,
};

/// Per-eye FSR state, recreated together when the render size changes.
struct EyeState {
    context: Context,
    /// The UAV output target FSR writes into (display-res; copied back into the post chain).
    output: ID3D11Texture2D,
    /// Set on creation; the first dispatch consumes it as FSR's `reset` so a fresh context (after a
    /// resize / toggle-on) discards any garbage history instead of reprojecting against it.
    fresh: bool,
}

/// The live FSR integration state (render thread only).
pub struct FsrState {
    eyes: [Option<EyeState>; 2],
    /// (width, height) the current eye contexts/outputs were built for; recreate on change.
    size: Option<(u32, u32)>,
}
impl FsrState {
    const fn new() -> Self {
        Self {
            eyes: [None, None],
            size: None,
        }
    }
}

/// Global FSR state. Locked briefly on the render thread at the AA hook.
pub static FSR_STATE: Mutex<FsrState> = Mutex::new(FsrState::new());

/// Drive an FSR resolve for `eye` over the current frame's buffers, writing the anti-aliased result
/// back into `slot_color` (the post-effect chain's working LDR texture). Returns `true` if FSR ran;
/// `false` means it fell through and the engine AA should proceed as normal.
///
/// `device` is the engine D3D11 device; `slot_color` is the chain's current result texture (input and
/// copy-back target); `depth` / `velocity` are the engine's MainDepth / Velocity buffers; `sharpness`
/// is the optional RCAS strength. Runs on the render thread; takes the engine context mutex itself for
/// the copy-back.
pub fn dispatch_eye(
    state: &mut FsrState,
    device: &Device,
    eye: usize,
    slot_color: &Texture,
    depth: &Texture,
    velocity: &Texture,
    sharpness: Option<f32>,
) -> bool {
    let width = u32::from(slot_color.m_Width);
    let height = u32::from(slot_color.m_Height);
    if width == 0 || height == 0 {
        return false;
    }

    // (Re)build both eyes' contexts + outputs if the render size changed.
    if state.size != Some((width, height)) {
        state.eyes = [None, None];
        state.size = Some((width, height));
    }
    if state.eyes[eye].is_none() {
        state.eyes[eye] = create_eye(device, slot_color, width, height);
    }
    let Some(eye_state) = state.eyes[eye].as_mut() else {
        return false;
    };

    // SAFETY: the engine's context wrapper is live for the duration of this render-thread call.
    let Some(context) = (unsafe { device.m_Context.as_ref() }) else {
        return false;
    };

    let color: &ID3D11Resource = &slot_color.m_Texture;
    let depth_res: &ID3D11Resource = &depth.m_Texture;
    let velocity_res: &ID3D11Resource = &velocity.m_Texture;
    let output_res: ID3D11Resource = eye_state.output.cast().expect("texture is a resource");

    let camera = camera_params();
    let info = DispatchInfo {
        context: &context.m_Context,
        color,
        depth: depth_res,
        motion_vectors: velocity_res,
        exposure: None,
        output: &output_res,
        render_size: Extent { width, height },
        // The same per-frame jitter applied to the camera projection (see apply_jitter_to_projection),
        // in FSR's pixel space.
        jitter: current_jitter(width, height).unwrap_or((0.0, 0.0)),
        // The velocity-buffer encoding is not yet pinned (the convention lives in the velocity-write
        // shader, which isn't in the binary), so the scale is a live debug knob -- calibrate in-game.
        motion_vector_scale: motion_vector_scale(width, height),
        sharpening: sharpness,
        frame_time_delta_ms: camera.frame_time_delta_ms,
        pre_exposure: 1.0,
        // Discard history on the first dispatch of a freshly created context.
        reset: std::mem::take(&mut eye_state.fresh),
        camera_near: camera.near,
        camera_far: camera.far,
        camera_fov_vertical: camera.fov_vertical,
    };

    // The FSR dispatch and the copy-back both record onto the engine's immediate context, so hold its
    // mutex across both -- the same lock the rest of the payload's D3D11 work takes.
    // SAFETY: `context.m_Mutex` guards the immediate context; both resources are valid for the call.
    unsafe {
        windows::Win32::System::Threading::EnterCriticalSection(context.m_Mutex);
        let ran = eye_state.context.dispatch(&info);
        if ran {
            // Copy the AA'd result back into the chain's working slot so the rest of the post chain
            // (and the back-buffer capture) sees it.
            context
                .m_Context
                .CopyResource(&slot_color.m_Texture, &output_res);
        }
        windows::Win32::System::Threading::LeaveCriticalSection(context.m_Mutex);
        ran
    }
}

/// Build one eye's FSR context + UAV output texture at `width`x`height`, matching the chain color's
/// format. Returns `None` on any failure (the caller then falls back to engine AA for this eye).
fn create_eye(device: &Device, slot_color: &Texture, width: u32, height: u32) -> Option<EyeState> {
    // SAFETY: `device.m_Device` is the live engine D3D11 device.
    let d3d = &device.m_Device;

    let mut output: Option<ID3D11Texture2D> = None;
    // The output is sampled by the copy-back and written by FSR, so it needs both bind flags.
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT(slot_color.m_Format as i32),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_UNORDERED_ACCESS.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    // SAFETY: valid device + desc; output receives the created texture.
    if unsafe { d3d.CreateTexture2D(&desc, None, Some(&mut output)) }.is_err() {
        tracing::error!("FSR: failed to create the output texture");
        return None;
    }
    let output = output?;

    // Native AA: render size == display size. The HDR / depth / exposure flags match how we feed FSR
    // (post-tonemap LDR for now, so no HDR flag; reverse-Z infinite-far depth; FSR auto-exposure).
    let flags = init_flags::DEPTH_INVERTED | init_flags::DEPTH_INFINITE | init_flags::AUTO_EXPOSURE;
    let extent = Extent { width, height };
    let context = Context::new(d3d, flags, extent, extent)?;
    Some(EyeState {
        context,
        output,
        fresh: true,
    })
}

/// The render camera's parameters FSR's depth reprojection needs this frame.
struct CameraParams {
    near: f32,
    far: f32,
    /// Vertical field of view in radians.
    fov_vertical: f32,
    frame_time_delta_ms: f32,
}

/// Fallbacks when the camera / clock singletons aren't reachable yet.
const FALLBACK_NEAR: f32 = 0.1;
const FALLBACK_FAR: f32 = 10000.0;
const FALLBACK_FOV_VERTICAL: f32 = std::f32::consts::FRAC_PI_2;
const FALLBACK_FRAME_MS: f32 = 1000.0 / 60.0;

/// Read the live render camera's near/far/FOV and the real frame time. The vertical FOV is derived
/// from the projection's `data[5]` (`= 1/tan(fovV/2)`) rather than `m_FOV`, which sidesteps the
/// horizontal-vs-vertical question and is invariant under the jitter/reverse-Z we apply.
fn camera_params() -> CameraParams {
    // SAFETY: the camera-manager and clock singletons are valid once the engine is initialised; both
    // accessors null-check the underlying pointer.
    unsafe {
        let mut params = CameraParams {
            near: FALLBACK_NEAR,
            far: FALLBACK_FAR,
            fov_vertical: FALLBACK_FOV_VERTICAL,
            frame_time_delta_ms: FALLBACK_FRAME_MS,
        };
        if let Some(cm) = jc3gi::camera::camera_manager::CameraManager::get()
            && let Some(camera) = cm.GetRenderCamera().as_ref()
        {
            params.near = camera.m_Near;
            params.far = camera.m_Far;
            let focal_y = camera.m_Projection.data[5];
            if focal_y.abs() > f32::EPSILON {
                params.fov_vertical = 2.0 * (1.0 / focal_y).atan();
            }
        }
        if let Some(clock) = jc3gi::clock::Clock::get()
            && clock.m_RealSPF > 0.0
        {
            params.frame_time_delta_ms = clock.m_RealSPF * 1000.0;
        }
        params
    }
}

/// The FSR `motionVectorScale` for this frame. FSR multiplies the sampled motion vectors by this to
/// reach its internal pixel convention; the right factor depends on the units JC3's velocity buffer
/// holds, which isn't yet pinned. The reference SDK uses `(-renderWidth, renderHeight)` for
/// NDC-encoded vectors -- exposed as a live debug knob (sign + magnitude) so the convention can be
/// calibrated against on-screen motion. See `config.fsr.mv_scale`.
fn motion_vector_scale(render_width: u32, render_height: u32) -> (f32, f32) {
    let cfg = crate::config::Config::lock_query(|c| c.fsr.mv_scale);
    let base = match cfg.basis {
        crate::config::MvScaleBasis::Ndc => (render_width as f32, render_height as f32),
        crate::config::MvScaleBasis::Unit => (1.0, 1.0),
    };
    (base.0 * cfg.x, base.1 * cfg.y)
}

/// FSR's sub-pixel jitter offset for the current frame, in **pixel space** (FSR's native unit, fed
/// straight to the dispatch's `jitter`). `None` when the resolution is degenerate.
///
/// The phase index is the engine's per-real-frame counter (`m_FrameIndex`), so both eye dispatches in
/// one frame share the same offset -- each eye's own FSR history then sees a clean per-frame-advancing
/// Halton sequence. The applied camera jitter ([`apply_jitter_to_projection`]) reads the same counter,
/// so the two always agree.
pub fn current_jitter(render_width: u32, render_height: u32) -> Option<(f32, f32)> {
    if render_width == 0 || render_height == 0 {
        return None;
    }
    if !crate::config::Config::lock_query(|c| c.fsr.jitter) {
        return None;
    }
    let phase_count = fsr_sys::jitter_phase_count(render_width as i32, render_width as i32);
    if phase_count <= 0 {
        return None;
    }
    // SAFETY: the render-frame counters live for the process; advanced once per frame in the prologue.
    let index = unsafe {
        jc3gi::graphics_engine::graphics_engine::get_render_frame_counters().m_FrameIndex as i32
    };
    Some(fsr_sys::jitter_offset(index, phase_count))
}

/// Post-multiply FSR's sub-pixel jitter onto `proj` (in place), converting FSR's pixel-space offset to
/// a clip-space translation: `proj' = proj * translate(2*jx/w, -2*jy/h, 0)`. This matches the engine's
/// own `ApplySubsampleJitter` idiom (translation on the right, row-major), so it slots in where the
/// engine's TAA jitter would have gone. The FSR docs' `-2*jy/h` sign (negative Y) is preserved.
pub fn apply_jitter_to_projection(proj: &mut Matrix4, render_width: u32, render_height: u32) {
    let Some((jx, jy)) = current_jitter(render_width, render_height) else {
        return;
    };
    let ndc_x = 2.0 * jx / render_width as f32;
    let ndc_y = -2.0 * jy / render_height as f32;

    // Identity with the jitter in the row-major translation row (data[12], data[13]).
    let jitter = Matrix4 {
        #[rustfmt::skip]
        data: [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            ndc_x, ndc_y, 0.0, 1.0,
        ],
    };
    // proj = proj * jitter, the engine's Multiply4x4(proj, jitterMat) convention.
    let mut out = Matrix4::default();
    // SAFETY: the engine matrix-multiply reads two valid matrices and writes a third.
    unsafe { Matrix4::Multiply4x4(proj, &jitter, &mut out) };
    *proj = out;
}
