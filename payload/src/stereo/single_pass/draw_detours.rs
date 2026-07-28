//! The draw-path COM detours: the vtable entry points every geometry submission passes through, and
//! the per-eye re-issue of the ones the collapse cannot handle by instance doubling alone.
//!
//! A patched vertex shader renders both eyes from one instanced draw, so the common case needs
//! nothing here. These exist for the draws that cannot take that route -- an unpatched shader, a draw
//! the engine already instanced for its own reasons, and the GPU-indirect draws whose instance count
//! lives in a buffer the CPU never sees -- each of which has to be submitted once per eye instead.

use super::*;

/// `ID3D11DeviceContext` vtable slots for the two indexed-draw entry points (verified against
/// `windows`'s `ID3D11DeviceContext_Vtbl`: field 6 → slot 12, field 14 → slot 20).
pub(super) const DRAW_INDEXED_SLOT: usize = 12;
pub(super) const DRAW_SLOT: usize = 13;
pub(super) const DRAW_INDEXED_INSTANCED_SLOT: usize = 20;

pub(super) type DrawIndexedFn = unsafe extern "system" fn(*mut c_void, u32, u32, i32);
pub(super) type DrawFn = unsafe extern "system" fn(*mut c_void, u32, u32);
pub(super) type DrawIndexedInstancedFn =
    unsafe extern "system" fn(*mut c_void, u32, u32, u32, i32, u32);

pub(super) static DRAW_INDEXED: DetourSlot<DrawIndexedFn> = DetourSlot::new();
pub(super) static DRAW: DetourSlot<DrawFn> = DetourSlot::new();
/// `DrawIndexedInstanced` (see [`draw_indexed_instanced_detour`]); its trampoline also serves as the
/// raw entry a promoted [`draw_indexed_detour`] draw is re-issued through.
///
/// An already-instanced draw whose patched vertex shader takes its per-instance data through a
/// vertex-buffer slot (rather than through `SV_InstanceID`, which would have deferred the shader with
/// `InstanceIdAlreadyDeclared`) has instance ids that are the game's own, so `SV_InstanceID & 1` sends
/// instance `i` to eye `i & 1` -- half the instances in each eye, and a 1-instance draw to the left eye
/// only. Promoting the instance count cannot fix it: the per-instance vertex-buffer stepping is indexed
/// by the instance id, so doubling the count reads past the instance data. Instead [`instanced_per_eye`]
/// makes the parity irrelevant, re-issuing the draw once per eye with both `cb13` eye slots and both
/// viewport slots pinned to that eye -- the bucket-(d) mechanism in
/// `docs/mod/single-pass-render-blocks.md`.
pub(super) static DRAW_INDEXED_INSTANCED: DetourSlot<DrawIndexedInstancedFn> = DetourSlot::new();

/// Handle a `DrawIndexed` while the dual-eye G-buffer geometry is drawing. A **patched** shader is
/// promoted to a 2-instance `DrawIndexedInstanced` -- its `SV_InstanceID & 1` selects the eye and
/// `SV_ViewportArrayIndex` routes it to that eye's viewport half (one draw, both eyes). An
/// **unpatched** shader writes no viewport index and so would rasterise to slot 0 only; under collapse
/// it is instead re-issued once per eye with both slots pinned to that eye's half, which costs a
/// second submission but is the only way it reaches both eyes. The patched/unpatched split is counted
/// for the diagnostic log.
pub(super) unsafe extern "system" fn draw_indexed_detour(
    context: *mut c_void,
    index_count: u32,
    start_index: u32,
    base_vertex: i32,
) {
    let detour = DRAW_INDEXED.get().expect("set before enable");
    if dual_eye_active() && in_gbuffer_range() && per_eye_reissue_eye().is_none() {
        let patched = BOUND_VS_PATCHED.load(Ordering::Relaxed);
        if patched {
            if collapse_active() {
                ensure_collapse_viewport(context, CollapseViewport::Split);
            }
            PATCHED_DRAWS.fetch_add(1, Ordering::Relaxed);
            if let Some(instanced) = DRAW_INDEXED_INSTANCED.get() {
                // Through the trampoline, not the detoured entry: the promotion is the mod's own
                // draw, so it must not be counted as exposure by [`draw_indexed_instanced_detour`].
                unsafe { instanced.call(context, index_count, 2, start_index, base_vertex, 0) };
                return;
            }
        } else {
            UNPATCHED_DRAWS.fetch_add(1, Ordering::Relaxed);
            if collapse_active() {
                // The shader was not rewritten, so it writes no `SV_ViewportArrayIndex` and always
                // rasterises to slot 0. Left to the split binding it would appear in the left eye
                // only; left to the full binding it would stretch across both. Re-issue it once per
                // eye with both slots pinned to that eye's half. It still reads the centred `cb0`, so
                // there is no parallax on it -- but it is present, in both eyes, at the right size.
                for eye in 0..2 {
                    ensure_collapse_viewport(context, CollapseViewport::Eye(eye));
                    unsafe { detour.call(context, index_count, start_index, base_vertex) };
                }
                ensure_collapse_viewport(context, CollapseViewport::Full);
                return;
            }
        }
    }
    unsafe { detour.call(context, index_count, start_index, base_vertex) };
}

/// Handle a non-indexed `DrawIndexed` sibling -- `Draw` (vtable slot 13). The fullscreen passes
/// (deferred lighting, screen-space effects, post) draw a fullscreen triangle this way, and under
/// collapse they must cover the **full** target, not the eye-half the previous patched geometry draw
/// left the viewport split to. Reset the viewport to full before the draw. Outside collapse (or the
/// camera scene) this is a straight pass-through.
pub(super) unsafe extern "system" fn draw_detour(
    context: *mut c_void,
    vertex_count: u32,
    start_vertex: u32,
) {
    let detour = DRAW.get().expect("set before enable");
    // SAFETY: forwards the caller's arguments unchanged to the trampoline.
    let submit = || unsafe { detour.call(context, vertex_count, start_vertex) };

    if collapse_active() && in_gbuffer_range() && per_eye_reissue_eye().is_none() {
        let pass = current_pass_id();
        SLOT13_BY_PASS[usize::from(pass)].fetch_add(1, Ordering::Relaxed);
        // A geometry pass on this entry point is not the fullscreen triangle the `Full` reset below
        // exists for, so give it the same per-eye treatment the indexed path gives its draws.
        if is_geometry_slot13_pass(pass)
            && config_flags().has(Flag::Slot13PerEye)
            && instanced_per_eye(&submit)
        {
            return;
        }
        ensure_collapse_viewport(context, CollapseViewport::Full);
    }
    submit();
}

/// Render passes whose non-indexed draws are **geometry**, not a viewport-covering triangle.
///
/// `Draw` (slot 13) is predominantly the fullscreen-pass entry point, which is why the default is to
/// reset the viewport to full for it. But a number of render blocks submit ordinary world geometry
/// non-indexed, and pinning `Full` for those rasterises them across the whole double-wide target,
/// stretched 2x horizontally about its centre. A 2x horizontal stretch is also a 2x horizontal
/// *motion* gain, which is what makes decals appear to slide across the world at twice the camera's
/// rate.
///
/// # How this list was derived
///
/// Statically, from the release binary rather than from a capture. The engine funnels every
/// non-indexed submission through one wrapper (`Graphics::Draw`), so its call sites are the complete
/// set of blocks that can reach slot 13 -- 60-odd of them. Each was classified by reading its `Draw`:
/// a block that binds a vertex stream and submits a world transform is geometry, one that submits a
/// screen-covering primitive is not. Their passes come from the engine's pass-creation sites
/// (`CRenderEngine::CreateRenderPass`, whose pass id is a literal at every call), which name the
/// owning system for each id, plus that system's enqueue site.
///
/// The two groups below are the result. Every fullscreen slot-13 block turned out to sit on a pass of
/// its own that no geometry block is enqueued onto -- including three that fall *inside* the GBuffer
/// index range and so are live under the collapse ([`RenderPassId::RP_DOWNSAMPLE_DEPTH`],
/// [`RenderPassId::RP_UNDERWATER_FOG_GRADIENT`], and the fog-volume trio) -- so no pass carries both
/// kinds and the pass id is a sound discriminator. It stays an allowlist, not a heuristic: the cost of
/// misclassifying a fullscreen pass as geometry is a visibly wrong frame, while the cost of missing a
/// geometry pass is only the status quo.
///
/// Deliberately absent: [`RenderPassId::RP_MODELS_REFLECTION`], which is model geometry but renders
/// the reflection camera's view into the reflection target, not into the double-wide scene target --
/// splitting it per eye would be wrong. The blocks whose passes lie outside the collapsed range
/// (UI, the reflection prepasses, `POST_RP_FULLSCREEN_VIDEO`) need no entry at all.
pub(super) const GEOMETRY_SLOT13_PASSES: &[u8] = &[
    // Passes whose entire draw list is a block that *always* submits non-indexed.
    // `CRoadMeshManager` owns both road passes; a road is enqueued onto the stencil one as well when
    // its visual type has shader type 0.
    RenderPassId::RP_ROAD_STENCIL as u8,
    RenderPassId::RP_ROAD_LAYERS as u8,
    // `CCreatureManager`'s pass: `CCreatureRenderBlock::Draw` has no indexed path at all.
    RenderPassId::RP_CREATURES as u8,
    // `CDecalManager`'s two passes, drawing the four `CRenderBlockDecal*` types.
    RenderPassId::RP_DECALS as u8,
    RenderPassId::RP_WINDOW_DECALS as u8,
    // `CSkidmarkManager`'s pass, drawing `CRenderBlockSkidmarks`.
    RenderPassId::RP_SKIDMARKS as u8,
    // The model-family passes. General / GeneralJC3 / GeneralMaskedJC3 / BuildingJC3 / Prop /
    // MaterialTune / Open / FXMeshFire / MeshParticle all end their `Draw` and `DrawZ` with the same
    // branch -- `DrawIndexed` when the block has an index buffer, plain `Draw` when it does not -- so
    // any index-buffer-less model in the shipped data lands on slot 13 in whichever of these it was
    // enqueued onto. The engine owns the two Z passes; `CModelInstanceManager` owns the rest, and
    // enqueues nothing but model instances onto them.
    RenderPassId::RP_Z_PASS as u8,
    RenderPassId::RP_Z_AND_VELOCITY_PASS as u8,
    RenderPassId::RP_MODELS_DYNAMIC as u8,
    RenderPassId::RP_MODELS_DYNAMIC_MASK_DAMAGE_POST_EFFECT as u8,
    RenderPassId::RP_MODELS_STATIC as u8,
    RenderPassId::RP_MODELS_TRANSPARENT as u8,
    RenderPassId::RP_MODELS_GLINT as u8,
    RenderPassId::RP_MODEL_HALO_POST as u8,
    RenderPassId::RP_MODELS_REFRACT as u8,
    RenderPassId::RP_Z_FINAL_TRANSPARENT as u8,
    RenderPassId::RP_GHOST_EFFECT as u8,
    RenderPassId::RP_OUTLINE_MASK as u8,
    RenderPassId::RP_FINAL_TRANSPARENT as u8,
];

pub(super) fn is_geometry_slot13_pass(pass: u8) -> bool {
    GEOMETRY_SLOT13_PASSES.contains(&pass)
}

// ---- GPU-indirect draws --------------------------------------------------------------------------

/// `ID3D11DeviceContext` vtable slots for the two GPU-indirect draw entry points (field 33 → slot 39,
/// field 34 → slot 40, verified against `windows`'s `ID3D11DeviceContext_Vtbl`). The engine reaches
/// them through `Graphics::DrawInstanced` (slot 39) and `Graphics::DrawIndexedInstancedIndirectNoMutex`
/// (slot 40); the terrain-patch block's near tessellating passes and the foliage block's dominant path
/// are the volume users.
pub(super) const DRAW_INDEXED_INSTANCED_INDIRECT_SLOT: usize = 39;
pub(super) const DRAW_INSTANCED_INDIRECT_SLOT: usize = 40;

/// Both indirect entry points take `(ID3D11Buffer* pBufferForArgs, UINT AlignedByteOffsetForArgs)`.
pub(super) type DrawIndirectFn = unsafe extern "system" fn(*mut c_void, *mut c_void, u32);

pub(super) static DRAW_INDEXED_INSTANCED_INDIRECT: DetourSlot<DrawIndirectFn> = DetourSlot::new();
pub(super) static DRAW_INSTANCED_INDIRECT: DetourSlot<DrawIndirectFn> = DetourSlot::new();

pub(super) unsafe extern "system" fn draw_indexed_instanced_indirect_detour(
    context: *mut c_void,
    buffer_for_args: *mut c_void,
    aligned_byte_offset: u32,
) {
    let detour = DRAW_INDEXED_INSTANCED_INDIRECT
        .get()
        .expect("set before enable");
    // SAFETY: forwards the caller's arguments unchanged to the trampoline.
    let submit = || unsafe { detour.call(context, buffer_for_args, aligned_byte_offset) };
    if !indirect_per_eye(context, &submit) {
        submit();
    }
}

pub(super) unsafe extern "system" fn draw_instanced_indirect_detour(
    context: *mut c_void,
    buffer_for_args: *mut c_void,
    aligned_byte_offset: u32,
) {
    let detour = DRAW_INSTANCED_INDIRECT.get().expect("set before enable");
    // SAFETY: forwards the caller's arguments unchanged to the trampoline.
    let submit = || unsafe { detour.call(context, buffer_for_args, aligned_byte_offset) };
    if !indirect_per_eye(context, &submit) {
        submit();
    }
}

/// Re-issue a GPU-indirect draw once per eye with both viewport slots pinned to that eye's half,
/// reporting whether it did; `false` means the caller must submit the draw itself, once, unchanged.
///
/// An indirect draw carries its vertex/instance counts in a GPU buffer, so it can be neither
/// instance-doubled (the promotion [`draw_indexed_detour`] applies to a patched shader) nor rewritten
/// -- the arguments are submitted verbatim, twice. What the mod controls is the viewport, and that is
/// the whole defect: nothing detoured these entry points, so the draw inherited whatever the previous
/// draw left bound, which under the collapse is dominated by [`CollapseViewport::Full`]. Rasterising
/// `cb0`'s per-eye off-axis projection into the full double-wide viewport stretches the geometry 2x
/// horizontally about the target centre, and a 2x horizontal stretch is a 2x horizontal *motion* gain
/// -- the near terrain patches and foliage sliding across the screen as the camera turns. Pinning each
/// submission to its eye's half is the same deal the unpatched `DrawIndexed` path already takes:
/// present and correctly sized in both eyes, with no parallax, since `cb0` stays eye 0's.
///
/// Declines outside the collapse and the G-buffer range (elsewhere there is no eye split to inherit),
/// inside a block-level per-eye re-issue (which has already pinned the viewport for this eye
/// deliberately -- the [`PER_EYE_REISSUE`] marker), and when the bound vertex shader is patched (it
/// writes `SV_ViewportArrayIndex` itself and would have to be routed, not pinned; that includes the
/// tessellated terrain families once `single_pass_terrain` transforms them). `cb13` needs no pinning
/// here for the same reason: no pristine game shader declares it, so an unpatched draw cannot read it.
///
/// The re-issue does not raise [`PER_EYE_REISSUE`] itself -- it drives no engine code, only the
/// trampolines, so nothing it calls can re-enter this module's detours.
pub(super) fn indirect_per_eye(context: *mut c_void, submit: &dyn Fn()) -> bool {
    if !collapse_active() || !in_gbuffer_range() || per_eye_reissue_eye().is_some() {
        return false;
    }
    if !config_flags().has(Flag::IndirectPerEye) || BOUND_VS_PATCHED.load(Ordering::Relaxed) {
        INDIRECT_FORWARDED.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    // Without a recorded full viewport `ensure_collapse_viewport` is a no-op, so the loop would submit
    // the draw twice into one region -- doubled geometry rather than a split.
    if COLLAPSE_FULL_VIEWPORT.lock().is_none() {
        INDIRECT_FORWARDED.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    for eye in 0..2 {
        ensure_collapse_viewport(context, CollapseViewport::Eye(eye));
        submit();
    }
    // Unconditional, as on every other path that pins a viewport: a leaked eye-half would clip the
    // rest of the frame -- including the fullscreen passes -- to half the target.
    ensure_collapse_viewport(context, CollapseViewport::Full);
    INDIRECT_REISSUED.fetch_add(1, Ordering::Relaxed);
    true
}

/// GPU-indirect draws [`indirect_per_eye`] re-issued, and those it saw in a collapsed G-buffer range
/// but forwarded once (the flag off, or a patched shader bound). Reset per pass range by
/// [`log_draw_split`], which reports them.
pub(super) static INDIRECT_REISSUED: AtomicUsize = AtomicUsize::new(0);
pub(super) static INDIRECT_FORWARDED: AtomicUsize = AtomicUsize::new(0);

// ---- Already-instanced draws ---------------------------------------------------------------------

/// Detour on `DrawIndexedInstanced`, handling the already-instanced case (see
/// [`DRAW_INDEXED_INSTANCED`]) with a per-eye re-issue and measuring how much of the frame it covers.
///
/// A call is in that case when a patched vertex shader is bound (so the shader reads `SV_InstanceID` as
/// an eye parity it did not ask for), the render thread is inside the G-buffer range, and the collapse
/// is on -- minus the mod's own draws. Promoted `DrawIndexed`es come through the trampoline and never
/// reach here; draws a per-eye re-issue re-drives are excluded by [`per_eye_reissue_eye`], since those
/// already land in one eye deliberately. Everything else is forwarded verbatim, once.
pub(super) unsafe extern "system" fn draw_indexed_instanced_detour(
    context: *mut c_void,
    index_count_per_instance: u32,
    instance_count: u32,
    start_index: u32,
    base_vertex: i32,
    start_instance: u32,
) {
    let detour = DRAW_INDEXED_INSTANCED.get().expect("set before enable");
    let submit = || {
        // SAFETY: forwards the caller's arguments unchanged to the trampoline.
        unsafe {
            detour.call(
                context,
                index_count_per_instance,
                instance_count,
                start_index,
                base_vertex,
                start_instance,
            );
        }
    };
    if active() && per_eye_reissue_eye().is_none() {
        INSTANCED_TOTAL.fetch_add(1, Ordering::Relaxed);
        let patched = BOUND_VS_PATCHED.load(Ordering::Relaxed);
        let in_range = in_gbuffer_range();
        if patched {
            let bucket = if in_range {
                &INSTANCED_RANGE_PATCHED
            } else {
                &INSTANCED_RANGE_OUT_PATCHED
            };
            bucket.fetch_add(1, Ordering::Relaxed);
        }
        if patched && in_range && collapse_active() {
            let handled = config_flags().has(Flag::InstancedPerEye) && instanced_per_eye(&submit);
            record_instanced_case(instance_count, handled);
            if handled {
                return;
            }
        } else {
            record_instanced_bystander(patched, in_range, instance_count);
            if patched && !in_range && collapse_active() {
                // The shader writes `SV_ViewportArrayIndex` from instance parity whatever pass it is
                // in, but this one binds no eye-half pair -- so the odd instances need slot 1 to hold
                // the same region as slot 0 to appear at all.
                unify_viewport_slots();
            }
        }
    }
    submit();
}

/// Re-issue an already-instanced draw once per eye, so the game's own instance ids stop being read as
/// an eye parity. Returns `false` when the re-issue cannot run (no full viewport recorded yet, no live
/// context, or no `cb13` contents to pin and restore), in which case the caller submits once and the
/// draw is counted as exposure.
///
/// For each eye, **both** `cb13` eye slots are filled with that eye's transforms and **both** viewport
/// slots with that eye's half of the double-wide target, so whichever parity `SV_InstanceID & 1`
/// produces -- and therefore whichever `SV_ViewportArrayIndex` the shader writes -- resolves to the
/// same eye. The instance count is left alone: per-instance vertex-buffer stepping is indexed by the
/// instance id, so doubling it would read past the batch's per-instance data.
///
/// `cb13` is shared by every patched shader in the pass, so its restore is unconditional: leaving it
/// pinned would collapse the rest of the frame's geometry to one eye. The viewport is put back exactly
/// as it was found rather than to the recorded full width, so a following draw that inherits the
/// viewport (an indirect draw, which nothing detours) sees no trace of the pin.
pub(super) fn instanced_per_eye(submit: &dyn Fn()) -> bool {
    let Some(full) = *COLLAPSE_FULL_VIEWPORT.lock() else {
        return false;
    };
    let Some(d3d) = EngineContext::get() else {
        return false;
    };
    let Some(rows) = CB13.lock().live_rows() else {
        return false;
    };
    let saved = capture_viewport_slots(d3d);

    let mut submitted = 0;
    for eye in 0..2 {
        // The same marker the baked-cb re-issues raise: it keeps this module's own detours -- and any
        // block-level re-issue reached from here -- from compounding the split this loop sets up.
        let _reissue = PerEyeReissue::enter(eye);
        if !write_cb13_rows(d3d, &pin_rows_to_eye(&rows, eye)) {
            break;
        }
        bind_both_viewport_slots(d3d, eye_half_viewport(full, eye));
        submit();
        submitted += 1;
    }

    if !write_cb13_rows(d3d, &rows) {
        // The buffer is still mapped, so there is no way to retry within this call. The warning is the
        // diagnostic signal: cb13 stays pinned to the last eye's rows for the rest of the pass.
        tracing::warn!(
            "cb13 restore failed after per-eye pin; geometry may render from one eye for the rest of the pass"
        );
    }
    restore_viewport_slots(d3d, saved);
    // A map failure before the first submission leaves the draw undrawn, so the caller must still
    // submit it; one after leaves it partially drawn, where a further submission would only duplicate
    // geometry in an eye that already has it.
    submitted > 0
}
