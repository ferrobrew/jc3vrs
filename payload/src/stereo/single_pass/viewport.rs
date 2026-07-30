//! Viewport routing: how a draw ends up in one eye's half of the double-wide target.
//!
//! A rewritten vertex shader picks its half by writing `SV_ViewportArrayIndex` from instance parity,
//! which only works if slot 0 and slot 1 hold the two halves. Keeping them that way is most of this
//! module: the engine rebinds its viewport constantly -- per shadow cascade, per post pass, at every
//! render-setup bind -- and each rebind resets slot 1, so the split has to be re-established rather
//! than set once, and from a detour low enough to see every set.
//!
//! The rest is the inverse problem. Outside the routed range both slots must hold the *same* region,
//! or a patched shader surviving into a shadow or reflection pass sends its odd instances into a half
//! that pass never bound.

use windows::Win32::Foundation::RECT;

use super::*;

/// Bind `viewport` to both viewport slots of the immediate context. Binding two slots (rather than
/// one) passes the collapse viewport detour through untouched -- it only special-cases a single-slot
/// set -- and the terrain-detail VS has no `SV_ViewportArrayIndex`, so it rasterizes into slot 0.
pub(super) fn bind_both_viewport_slots(d3d: EngineContext, viewport: D3D11_VIEWPORT) {
    // SAFETY: a two-element slice is a valid viewport array.
    d3d.with_lock(|ctx| unsafe { ctx.RSSetViewports(Some(&[viewport, viewport])) });
}

/// The immediate context's currently-bound viewport slots, captured so a per-eye re-issue can put back
/// exactly what the surrounding pass had rather than assume it was the collapse's full viewport. Only
/// the two slots single-pass uses are captured.
#[derive(Clone, Copy)]
pub(super) struct ViewportSlots {
    slots: [D3D11_VIEWPORT; 2],
    count: u32,
}

pub(super) fn capture_viewport_slots(d3d: EngineContext) -> ViewportSlots {
    let mut count = 2u32;
    let mut slots = [D3D11_VIEWPORT::default(); 2];
    // SAFETY: `count` is the length of `slots`, as `RSGetViewports` requires.
    d3d.with_lock(|ctx| unsafe { ctx.RSGetViewports(&mut count, Some(slots.as_mut_ptr())) });
    // Trailing zero-width slots are what a runtime writes for the elements it had nothing bound for,
    // and whether it also writes the count back is implementation-defined -- so take the width, not the
    // count, as the authority on how many slots there really are. Restoring a zero-width slot 1 would
    // clip every later right-eye primitive to nothing.
    let count = slots
        .iter()
        .take(count.min(2) as usize)
        .take_while(|v| v.Width > 0.0)
        .count() as u32;
    ViewportSlots { slots, count }
}

/// Re-bind the slots [`capture_viewport_slots`] recorded. Goes through the trampoline rather than the
/// vtable entry: a restored single-slot set would otherwise be re-recorded by
/// [`rs_set_viewports_detour`] as the collapse's full viewport.
pub(super) fn restore_viewport_slots(d3d: EngineContext, saved: ViewportSlots) {
    if saved.count == 0 {
        return;
    }
    let Some(detour) = RS_SET_VIEWPORTS.get() else {
        return;
    };
    // A single restored slot leaves slot 1 unbound, which is exactly the state a patched shader cannot
    // survive; flag it so the next out-of-range patched draw repairs it.
    set_viewport_slots_uniform(saved.count == 2 && saved.slots[0] == saved.slots[1]);
    // SAFETY: the context is the live immediate context and `slots` holds `count` viewports.
    d3d.with_lock(|ctx| unsafe {
        detour.call(ctx.as_raw(), saved.count, saved.slots.as_ptr());
    });
}

/// Whether `viewport` covers the scene render target, as opposed to one of the reduced-resolution
/// post-effect targets. Compared against [`crate::stereo::render_size`] -- the engine's
/// `m_BackBufferLinear`, which under the collapse's double-wide is the full two-eye width -- with a
/// pixel of slack for the engine's own rounding.
pub(super) fn is_scene_sized(viewport: D3D11_VIEWPORT) -> bool {
    let Some((width, height)) = crate::stereo::render_size() else {
        // Without a render size to compare against, take the viewport: the alternative is never
        // recording one and losing the eye split entirely.
        return true;
    };
    (viewport.Width - width as f32).abs() <= 1.0 && (viewport.Height - height as f32).abs() <= 1.0
}

/// The eye-half of `full` for eye `e` (left = 0, right = 1).
pub(super) fn eye_half_viewport(full: D3D11_VIEWPORT, eye: usize) -> D3D11_VIEWPORT {
    let half = full.Width / 2.0;
    D3D11_VIEWPORT {
        TopLeftX: full.TopLeftX + eye as f32 * half,
        Width: half,
        ..full
    }
}

/// If single-pass is active, duplicate the immediate context's current viewport (and scissor) into
/// slot 1. Called right after the engine binds a render setup ([`SetRenderSetup`]), which is where
/// the viewport is (re)set -- including per-cascade in the shadow passes, so slot 1 tracks whatever
/// region is currently bound rather than going stale between binds.
pub fn duplicate_current_viewport() {
    if !active() {
        return;
    }
    // SAFETY: runs on the render thread after a render-setup bind; the device/context pointers are
    // stable and the ops run under the engine's context mutex.
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
        duplicate_viewport(&context.m_Context);
        LeaveCriticalSection(context.m_Mutex);
    }
}

/// Re-apply the eye-half split to the currently-bound viewport at the start of the G-buffer range.
///
/// The main G-buffer render setup is bound (setting its viewport) *before* `DrawRenderPassRange`
/// raises [`in_gbuffer_range`], so the [`rs_set_viewports_detour`] identical-dups it instead of
/// splitting -- and that dup'd viewport covers the bulk of the geometry, so both instances of a
/// patched draw land in the same half. Called right after the range flag goes up (dual-eye only),
/// this reads that bound viewport and re-sets it as left/right halves.
pub fn apply_eye_split_viewport() {
    // Collapse routes per draw (`ensure_collapse_viewport`), so the pass-level pre-split is off there.
    if collapse_active() || !(dual_eye_active() && in_gbuffer_range()) {
        return;
    }
    // SAFETY: runs on the render thread at the G-buffer range boundary; the device/context pointers
    // are stable and the ops run under the engine's context mutex.
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
        let ctx = &context.m_Context;
        EnterCriticalSection(context.m_Mutex);
        let mut count = 1u32;
        let mut viewports = [D3D11_VIEWPORT::default(); 1];
        ctx.RSGetViewports(&mut count, Some(viewports.as_mut_ptr()));
        let vp = viewports[0];
        if vp.Width > 0.0 {
            let half = vp.Width / 2.0;
            let mut left = vp;
            left.Width = half;
            let mut right = vp;
            right.Width = half;
            right.TopLeftX = vp.TopLeftX + half;
            // count == 2 passes straight through the detour to the raw RSSetViewports.
            ctx.RSSetViewports(Some(&[left, right]));
        }
        LeaveCriticalSection(context.m_Mutex);
    }
}

/// Whether both viewport slots are known to be bound to the **same** region, so a patched shader's
/// `SV_ViewportArrayIndex = SV_InstanceID & 1` rasterises identically whichever parity it computes.
///
/// A patched vertex shader writes the viewport index unconditionally -- the bytecode has no idea which
/// pass it is in -- but the eye-half pair is only ever bound for the G-buffer geometry. Everywhere else
/// (the shadow cascades, the reflection prepass, the post and UI passes) slot 1 must be a duplicate of
/// slot 0, or the odd-parity instances of an already-instanced draw rasterise into a region that pass
/// never meant to write. [`rs_set_viewports_detour`] keeps that true for every viewport the engine
/// binds, but the collapse's own per-draw split ([`ensure_collapse_viewport`]) leaves the two slots
/// holding *different* halves, and that state outlives the G-buffer range until the next engine bind.
/// This flag tracks which of the two the slots are in, so [`unify_viewport_slots`] can repair the
/// split state without reading the device on every draw.
///
/// Conservative: anything that leaves the slots in an unknown state clears it, costing at most one
/// redundant repair.
pub(super) static VIEWPORT_SLOTS_UNIFORM: AtomicBool = AtomicBool::new(false);

/// Record what the two viewport slots now hold. Called by every path that binds them.
pub(super) fn set_viewport_slots_uniform(uniform: bool) {
    VIEWPORT_SLOTS_UNIFORM.store(uniform, Ordering::Relaxed);
}

/// Re-bind viewport slot 0's region to **both** slots, if they are not already known to hold the same
/// one. Restores the invariant [`VIEWPORT_SLOTS_UNIFORM`] describes outside the G-buffer range.
///
/// Slot 0 is left exactly as it was found, so this cannot change where an even-parity primitive lands
/// -- the only difference it makes is that the odd-parity ones stop being routed somewhere else. The
/// device read is behind the flag, so the common (already uniform) case costs a relaxed load.
pub(super) fn unify_viewport_slots() {
    if VIEWPORT_SLOTS_UNIFORM.load(Ordering::Relaxed)
        || !config_flags().has(Flag::UniformViewportSlots)
    {
        return;
    }
    let (Some(d3d), Some(detour)) = (EngineContext::get(), RS_SET_VIEWPORTS.get()) else {
        return;
    };
    d3d.with_lock(|ctx| {
        // SAFETY: `count` is the length of `slots`; `detour.call` is the original RSSetViewports, so
        // the re-bind does not re-enter the detour and re-record a full viewport.
        unsafe {
            let mut count = 1u32;
            let mut slots = [D3D11_VIEWPORT::default(); 1];
            ctx.RSGetViewports(&mut count, Some(slots.as_mut_ptr()));
            // A zero-width slot 0 means no viewport is bound at all; duplicating it would clip
            // everything to nothing.
            if slots[0].Width > 0.0 {
                detour.call(ctx.as_raw(), 2, [slots[0], slots[0]].as_ptr());
                set_viewport_slots_uniform(true);
                VIEWPORT_UNIFIED.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

/// Duplicate the current (single) viewport into viewport slots 0 **and** 1, both covering the same
/// region.
///
/// A patched shader writes `SV_ViewportArrayIndex = SV_InstanceID & 1`. With divergence off nothing
/// doubles instances or sets up per-eye viewports, so an instanced draw's odd-`SV_InstanceID`
/// primitives would route to viewport 1 -- which the engine never bound -- and be discarded, dropping
/// half of every instanced object (the flicker, since VR head-motion re-sorts which instance ids are
/// odd). Binding a second, identical viewport makes index 1 valid and render the same as index 0.
/// When the eyes diverge, the two identical viewports become the left/right halves of the double-wide
/// target instead.
pub(super) unsafe fn duplicate_viewport(context: &ID3D11DeviceContext) {
    unsafe {
        let mut count = 1u32;
        let mut viewports = [D3D11_VIEWPORT::default(); 1];
        context.RSGetViewports(&mut count, Some(viewports.as_mut_ptr()));
        // Only duplicate a real viewport; a zero-width one (no viewport bound yet) would clip
        // everything to nothing.
        if viewports[0].Width > 0.0 {
            context.RSSetViewports(Some(&[viewports[0], viewports[0]]));
        }

        // If scissor testing is on, viewport 1 pairs with scissor rect 1; duplicate the engine's
        // rect into slot 1 too, else index-1 primitives clip to an empty (unset) rect.
        let mut scissor_count = 1u32;
        let mut scissors = [RECT::default(); 1];
        context.RSGetScissorRects(&mut scissor_count, Some(scissors.as_mut_ptr()));
        if scissors[0].right > scissors[0].left && scissors[0].bottom > scissors[0].top {
            context.RSSetScissorRects(Some(&[scissors[0], scissors[0]]));
        }
    }
}

// The mirror at `SetRenderSetup` (above) covers the scene passes, but the shadow cascades set their
// viewport through a raw `RSSetViewports` between binds, which that hook does not see -- so slot 1
// goes stale there and odd-instance shadow casters route to the wrong region (flickering shadows).
// Detouring `RSSetViewports`/`RSSetScissorRects` on the immediate-context vtable catches *every*
// viewport set, wherever it comes from, and mirrors a single-viewport set into two identical slots.

/// `ID3D11DeviceContext` vtable slots (7 base `IUnknown`/`ID3D11DeviceChild` slots + the method's
/// index), verified against `windows`'s `ID3D11DeviceContext_Vtbl`.
pub(super) const RS_SET_VIEWPORTS_SLOT: usize = 44;
pub(super) const RS_SET_SCISSOR_RECTS_SLOT: usize = 45;

pub(super) type RsSetViewportsFn =
    unsafe extern "system" fn(*mut c_void, u32, *const D3D11_VIEWPORT);
pub(super) type RsSetScissorRectsFn = unsafe extern "system" fn(*mut c_void, u32, *const RECT);

pub(super) static RS_SET_VIEWPORTS: DetourSlot<RsSetViewportsFn> = DetourSlot::new();
pub(super) static RS_SET_SCISSOR_RECTS: DetourSlot<RsSetScissorRectsFn> = DetourSlot::new();

pub(super) unsafe extern "system" fn rs_set_viewports_detour(
    context: *mut c_void,
    count: u32,
    viewports: *const D3D11_VIEWPORT,
) {
    let detour = RS_SET_VIEWPORTS.get().expect("set before enable");
    if active() && count == 1 && !viewports.is_null() {
        let vp = unsafe { *viewports };
        if let Some(eye) = per_eye_reissue_eye() {
            // Inside a per-eye re-issue the eye half must survive whatever the block binds. Honour the
            // requested region but keep it pinned to this eye's half, and leave `COLLAPSE_FULL_VIEWPORT`
            // alone so the re-issue cannot redefine what "full" means for the draws that follow it.
            let half = eye_half_viewport(vp, eye);
            set_viewport_slots_uniform(true);
            unsafe { detour.call(context, 2, [half, half].as_ptr()) };
            return;
        }
        if collapse_active() {
            // Collapse: record the full viewport and bind both slots to it unsplit. The eye-split is
            // applied per-draw in `draw_indexed_detour` via `ensure_collapse_viewport`, so the
            // interleaved fullscreen lighting/post passes (which do not route to an eye) keep the full
            // width while patched geometry gets the L/R halves. Binding both slots keeps a patched
            // shader that writes `SV_ViewportArrayIndex` valid before the first split of a pass.
            //
            // Only a scene-sized viewport is recorded. The detour sees every viewport bind in the
            // frame, including the half-resolution SSAO/SSR/bloom targets, and the eye halves are
            // derived from this record -- so without the size check the eye split would follow
            // whichever post pass happened to bind last.
            if is_scene_sized(vp) {
                *COLLAPSE_FULL_VIEWPORT.lock() = Some(vp);
            }
            // Unconditional, unlike the scene record above: this one has to follow the engine onto the
            // reduced-resolution off-screen targets, which is the whole point of it.
            *CURRENT_ENGINE_VIEWPORT.lock() = Some(vp);
            VIEWPORT_DUP.fetch_add(1, Ordering::Relaxed);
            set_viewport_slots_uniform(true);
            unsafe { detour.call(context, 2, [vp, vp].as_ptr()) };
            return;
        }
        let (slot0, slot1) = if dual_eye_active() && in_gbuffer_range() {
            // Route the two eyes to the left/right halves of the (double-wide) target.
            VIEWPORT_SPLIT.fetch_add(1, Ordering::Relaxed);
            let half = vp.Width / 2.0;
            let mut left = vp;
            left.Width = half;
            let mut right = vp;
            right.Width = half;
            right.TopLeftX = vp.TopLeftX + half;
            (left, right)
        } else {
            // Not diverging: both slots identical, so a patched shader routes anywhere validly.
            VIEWPORT_DUP.fetch_add(1, Ordering::Relaxed);
            (vp, vp)
        };
        set_viewport_slots_uniform(slot0 == slot1);
        unsafe { detour.call(context, 2, [slot0, slot1].as_ptr()) };
    } else {
        // A multi-slot set passes straight through, so the slots become whatever the caller asked for:
        // uniform only for the mod's own two-identical-slot binds. Anything else (including a set that
        // leaves slot 1 unbound) is taken as non-uniform, so the next patched draw outside the range
        // repairs it.
        let uniform =
            count == 2 && !viewports.is_null() && unsafe { *viewports == *viewports.add(1) };
        set_viewport_slots_uniform(uniform);
        unsafe { detour.call(context, count, viewports) };
    }
}

/// What a collapse draw wants bound to the two viewport slots.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CollapseViewport {
    /// Both slots span the whole double-wide target: the fullscreen lighting and post passes.
    Full,
    /// Slot 0 is the left eye's half and slot 1 the right: a patched shader's
    /// `SV_ViewportArrayIndex` picks its own.
    Split,
    /// Both slots are the same eye's half, so a shader that writes no viewport index -- or writes
    /// either one -- still lands in that eye. Used by the per-eye re-issue of unpatched geometry.
    Eye(usize),
}

/// In the collapsed single walk, bind the immediate-context viewport for the draw about to be
/// submitted. Derives the halves from the full viewport recorded by [`rs_set_viewports_detour`]; a
/// no-op until the scene's first viewport bind records it.
pub(super) fn ensure_collapse_viewport(context: *mut c_void, target: CollapseViewport) {
    // Split the viewport of whatever target is actually bound, not the scene's. They are the same
    // everywhere except the reduced-resolution off-screen passes -- see [`CURRENT_ENGINE_VIEWPORT`].
    // From the dispatch snapshot, not live: this function both pins an eye half and puts the full
    // viewport back, and the two calls have to agree on which "full" they mean.
    let base = if config_flags().has(Flag::ViewportFollowsTarget) {
        (*CURRENT_ENGINE_VIEWPORT.lock()).or(*COLLAPSE_FULL_VIEWPORT.lock())
    } else {
        *COLLAPSE_FULL_VIEWPORT.lock()
    };
    let Some(full) = base else {
        return;
    };
    let Some(detour) = RS_SET_VIEWPORTS.get() else {
        return;
    };
    let viewports = match target {
        CollapseViewport::Split => {
            VIEWPORT_SPLIT.fetch_add(1, Ordering::Relaxed);
            [eye_half_viewport(full, 0), eye_half_viewport(full, 1)]
        }
        CollapseViewport::Eye(eye) => {
            VIEWPORT_SPLIT.fetch_add(1, Ordering::Relaxed);
            let half = eye_half_viewport(full, eye);
            [half, half]
        }
        CollapseViewport::Full => [full, full],
    };
    set_viewport_slots_uniform(viewports[0] == viewports[1]);
    // SAFETY: `context` is the live immediate context; `detour.call` invokes the original
    // RSSetViewports (the trampoline), so this does not re-enter the detour. Bound unconditionally
    // (no split-state skip): the engine can change the viewport underneath us via a path we do not
    // observe (a `count != 1` set), so a cached "already split" flag would go stale and let both
    // instances land in one half -- the doubled/"same geometry twice" artifact. Re-binding per draw
    // is cheap (a few hundred geometry draws per frame, far below the draw budget we are cutting).
    unsafe { detour.call(context, 2, viewports.as_ptr()) };
}
