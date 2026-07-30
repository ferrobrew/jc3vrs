//! The mod-owned `cb13` constant buffer: the per-eye view-projection rows, camera positions, and
//! `M_eye` reprojection matrices every rewritten vertex shader reads its eye from.
//!
//! This is the data half of single-pass stereo. The shader rewrites decide *how* a shader reads its
//! eye; everything here decides *what* it reads -- how the rows are derived from the engine's own
//! camera constants, how they are uploaded and bound, and how they are pinned to one eye for a
//! re-issue that has to submit each eye separately.

use super::*;
use crate::vr::render_params;

/// The stereo constant buffer's register slot (`b13`, free across the game's vertex shaders) and its
/// size in float4 rows (five per eye: four view-projection rows then the camera position, two eyes).
const STEREO_CB_REGISTER: u32 = 13;
/// The `cb0`-remap block: five rows per eye (`dxbc_stereo::STEREO_CB_ROWS`).
const STEREO_CB_ROWS: usize = 10;
/// The row where the reprojection `M_eye` block begins (`dxbc_stereo::MEYE_ROW_BASE`).
const MEYE_ROW_BASE: usize = 10;
/// The full `cb13` size: the remap block plus a four-rows-per-eye `M_eye` block for the reprojection
/// rewrite (`dxbc_stereo::STEREO_REPROJ_CB_ROWS`). Both idioms bind the same `b13` buffer.
const STEREO_CB_TOTAL_ROWS: usize = 18;

// Keep the payload's cb13 layout in lockstep with the rewriter that reads it.
const _: () = {
    assert!(STEREO_CB_ROWS == dxbc_stereo::STEREO_CB_ROWS as usize);
    assert!(MEYE_ROW_BASE == dxbc_stereo::MEYE_ROW_BASE as usize);
    assert!(STEREO_CB_TOTAL_ROWS == dxbc_stereo::STEREO_REPROJ_CB_ROWS as usize);
    assert!(STEREO_CB_REGISTER == dxbc_stereo::STEREO_CB_REGISTER);
};

/// The `cb0` (`m_VPGlobalConstData`) rows the patched shaders read per eye, in the order the rewrite
/// lays them out in `cb13`: the four translation-free view-projection rows (`cb0[29..32]`), then the
/// camera world position (`cb0[4]`). See `dxbc_stereo::PER_EYE_CB0_ROWS`.
const PER_EYE_SOURCE_ROWS: [usize; 5] = [29, 30, 31, 32, 4];

/// Mirror the current view's per-eye `cb0` rows into the mod-owned `cb13` and bind it at `b13`.
///
/// Outside the diverging case both eye slots get the **same** (current-view) rows, so a patched
/// vertex shader -- which reads its position from `cb13` instead of `cb0` -- renders exactly what it
/// would have from `cb0`, in *every* pass (the G-buffer, but also the shadow and reflection passes
/// that reuse the same model shaders under a different view). That shadow-safety is why `cb13` tracks
/// whatever view is current rather than being written once.
///
/// Called from the `SetAllGlobalShaderProgramConstants` detour, after the engine has refreshed
/// `m_VPGlobalConstData` and uploaded `cb0`, on the render thread.
pub fn mirror_and_bind_cb13(engine: &RenderEngine) {
    // Ensure the viewport-duplication detours are installed (once, on the first active frame).
    ensure_viewport_detours();

    // During the main-scene G-buffer range, fill the two eye slots with *distinct* per-eye
    // view-projections so the eyes diverge. Everywhere else (the shadow and reflection passes, and
    // whenever divergence is off) mirror the current view into both slots -- diverging those would be
    // wrong, since they render from the sun or reflection camera, not the eye camera.
    let rows = if dual_eye_active() && in_gbuffer_range() {
        compute_dual_eye_rows(engine).unwrap_or_else(|| mirror_rows(engine))
    } else {
        mirror_rows(engine)
    };

    // SAFETY: `GraphicsEngine::get` returns the live singleton or `None`; the device/context pointers
    // are stable once the engine has initialised, and the ops run under the engine's context mutex.
    unsafe {
        let Some(ge) = GraphicsEngine::get() else {
            return;
        };
        let Some(device) = ge.m_Device.as_ref() else {
            return;
        };
        let Some(context) = device.m_Context.as_ref() else {
            return;
        };
        EnterCriticalSection(context.m_Mutex);
        let result = CB13
            .lock()
            .upload_and_bind(&device.m_Device, &context.m_Context, &rows);
        LeaveCriticalSection(context.m_Mutex);
        if let Err(e) = result {
            tracing::warn!("single-pass cb13: {e}");
        }
    }
}

/// Mirror the current view's per-eye `cb0` rows into both `cb13` eye slots (the non-scene passes, and
/// any frame where divergence is off): a patched shader then renders exactly what it would from
/// `cb0`. The `M_eye` reprojection block is left at identity, so a reprojected shader is a no-op here
/// too.
fn mirror_rows(engine: &RenderEngine) -> [Vector4; STEREO_CB_TOTAL_ROWS] {
    let vp = &engine.m_VPGlobalConstData;
    let mut rows = [Vector4::default(); STEREO_CB_TOTAL_ROWS];
    for eye in 0..2 {
        for (k, &src) in PER_EYE_SOURCE_ROWS.iter().enumerate() {
            rows[eye * PER_EYE_SOURCE_ROWS.len() + k] = vp[src];
        }
        write_meye(&mut rows, eye, glam::Mat4::IDENTITY);
    }
    rows
}

/// Write eye `e`'s reprojection matrix into the `M_eye` block (`cb13[MEYE_ROW_BASE + 4*e ..]`), one
/// glam row per `cb13` row. The reprojection rewrite reads these with `dp4 o0.{xyzw}, cb13[row],
/// rClip`, so each `cb13` row must be a row of `M_eye` acting on the clip column vector.
fn write_meye(rows: &mut [Vector4; STEREO_CB_TOTAL_ROWS], eye: usize, m_eye: glam::Mat4) {
    for r in 0..4 {
        rows[MEYE_ROW_BASE + eye * 4 + r] = Vector4 {
            data: m_eye.row(r).to_array(),
        };
    }
}

/// Compute distinct per-eye `cb13` rows from the pristine center render-camera transform and the
/// per-eye [`EyeRenderParams`](crate::vr::frame::EyeRenderParams), replicating the double-draw's
/// per-eye camera math (`hooks/camera.rs`) purely in mod code -- so the single walk produces both
/// eyes. Returns `None` (falling back to the mirror) if the center transform or per-eye params are
/// not available this frame.
///
/// Per eye: offset the center world transform by the eye parallax + orientation delta, invert to a
/// view, zero its translation for the camera-relative OffsetVP, multiply by the reverse-Z eye
/// projection, and pair it with the eye's camera world position (`center campos + world_offset`).
/// The engine `Matrix4` <-> `glam::Mat4` bridge is a transpose, so the math is done in glam
/// column-vector form and converted back once (see the `Matrix4` doc-comment).
fn compute_dual_eye_rows(engine: &RenderEngine) -> Option<[Vector4; STEREO_CB_TOTAL_ROWS]> {
    let center_transform = crate::stereo::STEREO_STATE.lock().center_transform?;
    let center_world = glam::Mat4::from(center_transform);
    let center_campos = engine.m_VPGlobalConstData[4];

    // The reprojection `M_eye = VP_eye · VP_center⁻¹` needs the engine's *center* full view-projection
    // (world -> clip, column-vector) -- the one the baked-WVP shaders folded into their `cb1`. It is
    // `cb0[29..32]` (the translation-free OffsetVP, stored row-major so `glam::Mat4::from` -- which is
    // `from_cols_array` -- yields its transpose, the column-vector form) composed with the
    // camera-relative `−campos` translation. Inverted in f64: the reverse-Z VP is near-singular.
    let center_offset_vp = {
        let mut data = [0.0f32; 16];
        for r in 0..4 {
            data[r * 4..r * 4 + 4].copy_from_slice(&engine.m_VPGlobalConstData[29 + r].data);
        }
        glam::Mat4::from(Matrix4 { data })
    };
    let center_campos_v = glam::Vec3::new(
        center_campos.data[0],
        center_campos.data[1],
        center_campos.data[2],
    );
    let vp_center = center_offset_vp * glam::Mat4::from_translation(-center_campos_v);
    let vp_center_inv = vp_center.as_dmat4().inverse();

    let mut rows = [Vector4::default(); STEREO_CB_TOTAL_ROWS];
    let mut forwards = [glam::Vec3::ZERO; 2];
    let mut m_eyes = [glam::Mat4::IDENTITY; 2];
    for eye in 0..2 {
        let params = render_params(eye)?;

        let mut eye_world = center_world;
        eye_world.w_axis += params.world_offset.extend(0.0);
        let eye_world = eye_world * glam::Mat4::from_quat(params.orientation_delta);
        // Camera forward is -Z of the world transform; kept for the divergence diagnostic below.
        forwards[eye] = (-eye_world.z_axis.truncate()).normalize_or_zero();

        let mut offset_view = eye_world.inverse();
        offset_view.w_axis = glam::Vec4::new(0.0, 0.0, 0.0, 1.0);

        let offset_vp_glam = glam::Mat4::from(params.projection_reverse_z) * offset_view;
        let offset_vp = Matrix4::from(offset_vp_glam);

        for r in 0..4 {
            rows[eye * 5 + r] = Vector4 {
                data: [
                    offset_vp.data[r * 4],
                    offset_vp.data[r * 4 + 1],
                    offset_vp.data[r * 4 + 2],
                    offset_vp.data[r * 4 + 3],
                ],
            };
        }
        let eye_campos = glam::Vec3::new(
            center_campos.data[0] + params.world_offset.x,
            center_campos.data[1] + params.world_offset.y,
            center_campos.data[2] + params.world_offset.z,
        );
        rows[eye * 5 + 4] = Vector4 {
            data: [
                eye_campos.x,
                eye_campos.y,
                eye_campos.z,
                center_campos.data[3],
            ],
        };

        // M_eye maps this eye's own centre-clip to eye-clip: build the eye's full VP the same way as
        // the centre (offset VP composed with −campos) and post-compose the centre's inverse.
        let vp_eye = offset_vp_glam * glam::Mat4::from_translation(-eye_campos);
        let m_eye = (vp_eye.as_dmat4() * vp_center_inv).as_mat4();
        write_meye(&mut rows, eye, m_eye);
        m_eyes[eye] = m_eye;
    }
    // Publish the per-eye reprojection matrices for the render-block-level intercepts (terrain detail),
    // which apply `M_eye` on the CPU to a per-draw constant buffer rather than through a rewritten shader.
    *CURRENT_M_EYE.lock() = Some(m_eyes);

    // Diagnostic (rate-limited): the angle between the two eyes' forward vectors. A stereo pair should
    // diverge only by the display cant (a few degrees on the Index) -- a large value means a per-eye
    // matrix bug rather than the canted-runtime views that merely look divergent on a flat capture.
    if let (Some(p0), Some(p1)) = (render_params(0), render_params(1)) {
        let diverge = forwards[0]
            .dot(forwards[1])
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees();

        // Flatten one eye's four `cb13` view-projection rows into a 16-float matrix.
        let vp16 = |base: usize| {
            let mut m = [0.0f32; 16];
            for r in 0..4 {
                m[r * 4..r * 4 + 4].copy_from_slice(&rows[base + r].data);
            }
            m
        };
        let eye_diag = |p: &crate::vr::EyeRenderParams, i: usize| EyeDiagnostics {
            world_offset: p.world_offset.to_array(),
            orientation_delta_quat: p.orientation_delta.to_array(),
            orientation_delta_deg: p.orientation_delta.to_axis_angle().1.to_degrees(),
            forward: forwards[i].to_array(),
            projection_reverse_z: p.projection_reverse_z.data,
            cb13_view_projection: vp16(i * 5),
            cb13_camera_position: rows[i * 5 + 4].data,
            cb13_m_eye: vp16(MEYE_ROW_BASE + i * 4),
        };
        let full_viewport = COLLAPSE_FULL_VIEWPORT.lock().map(|v| {
            [
                v.TopLeftX, v.TopLeftY, v.Width, v.Height, v.MinDepth, v.MaxDepth,
            ]
        });
        *LAST_FRAME_DIAG.lock() = Some(FrameDiagnostics {
            single_pass: active(),
            dual_eye: dual_eye_active(),
            collapse: collapse_active(),
            double_wide: double_wide_active(),
            capability: match capability() {
                Capability::Supported => "supported",
                Capability::Unsupported => "unsupported",
                Capability::Unprobed => "unprobed",
            },
            full_viewport,
            center_transform: center_transform.data,
            center_camera_position: center_campos.data,
            forward_divergence_deg: diverge,
            substitution: substitution_stats(),
            eyes: [eye_diag(&p0, 0), eye_diag(&p1, 1)],
        });

        if CB13_DIVERGE_LOG
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(240)
        {
            // Max |M_eye - I|: the reprojection matrix is near-identity for a small IPD and cant, so a
            // large deviation flags a construction bug (a wrong VP convention or a bad inverse).
            let meye_dev = |base: usize| {
                let m = vp16(base);
                (0..16)
                    .map(|k| (m[k] - if k % 5 == 0 { 1.0 } else { 0.0 }).abs())
                    .fold(0.0f32, f32::max)
            };
            tracing::info!(
                target: "single_pass",
                "cb13 eyes: fwd divergence={diverge:.2}deg | eye0 delta={:.2}deg off={:.4?} | eye1 delta={:.2}deg off={:.4?} | M_eye dev eye0={:.4} eye1={:.4}",
                p0.orientation_delta.to_axis_angle().1.to_degrees(), p0.world_offset,
                p1.orientation_delta.to_axis_angle().1.to_degrees(), p1.world_offset,
                meye_dev(MEYE_ROW_BASE), meye_dev(MEYE_ROW_BASE + 4),
            );
        }
    }

    Some(rows)
}

static CB13_DIVERGE_LOG: AtomicUsize = AtomicUsize::new(0);

/// The mod-owned `cb13` constant buffer, lazily created and updated per view.
pub(super) struct Cb13Buffer {
    buffer: Option<ID3D11Buffer>,
    /// The rows last uploaded through [`upload_and_bind`](Self::upload_and_bind) -- the dual-eye state
    /// every patched shader in the pass reads. Kept so the already-instanced per-eye re-issue can pin
    /// `cb13` to one eye for a single submission and put this back afterwards; the re-issue runs from a
    /// draw detour, which has no `RenderEngine` to recompute the rows from.
    rows: Option<[Vector4; STEREO_CB_TOTAL_ROWS]>,
}

impl Cb13Buffer {
    /// The dual-eye rows currently in the live buffer, or `None` before the first upload -- the state a
    /// per-eye pin must be able to restore before it is allowed to pin at all.
    pub(super) fn live_rows(&self) -> Option<[Vector4; STEREO_CB_TOTAL_ROWS]> {
        self.buffer.as_ref()?;
        self.rows
    }

    /// Overwrite the live buffer's contents without re-binding it.
    ///
    /// The already-instanced per-eye re-issue pins and restores `cb13` around every handled draw, so
    /// this runs three times per such draw: a `WRITE_DISCARD` map is all it needs (the buffer stays
    /// bound at `b13`, and renaming a bound dynamic buffer is precisely what that usage is for), where
    /// [`upload_and_bind`](Self::upload_and_bind)'s re-bind would cost an `AddRef`/`Release` pair per
    /// shader stage per call. A no-op before the buffer exists.
    unsafe fn write(
        &self,
        context: &ID3D11DeviceContext,
        rows: &[Vector4; STEREO_CB_TOTAL_ROWS],
    ) -> Result<(), windows::core::Error> {
        let Some(buffer) = &self.buffer else {
            return Ok(());
        };
        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
            std::ptr::copy_nonoverlapping(rows.as_ptr(), mapped.pData.cast(), STEREO_CB_TOTAL_ROWS);
            context.Unmap(buffer, 0);
        }
        Ok(())
    }

    /// Ensure the dynamic `cb13` buffer exists, write `rows` into it, and bind it at `b13`.
    unsafe fn upload_and_bind(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        rows: &[Vector4; STEREO_CB_TOTAL_ROWS],
    ) -> Result<(), windows::core::Error> {
        let byte_width = std::mem::size_of_val(rows) as u32;
        let buffer = match &self.buffer {
            Some(buffer) => buffer,
            None => {
                let mut created = None;
                unsafe {
                    device.CreateBuffer(
                        &D3D11_BUFFER_DESC {
                            ByteWidth: byte_width,
                            Usage: D3D11_USAGE_DYNAMIC,
                            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                            ..Default::default()
                        },
                        Some(&D3D11_SUBRESOURCE_DATA {
                            pSysMem: rows.as_ptr().cast(),
                            ..Default::default()
                        }),
                        Some(&mut created),
                    )?;
                }
                self.buffer
                    .insert(created.expect("CreateBuffer returned Ok with no buffer"))
            }
        };

        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
            std::ptr::copy_nonoverlapping(rows.as_ptr(), mapped.pData.cast(), STEREO_CB_TOTAL_ROWS);
            context.Unmap(buffer, 0);
            context.VSSetConstantBuffers(STEREO_CB_REGISTER, Some(&[Some(buffer.clone())]));
            // The terrain domain shader also reads `cb13` (its per-eye `M_eye` reprojection block), so
            // bind the same buffer at `b13` on the domain stage. The hull shader only forwards the eye
            // lane and reads nothing from `cb13`, so it needs no binding.
            context.DSSetConstantBuffers(STEREO_CB_REGISTER, Some(&[Some(buffer.clone())]));
        }
        self.rows = Some(*rows);
        Ok(())
    }
}

/// Copy eye `eye`'s view-projection rows, camera position, and `M_eye` block over **both** eye slots of
/// a `cb13` row set. A patched shader then reads the same eye whichever parity it computes.
pub(super) fn pin_rows_to_eye(
    rows: &[Vector4; STEREO_CB_TOTAL_ROWS],
    eye: usize,
) -> [Vector4; STEREO_CB_TOTAL_ROWS] {
    let stride = PER_EYE_SOURCE_ROWS.len();
    let mut pinned = *rows;
    for slot in 0..2 {
        pinned[slot * stride..(slot + 1) * stride]
            .copy_from_slice(&rows[eye * stride..(eye + 1) * stride]);
        pinned[MEYE_ROW_BASE + slot * 4..MEYE_ROW_BASE + slot * 4 + 4]
            .copy_from_slice(&rows[MEYE_ROW_BASE + eye * 4..MEYE_ROW_BASE + eye * 4 + 4]);
    }
    pinned
}

/// Write `rows` into the live `cb13` buffer, reporting whether it landed. Takes the engine context
/// mutex before [`CB13`], the same order as [`mirror_and_bind_cb13`].
pub(super) fn write_cb13_rows(d3d: EngineContext, rows: &[Vector4; STEREO_CB_TOTAL_ROWS]) -> bool {
    // SAFETY: runs on the render thread under the engine's context mutex, as every other `cb13` write.
    let result = d3d.with_lock(|ctx| unsafe { CB13.lock().write(ctx, rows) });
    if let Err(e) = result {
        if !CB13_PIN_WARNED.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                target: "single_pass",
                "per-eye re-issue of an already-instanced draw could not map cb13 ({e}); those draws \
                 stay split between the eyes"
            );
        }
        return false;
    }
    true
}

/// Whether [`write_cb13_rows`] has already reported a map failure, so a per-draw path cannot flood the
/// log with the same warning.
static CB13_PIN_WARNED: AtomicBool = AtomicBool::new(false);

/// Unbind the mod-owned `cb13` from `b13` on the vertex and domain stages and release the buffer.
///
/// Rust statics are not dropped and `FreeLibrary` runs no destructors, so without this the buffer
/// leaks on every inject/eject cycle *and* stays bound at `b13` on a clean game -- a mod-owned
/// resource outliving the mod. Called from [`uninstall_com_detours`], after the detours are disabled
/// (so the unbind goes straight to the real D3D entry points) and while the device is still alive.
pub(super) fn release_cb13() {
    // SAFETY: runs on the eject path with the engine still live; the ops run under the engine's
    // context mutex, as everywhere else that touches the immediate context.
    unsafe {
        if let Some(ge) = GraphicsEngine::get()
            && let Some(device) = ge.m_Device.as_ref()
            && let Some(context) = device.m_Context.as_ref()
        {
            EnterCriticalSection(context.m_Mutex);
            context
                .m_Context
                .VSSetConstantBuffers(STEREO_CB_REGISTER, Some(&[None]));
            context
                .m_Context
                .DSSetConstantBuffers(STEREO_CB_REGISTER, Some(&[None]));
            LeaveCriticalSection(context.m_Mutex);
        }
    }
    let mut cb13 = CB13.lock();
    cb13.buffer = None;
    cb13.rows = None;
}

pub(super) static CB13: Mutex<Cb13Buffer> = Mutex::new(Cb13Buffer {
    buffer: None,
    rows: None,
});
