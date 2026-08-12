//! Re-issuing a render block's own `Draw` once per eye, for the geometry the collapse cannot make
//! stereo any other way.
//!
//! The general path is a rewritten vertex shader: one instanced draw, both eyes, no CPU cost. These
//! blocks cannot take it. They bake their view-projection into a constant buffer inside their own
//! `Draw`, so there is nothing in the shader for the rewrite to retarget -- and several of them draw
//! GPU-indirect, where the instance count lives in a buffer and `SV_InstanceID` never reaches two.
//!
//! So the block draws twice, and what changes between the runs is the constant it bakes: this module
//! arms a detour on the engine's own constant upload, post-multiplies the block's matrix by that eye's
//! `M_eye` on its way through, and pins the eye's half-viewport around the call. The block's shader
//! stays pristine, which is also why it must not be rewritten -- see `shader_policy`.

use super::*;
use crate::hooks::graphics_engine::clustered_lighting;

/// One eye's re-issue of a terrain-detail draw: the reprojected `cb1` (four float4 rows) to stage on
/// vertex slot 1, and the eye-half viewport to render into.
pub(super) struct TerrainDetailEyePass {
    cb1: [f32; 16],
}

/// The per-eye passes for a terrain-detail draw, or `None` when the single-pass terrain intercept
/// should not run -- the same gate every other per-eye re-issue takes
/// ([`baked_cb_intercept_ready`]: the collapse, the G-buffer range, and a published `M_eye` and
/// viewport), plus the terrain flag.
///
/// The detail draw is GPU-indirect, so it cannot be instance-doubled like the model geometry; instead
/// the render block's `Draw` is re-issued once per eye with a per-eye `cb1`. The detail VS builds clip
/// with a multiply-add chain over `cb1[0..3]` (`clip = Σ_i P_local[i] · cb1[i]`), so those four
/// registers are the *columns* of `T_patch · OffsetVP` (`T_patch` translating the patch origin relative
/// to the camera) and the per-eye buffer is the column-wise `cb1_eye[k] = M_eye · cb1_center[k]`.
/// `cb1[4]` (the LOD-fade position) is left untouched. `this` and `rc` are the
/// [`RenderBlockTerrainDetail`] and [`RenderContext`] the block's `Draw` received.
///
/// # Safety
///
/// `this` and `rc` must be the live pointers the detoured `Draw` received.
pub(super) unsafe fn terrain_detail_eye_passes(
    this: *const RenderBlockTerrainDetail,
    rc: *const RenderContext,
) -> Option<(
    [TerrainDetailEyePass; 2],
    [f32; 16],
    D3D11_VIEWPORT,
    EngineContext,
)> {
    if !terrain_active() {
        return None;
    }
    let (m_eye, full, d3d) = baked_cb_intercept_ready("terrain-detail", BoundVsGate::Checked)?;

    // SAFETY: caller guarantees live pointers.
    let ovp = unsafe { (*rc).m_OffsetViewProjection.data };
    let cam = unsafe { (*rc).m_CameraPosition.data };
    let (patch_x, patch_z) = unsafe { ((*this).m_WorldPatchX, (*this).m_WorldPatchZ) };

    let row =
        |r: usize| glam::Vec4::new(ovp[r * 4], ovp[r * 4 + 1], ovp[r * 4 + 2], ovp[r * 4 + 3]);
    let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
    // The engine stores `Matrix4` row-major, so `ovp` row `k` is column `k` of the column-vector
    // `OffsetVP` -- exactly the entry the VS's mad chain wants at `cb1[k]`. The fourth column folds in
    // the patch-relative camera translation `T_patch`.
    let (tx, ty, tz) = (patch_x - cam[0], -cam[1], patch_z - cam[2]);
    let cb1_center = [r0, r1, r2, r3 + tx * r0 + ty * r1 + tz * r2];

    let passes = std::array::from_fn(|eye| {
        let mut cb1 = [0.0f32; 16];
        for (k, center) in cb1_center.iter().enumerate() {
            cb1[k * 4..k * 4 + 4].copy_from_slice(&m_eye[eye].mul_vec4(*center).to_array());
        }
        TerrainDetailEyePass { cb1 }
    });
    let mut cb1_center_rows = [0.0f32; 16];
    for (k, center) in cb1_center.iter().enumerate() {
        cb1_center_rows[k * 4..k * 4 + 4].copy_from_slice(&center.to_array());
    }
    Some((passes, cb1_center_rows, full, d3d))
}

/// The graphics context (`HContext_t*`) a render block's `Draw` stages constants into, read from its
/// [`RenderContext`]. Used by the terrain-detail intercept to call `SetVertexProgramConstants`.
///
/// # Safety
///
/// `rc` must be a live pointer.
pub(super) unsafe fn render_context_graphics_context(rc: *const RenderContext) -> *mut HContext_t {
    unsafe { (*rc).m_Context }
}

/// Re-issue a terrain-detail `Draw` once per eye with a per-eye `cb1` and the eye's half-viewport,
/// calling `draw` (the block's original `Draw` trampoline) each time. Returns `false` when the
/// single-pass terrain intercept should not run, in which case the caller draws normally once. The
/// detail draw is GPU-indirect and so cannot be instance-doubled; this drives per-eye rendering from
/// the CPU instead. See [`terrain_detail_eye_passes`].
///
/// # Safety
///
/// `this` and `rc` must be the live pointers the detoured `Draw` received, and `draw` must invoke the
/// original `Draw`.
pub unsafe fn terrain_detail_per_eye(
    this: *const RenderBlockTerrainDetail,
    rc: *const RenderContext,
    mut draw: impl FnMut(),
) -> bool {
    let Some((passes, cb1_center, full, d3d)) = (unsafe { terrain_detail_eye_passes(this, rc) })
    else {
        return false;
    };
    // SAFETY: `rc` is live per the caller contract.
    let ctx = unsafe { render_context_graphics_context(rc) };
    per_eye_halves(full, d3d, &mut |eye| {
        // SAFETY: `ctx` is the render context's live graphics context; `cb1` is four float4 rows.
        unsafe { SetVertexProgramConstants(ctx, 1, 0, passes[eye].cb1.as_ptr(), 4) };
        draw();
    });
    // Put the centre transform back, as `screen_uv_cb_per_eye` does with the rows it biases. The
    // block type stages this constant once per pass rather than per draw, so leaving eye 1's
    // reprojection behind hands it to anything later in the pass that reads vertex `cb1[0..3]` --
    // including a terrain-detail draw this intercept declines, which it can do per draw, since
    // `BoundVsGate::Checked` reads the live bound-shader flag.
    // SAFETY: as above; `cb1_center` is the same four float4 rows, un-reprojected.
    unsafe { SetVertexProgramConstants(ctx, 1, 0, cb1_center.as_ptr(), 4) };
    true
}

// ---- Baked-cb per-eye re-issue -----------------------------------------------------------------
//
// The generalization of the terrain-detail intercept above, for the render blocks (bark, foliage,
// occluder) that bake their view-projection into a constant buffer inside their own `Draw` -- across
// draw kinds that cannot be instance-doubled (CPU-instanced, GPU-indirect). Rather than replicate each
// block's bake, [`reproject_baked_cb_per_eye`] re-issues the block's whole `Draw` once per eye and, for
// the duration of each call, arms [`set_vertex_program_constants_detour`] to post-multiply the block's
// own constant upload by that eye's `M_eye`.

/// A pending reprojection of a render block's baked view-projection constant, armed around a per-eye
/// re-issue. While armed, the game's stage of the four `float4` entries at (`cb_index`, `reg_offset`)
/// -- the columns of the baked matrix -- is reprojected by `m_eye`.
#[derive(Clone, Copy)]
pub(super) struct ReprojectUpload {
    /// The graphics context (`HContext_t*`) the re-issued block stages into. The detour sees *every*
    /// vertex-constant stage in the process, and `(slot, offset)` alone does not identify the block --
    /// `RenderBlockTerrainPatch` also stages four rows at vertex `cb1` offset 0 -- so a stage from any
    /// other context in the armed window would otherwise be reprojected by this eye's matrix. Held as
    /// an address rather than a pointer so the static stays `Sync`; it is only ever compared.
    ctx: usize,
    cb_index: i32,
    reg_offset: u32,
    m_eye: glam::Mat4,
}

/// `eye + 1` while a render-block per-eye re-issue is in flight, `0` otherwise.
///
/// A re-issue re-drives the block's whole `Draw`, so every draw and viewport call the block makes
/// passes back through this module's own detours. Without a marker they would compound the split the
/// re-issue just set up: a patched `DrawIndexed` inside would be instance-doubled and re-split
/// (drawing the geometry twice in each eye), and any `Draw` or single-slot `RSSetViewports` would
/// restore the full width, un-splitting the eye half mid-re-issue.
pub(super) static PER_EYE_REISSUE: AtomicUsize = AtomicUsize::new(0);

/// The eye whose per-eye re-issue is currently in flight, or `None` outside one.
pub(super) fn per_eye_reissue_eye() -> Option<usize> {
    match PER_EYE_REISSUE.load(Ordering::Acquire) {
        0 => None,
        marker => Some(marker - 1),
    }
}

/// Raises [`PER_EYE_REISSUE`] for one eye for as long as it lives, carrying the previous marker so a
/// nested re-issue restores rather than clears it.
pub(super) struct PerEyeReissue(usize);

impl PerEyeReissue {
    /// Saves and restores the previous marker rather than clearing it, the same shape
    /// [`set_current_pass`] uses. Nothing today re-issues inside a re-issue -- no intercepted block's
    /// `Draw` reaches another intercepted block's `Draw` -- but nothing states or enforces that
    /// either, and clearing would un-guard the remainder of the outer loop the moment it stopped
    /// holding: this module's own draw detours would start splitting geometry the outer re-issue had
    /// already split, which is the doubled-geometry artifact the marker exists to prevent.
    pub(super) fn enter(eye: usize) -> Self {
        let previous = PER_EYE_REISSUE.swap(eye + 1, Ordering::Release);
        Self(previous)
    }
}

impl Drop for PerEyeReissue {
    fn drop(&mut self) {
        PER_EYE_REISSUE.store(self.0, Ordering::Release);
    }
}

/// Fast-path guard for [`set_vertex_program_constants_detour`]: a relaxed load skips the mutex on every
/// un-armed stage (the common case -- the detour sees every VS constant upload in the frame).
pub(super) static REPROJECT_ARMED: AtomicBool = AtomicBool::new(false);
pub(super) static REPROJECT_UPLOAD: Mutex<Option<ReprojectUpload>> = Mutex::new(None);

pub(super) fn arm_reproject(upload: ReprojectUpload) {
    *REPROJECT_UPLOAD.lock() = Some(upload);
    REPROJECT_FIRED.store(false, Ordering::Relaxed);
    REPROJECT_ARMED.store(true, Ordering::Release);
}

/// Disarm, reporting whether the block actually staged the constants the arm was waiting for.
pub(super) fn disarm_reproject() -> bool {
    REPROJECT_ARMED.store(false, Ordering::Release);
    *REPROJECT_UPLOAD.lock() = None;
    REPROJECT_FIRED.load(Ordering::Relaxed)
}

/// Whether the armed reprojection matched a stage before it was disarmed.
///
/// A block that takes a different internal path than the intercept expects -- the occluder's
/// instanced path, selected by the `gfx.occluders.use_instancing` cvar, reads `cb0`'s
/// `m_VPGlobals` with per-instance world rows instead of baking a `cb1` matrix -- stages nothing the
/// arm can reproject, so the re-issue would draw the same mono geometry into both eye halves. This
/// makes that condition observable rather than silent; nothing in the mod forces the cvar, so it is
/// the only thing standing between the intercept and its precondition.
pub(super) static REPROJECT_FIRED: AtomicBool = AtomicBool::new(false);

/// Whether a baked-cb per-eye re-issue has to satisfy itself that the block's vertex shader was not
/// claimed by the `cb0` remap.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BoundVsGate {
    /// Stand down if a patched vertex shader is bound. For a block whose shaders the remap may claim
    /// (patching is by operand detection, not by name), a patched shader already renders both eyes from
    /// one draw: re-issuing would draw the geometry twice per eye, from `cb13`'s eye slot rather than
    /// the reprojected constant.
    Checked,
    /// Run regardless. For a block whose shaders are declined at creation while its own flag is on
    /// ([`baked_cb_block_owns_vs`], a decline both creation paths honour), so none of them can be
    /// patched — and where the check would be
    /// wrong anyway: it reads the shader bound by the *previous* draw, since the block binds its own
    /// inside the `Draw` being wrapped. A bark block drawn between two patched model draws would stand
    /// down on the neighbour's shader and lose its parallax.
    Owned,
}

/// The state a baked-cb per-eye re-issue needs, or `None` when it must not run: the two per-eye `M_eye`
/// matrices, the collapse full viewport, and the immediate context. Requires the collapse (a single
/// centered walk) and the G-buffer pass range -- outside the range the eye-half split does not apply
/// (the shadow-cascade and reflection passes reuse these blocks' `DrawZ`, and eye-splitting a
/// shadow-atlas draw would corrupt it), and outside the collapse re-issuing per eye is wrong.
pub(super) fn baked_cb_intercept_ready(
    site: &'static str,
    gate: BoundVsGate,
) -> Option<([glam::Mat4; 2], D3D11_VIEWPORT, EngineContext)> {
    if gate == BoundVsGate::Checked && BOUND_VS_PATCHED.load(Ordering::Relaxed) {
        warn_intercept_declined_on_patched_vs(site);
        return None;
    }
    eye_split_state()
}

/// Warn, at most once per `site`, that a per-eye intercept stood itself down because a patched vertex
/// shader was bound.
///
/// The gate exists so an intercept and the shader rewrite cannot both claim the same geometry. But a
/// family whose *own* vertex shaders the rewrite claims declines on every draw, for the whole session,
/// silently -- the intercept is then dead code that reports success, and its flag documents a fix that
/// never runs. Which families those are is not decidable from the bytecode alone: it depends on which
/// permutations a session loads and on what happens to be bound when the block draws. So record it
/// from the one place that knows.
///
/// A single line per site over a whole session is the signal that the intercept never ran at all.
pub(super) fn warn_intercept_declined_on_patched_vs(site: &'static str) {
    if INTERCEPT_DECLINED_ON_PATCHED_VS.lock().insert(site) {
        tracing::warn!(
            target: "single_pass",
            "{site} per-eye intercept declined: a patched vertex shader was bound, so the shader \
             rewrite already owns this draw. If this is the only line for {site} all session, the \
             intercept never ran and its flag is documenting a fix that is not happening.",
        );
    }
}

pub(super) static INTERCEPT_DECLINED_ON_PATCHED_VS: Mutex<BTreeSet<&'static str>> =
    Mutex::new(BTreeSet::new());

/// The state every per-eye re-issue needs, without the bound-shader gate: the two per-eye `M_eye`
/// matrices, the collapse full viewport, and the immediate context. Requires the collapse (a single
/// centered walk) and the G-buffer pass range -- outside the range the eye-half split does not apply
/// (the shadow-cascade and reflection passes reuse these blocks' `DrawZ`, and eye-splitting a
/// shadow-atlas draw would corrupt it), and outside the collapse re-issuing per eye is wrong.
pub(super) fn eye_split_state() -> Option<([glam::Mat4; 2], D3D11_VIEWPORT, EngineContext)> {
    if !collapse_active() || !in_gbuffer_range() {
        return None;
    }
    let m_eye = (*CURRENT_M_EYE.lock())?;
    let full = (*COLLAPSE_FULL_VIEWPORT.lock())?;
    let d3d = EngineContext::get()?;
    Some((m_eye, full, d3d))
}

/// Re-issue a render block's `Draw` once per eye, reprojecting the four `float4` entries the block bakes
/// on `rc`'s graphics context at (`cb_index`, `reg_offset`) by that eye's `M_eye` and binding the eye's
/// half-viewport. Returns `false`
/// when the intercept must not run (collapse inactive, or the dual-eye state not yet published), in which
/// case the caller draws normally once.
///
/// The block writes its view-projection into a constant buffer inside its own `Draw`, so rather than
/// replicate that bake, this arms [`set_vertex_program_constants_detour`] to reproject the block's own
/// upload for the duration of a wrapped original-`Draw` call. It covers every draw kind (plain,
/// CPU-instanced, GPU-indirect) uniformly, since it re-drives the block's whole `Draw`, and each of the
/// two re-issues renders into its eye's viewport half. The re-issue raises [`PER_EYE_REISSUE`] so the
/// draw and viewport detours leave the block's own calls alone rather than compounding the split, and
/// `gate` decides whether it also stands down for a patched bound vertex shader (see [`BoundVsGate`]).
///
/// # Safety
///
/// `rc` must be the live [`RenderContext`] the detoured `Draw` received, and `draw` must invoke the
/// block's original `Draw` trampoline.
pub unsafe fn reproject_baked_cb_per_eye(
    rc: *const RenderContext,
    cb_index: i32,
    reg_offset: u32,
    gate: BoundVsGate,
    mut draw: impl FnMut(),
) -> bool {
    // SAFETY: forwarded unchanged from this function's own contract.
    unsafe { reproject_baked_cb_per_eye_staged(rc, cb_index, reg_offset, gate, |_| draw()) }
}

/// [`reproject_baked_cb_per_eye`] for a block that *also* has per-eye state of its own to stage:
/// `render` receives the eye and does that staging before invoking the block's `Draw`, all while the
/// vertex reprojection is armed.
///
/// The two halves act on different uploads and must not be conflated. The armed reprojection rewrites
/// the block's own **vertex** constant stage at (`cb_index`, `reg_offset`) as it passes through
/// [`set_vertex_program_constants_detour`], which is what gives the geometry its parallax; anything
/// `render` stages is its own upload, on whatever stage and slot it chooses, and is left alone.
///
/// # Safety
///
/// `rc` must be the live [`RenderContext`] the detoured `Draw` received, and `render` must invoke the
/// block's original `Draw` trampoline.
pub unsafe fn reproject_baked_cb_per_eye_staged(
    rc: *const RenderContext,
    cb_index: i32,
    reg_offset: u32,
    gate: BoundVsGate,
    mut render: impl FnMut(usize),
) -> bool {
    let Some((m_eye, full, d3d)) = baked_cb_intercept_ready("baked-cb", gate) else {
        return false;
    };
    // SAFETY: `rc` is the live render context the detoured `Draw` received.
    let ctx = unsafe { render_context_graphics_context(rc) } as usize;
    per_eye_halves(full, d3d, &mut |eye| {
        arm_reproject(ReprojectUpload {
            ctx,
            cb_index,
            reg_offset,
            m_eye: m_eye[eye],
        });
        render(eye);
        if !disarm_reproject() {
            warn_reproject_never_fired(cb_index, reg_offset);
        }
    });
    bind_both_viewport_slots(d3d, full);
    true
}

/// Run `render` once per eye with that eye's half-viewport pinned on both slots, restoring the
/// collapse's full viewport afterwards. Returns `false` when the intercept must not run -- the same
/// gate every other per-eye re-issue takes ([`baked_cb_intercept_ready`]) -- in which case the caller
/// must do its work itself, exactly once.
pub fn draw_per_eye_half(site: &'static str, mut render: impl FnMut(usize)) -> bool {
    let Some((_, full, d3d)) = baked_cb_intercept_ready(site, BoundVsGate::Checked) else {
        return false;
    };
    per_eye_halves(full, d3d, &mut render);
    true
}

/// Run `render` once per eye with that eye's half of `full` pinned on both viewport slots, then put
/// `full` back for the draws that follow.
pub(super) fn per_eye_halves(
    full: D3D11_VIEWPORT,
    d3d: EngineContext,
    render: &mut impl FnMut(usize),
) {
    for eye in 0..2 {
        let _reissue = PerEyeReissue::enter(eye);
        bind_both_viewport_slots(d3d, eye_half_viewport(full, eye));
        render(eye);
    }
    bind_both_viewport_slots(d3d, full);
}

/// Warn, at most once per (`cb_index`, `reg_offset`), that a per-eye re-issue found nothing to
/// reproject -- see [`REPROJECT_FIRED`].
pub(super) fn warn_reproject_never_fired(cb_index: i32, reg_offset: u32) {
    let key = (cb_index as i64) << 32 | i64::from(reg_offset);
    if REPROJECT_NEVER_FIRED.lock().insert(key) {
        tracing::warn!(
            target: "single_pass",
            "baked-cb per-eye re-issue at cb{cb_index}[{reg_offset}..{}] staged nothing to \
             reproject: the block took a path that does not bake this constant, so both eyes get \
             the same view",
            reg_offset + 4,
        );
    }
}

pub(super) static REPROJECT_NEVER_FIRED: Mutex<BTreeSet<i64>> = Mutex::new(BTreeSet::new());

pub(super) type SetVertexProgramConstantsFn =
    unsafe extern "system" fn(*mut c_void, i32, u32, *const f32, u32);
pub(super) static SET_VERTEX_PROGRAM_CONSTANTS: DetourSlot<SetVertexProgramConstantsFn> =
    DetourSlot::new();

/// Detour on `Graphics::SetVertexProgramConstants`. While a baked-cb per-eye re-issue is armed (see
/// [`reproject_baked_cb_per_eye`]), reproject the four `float4` entries at the armed (`cb_index`,
/// `reg_offset`) by the armed `M_eye` before the engine stages them, so the block's own
/// view-projection upload becomes that eye's. Every other stage -- un-armed, a different slot, or a
/// range that does not contain the target entries -- passes through unchanged.
///
/// The transform is applied entry-wise (`M_eye · cb[k]`) because the vertex shaders that consume these
/// registers build clip with a multiply-add chain (`clip = Σ_i p_i · cb[k+i]`) rather than four `dp4`s:
/// each register is a *column* of the baked matrix, not a row. Confirmed against the bundle's Bark,
/// Foliage and Occluder vertex shaders; see `docs/mod/stereo/single-pass-render-blocks.md`. (`cb13`'s own
/// `M_eye` block is the opposite convention -- the rewriter's epilogue *is* a `dp4` chain -- so
/// [`write_meye`] stores rows there.)
pub(super) unsafe extern "system" fn set_vertex_program_constants_detour(
    ctx: *mut c_void,
    cb_index: i32,
    start_offset: u32,
    data: *const f32,
    count: u32,
) {
    let detour = SET_VERTEX_PROGRAM_CONSTANTS
        .get()
        .expect("set before enable");
    // The clustered light-assignment view matrix, when the per-eye froxel split is assigning this
    // eye's lights from this eye's position. Checked first because it is scoped to a single call
    // inside `DrawClustered` and cannot overlap a baked-cb re-issue, which only arms in the G-buffer
    // range.
    if let Some(rows) = clustered_lighting::substitute_assignment_view(
        ctx as usize,
        cb_index,
        start_offset,
        data,
        count,
    ) {
        // SAFETY: `rows` holds the 4 float4 rows the call stages and outlives it; `detour.call` is the
        // trampoline.
        unsafe { detour.call(ctx, cb_index, start_offset, rows.as_ptr(), count) };
        return;
    }
    if REPROJECT_ARMED.load(Ordering::Acquire)
        && !data.is_null()
        && let Some(up) = *REPROJECT_UPLOAD.lock()
        && ctx as usize == up.ctx
        && cb_index == up.cb_index
        && start_offset <= up.reg_offset
        && up.reg_offset + 4 <= start_offset + count
    {
        let n = count as usize * 4;
        let mut buf = vec![0.0f32; n];
        // SAFETY: the caller stages `count` float4 rows = `n` floats from `data`.
        unsafe { std::ptr::copy_nonoverlapping(data, buf.as_mut_ptr(), n) };
        let base = (up.reg_offset - start_offset) as usize * 4;
        for k in 0..4 {
            let column = glam::Vec4::from_slice(&buf[base + k * 4..base + k * 4 + 4]);
            buf[base + k * 4..base + k * 4 + 4]
                .copy_from_slice(&up.m_eye.mul_vec4(column).to_array());
        }
        REPROJECT_FIRED.store(true, Ordering::Relaxed);
        // SAFETY: `buf` holds `n` floats and outlives the call; `detour.call` is the trampoline.
        unsafe { detour.call(ctx, cb_index, start_offset, buf.as_ptr(), count) };
        return;
    }
    // SAFETY: forwards the original arguments to the trampoline.
    unsafe { detour.call(ctx, cb_index, start_offset, data, count) };
}
