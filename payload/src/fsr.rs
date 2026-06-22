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

    let info = DispatchInfo {
        context: &context.m_Context,
        color,
        depth: depth_res,
        motion_vectors: velocity_res,
        exposure: None,
        output: &output_res,
        render_size: Extent { width, height },
        // The same per-frame jitter applied to the camera projection (see apply_jitter_to_projection),
        // in FSR's pixel space. Per-eye motion-vector scale is still identity (wired up next).
        jitter: current_jitter(width, height).unwrap_or((0.0, 0.0)),
        motion_vector_scale: (1.0, 1.0),
        sharpening: sharpness,
        frame_time_delta_ms: 1000.0 / 60.0,
        pre_exposure: 1.0,
        reset: false,
        camera_near: NEAR_PLANE,
        camera_far: FAR_PLANE,
        camera_fov_vertical: DEFAULT_FOV_VERTICAL,
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
    Some(EyeState { context, output })
}

// Camera constants for the dispatch. Provisional -- the real per-eye projection values get threaded
// through once the camera path feeds them in.
const NEAR_PLANE: f32 = 0.1;
const FAR_PLANE: f32 = 10000.0;
const DEFAULT_FOV_VERTICAL: f32 = std::f32::consts::FRAC_PI_2;

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
