//! The time-multiplexed HUD split: refresh one depth layer's texture per frame, so each group of
//! elements can be composited at its own depth (issue #14).
//!
//! A single HUD texture forces every element to one depth: a crosshair crossing a distant marker
//! gets the same stereo disparity as the marker, and the full-screen overlays cover the whole
//! panel. Scaleform composites overlapping clips into the texture at draw time, so the split has
//! to happen at render time, with different subsets visible.
//!
//! Rendering the movie several times per frame does not work: the snapshot pipeline's `Capture`
//! mutates the *active* snapshot -- the same data every game-thread UI write touches, with no
//! writer lock -- so it is only safe on the game update thread, where it is serialized with all
//! other UI code by being the same thread (which is exactly how the engine uses it, in
//! `PreRender`). Capturing from the render worker races every invoke and display-info write:
//! torn change lists, flicker, and updates surfacing frames late.
//!
//! So the split multiplexes in time instead: each game frame, [`apply_capture_mask`] (running in
//! the [`MovieImpl::Capture`] detour -- game thread, after `UpdatePOIs` and `Advance`, deferred
//! render lock held) sets the visibility mask for *one* layer, round-robin, and the engine's own
//! once-a-frame capture carries it. The UI render detour ([`bind_layer_and_clear`]) redirects the
//! render buffer to that layer's persistent texture and lets the single original render fill it.
//! Each texture refreshes at `1/LAYER_COUNT` rate; the draw side compensates by freezing the
//! marker layer's world pose between refreshes (world-anchored icons stay glued to their world
//! spots), while the head-locked static and center layers only see a few frames of content
//! latency.
//!
//! Within one engine frame the UI renders once per eye. Only the first render after a frame tick
//! consumes a pending capture; later renders in the same frame set the context's once-a-frame
//! latch ([`RenderContext::NextCaptureCalledInFrame`]) so they redraw the same snapshot -- both
//! eyes always see identical layer content, even when the game thread slips the next frame's
//! capture in between them.

use jc3gi::ui::{
    scaleform::{DisplayInfo, Value},
    ui_manager::UIManager,
};
use windows::{
    Win32::{
        Graphics::Direct3D11::{
            D3D11_CLEAR_DEPTH, D3D11_CLEAR_STENCIL, ID3D11DepthStencilView, ID3D11DeviceContext,
            ID3D11RenderTargetView,
        },
        System::Threading::{EnterCriticalSection, LeaveCriticalSection},
    },
    core::Interface as _,
};

/// The HUD layers, in composite order (bottom to top). Each is one texture, refreshed round-robin
/// one layer per frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HudLayer {
    /// The static HUD: the corner/edge safe-area containers and the weapon-selection wheel. No
    /// world anchor; lives at the panel depth.
    Static = 0,
    /// World-anchored markers: the POI stage and the target tracker / score container.
    Markers = 1,
    /// The screen-center group: weapon/grapple/mech reticles, pickups, and center indicators.
    /// Composited on top, at the aim depth when that is driven.
    Center = 2,
}

/// The number of layers / textures.
pub const LAYER_COUNT: usize = 3;

/// The authored top-level containers of `hud.gfx` assigned to each layer. Paths are relative to
/// the HUD movie's timeline; [`HudConfig::split_path_prefix`](crate::hud::HudConfig) is prepended
/// at call time once the runtime attachment point is known.
pub(crate) const LAYER_CONTAINERS: [&[&str]; LAYER_COUNT] = [
    // Static: the six corner/edge safe-area groups and the selection wheel.
    &[
        "MCI_safe_area_top_left",
        "MCI_safe_area_top_middle",
        "MCI_safe_area_top_right",
        "MCI_safe_area_bottom_left",
        "MCI_safe_area_bottom_middle",
        "MCI_safe_area_bottom_right",
        "MCI_weapon_selection_wheel",
    ],
    // Markers: the POI stage, and MCI_hud (the target-tracker stage plus score/ghost data).
    &["MCI_poi_stage", "MCI_hud"],
    // Center: the whole screen-center group (reticles and center indicators).
    &["MCI_safe_area_center"],
];

/// The full-screen overlay clips of issue #8, all children of the center container: held
/// invisible while [`HudConfig::suppress_overlays`](crate::hud::HudConfig::suppress_overlays) is
/// on. Paths confirmed against an in-game display-tree dump; `MCI_omni_damage` (the screen-wide
/// damage flash) is a direct child of the center container, not of the health-damage manager.
pub(crate) const OVERLAY_CLIPS: &[&str] = &[
    "MCI_safe_area_center.MCI_omni_damage",
    "MCI_safe_area_center.MCI_drowning_container",
    "MCI_safe_area_center.MCI_health_damage_manager",
    "MCI_safe_area_center.MCI_character_damage_indicators",
    "MCI_safe_area_center.MCI_vehicle_damage_indicators",
    "MCI_safe_area_center.MCI_inflict_damage",
    "MCI_safe_area_center.MCI_sniper",
];

/// A heap-pinned managed [`Value`] handle to a clip, plus the game-intent tracking the mask needs.
/// The `Value` is an intrusive list node on the movie's external-references list, so it must never
/// move after `GetVariable` fills it -- hence the box. Release (on the capture thread) before
/// dropping.
pub(crate) struct ClipHandle {
    /// The pinned managed value. Present until released; absent when the clip path did not
    /// resolve at discovery (the handle then reads as visible and writes are no-ops).
    pub value: Option<Box<Value>>,
    /// The game's own visibility intent for this clip, tracked across our forced writes: refreshed
    /// from a read-back whenever the current value differs from what we last wrote (meaning the
    /// game wrote in between).
    pub game_visible: bool,
    /// The visibility we last forced, or `None` while unforced.
    pub forced: Option<bool>,
}

impl ClipHandle {
    /// Wrap a resolved (or unresolved) pinned value.
    pub fn new(value: Option<Box<Value>>) -> Self {
        Self {
            value,
            game_visible: true,
            forced: None,
        }
    }

    /// Release the managed value through its object interface. Must run on the capture thread
    /// (the game update thread).
    pub unsafe fn release(&mut self) {
        if let Some(mut value) = self.value.take() {
            // SAFETY: the value was filled in place by GetVariable and never moved; managed
            // values carry their object interface.
            unsafe {
                if let Some(interface) = value.pObjectInterface.as_mut() {
                    let data = value.mValue as *mut std::ffi::c_void;
                    interface.ObjectRelease(value.as_mut(), data);
                }
            }
        }
    }
}

/// The clip handles the mask toggles, grouped by role. Built by the layout discovery on the game
/// thread; masked on the game thread; replaced wholesale on rediscovery (old handles released
/// first).
pub(crate) struct ClipHandles {
    /// Per layer, the named containers (in [`LAYER_CONTAINERS`] order, missing clips skipped).
    pub containers: [Vec<ClipHandle>; LAYER_COUNT],
    /// The issue #8 overlay clips.
    pub overlays: Vec<ClipHandle>,
    /// The anonymous POI pool (markers layer).
    pub dynamic: Vec<ClipHandle>,
}

// SAFETY: the raw pointers inside the handles (the pinned Values and their object interfaces)
// are only dereferenced on the capture (game update) thread, never concurrently -- the registry
// mutex plus that thread discipline provide the synchronization the pointer types cannot express.
unsafe impl Send for ClipHandle {}

/// The live handle registry. `None` until a discovery succeeds.
pub(crate) static CLIP_HANDLES: parking_lot::Mutex<Option<ClipHandles>> =
    parking_lot::Mutex::new(None);

/// The per-layer render-target views, snapshotted from the HUD state before the render detour
/// uses them (so the state lock is not held across the original render). The views are COM
/// clones, so they keep their textures alive even if the state recreates its targets concurrently.
pub struct LayerViews {
    /// `(RTV, DSV)` per layer, in [`LAYERS`] order. Layer 0 (static) is the main HUD target.
    pub(super) views: [(ID3D11RenderTargetView, ID3D11DepthStencilView); LAYER_COUNT],
}

/// The masked-capture handshake between the game thread and the UI render worker: `(seq << 8) |
/// (layer + 1)`, written by [`apply_capture_mask`] whenever a capture carries a layer mask, and
/// zero while the mask is inactive. The layer is offset by one so the initial state reads as
/// "no masked capture".
static MASK_STATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The last masked-capture sequence number the render side consumed.
static CONSUMED_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The engine frame id of the last split render (from [`bump_render_frame`]); only the first
/// render of a frame may consume a pending capture, so later renders (the other eye) redraw the
/// same snapshot and the eyes never diverge.
static LAST_RENDER_FRAME: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(u64::MAX);

/// The layer the render side last bound (what the displaying snapshot is masked for).
static BOUND_LAYER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The current engine frame id, bumped once per frame by the render-thread tick.
static RENDER_FRAME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Per-layer refresh generations, bumped when a layer's texture is (re)rendered with fresh
/// content. The draw side freezes the marker layer's pose when its generation changes.
static LAYER_GENERATIONS: [std::sync::atomic::AtomicU64; LAYER_COUNT] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

/// Mark the start of a new engine frame (render thread, once per frame). The next UI render may
/// consume a pending capture again.
pub fn bump_render_frame() {
    RENDER_FRAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// The per-layer refresh generations (see [`LAYER_GENERATIONS`]).
pub fn layer_generations() -> [u64; LAYER_COUNT] {
    std::array::from_fn(|i| LAYER_GENERATIONS[i].load(std::sync::atomic::Ordering::Relaxed))
}

/// Whether the displaying snapshot currently carries a layer mask (a masked capture has been
/// consumed and the mask has not been deactivated since). Used by the draw side to decide between
/// the split composite and the single panel.
pub fn mask_live() -> bool {
    MASK_STATE.load(std::sync::atomic::Ordering::Relaxed) != 0
}

/// Apply the frame's visibility state ahead of the engine's own capture. Runs in the
/// [`MovieImpl::Capture`] detour on the game update thread, with the deferred render lock held
/// and every display-tree writer quiescent -- the only safe point for these writes.
///
/// While the split is active, advances the round-robin schedule (only once the previous mask was
/// consumed, so a game thread running ahead of the renderer re-captures the same mask instead of
/// skipping layers) and masks each clip to `in_layer && game_intent`. While it is not, restores
/// every clip to the game's intent once. The issue #8 overlays are forced hidden independently
/// while overlay suppression is on.
pub fn apply_capture_mask(split_active: bool, suppress_overlays: bool) {
    use std::sync::atomic::Ordering;

    let mut handles = CLIP_HANDLES.lock();
    let Some(handles) = handles.as_mut() else {
        return;
    };

    // SAFETY (all get/set_visible calls below): game update thread, deferred render lock held;
    // the handles are pinned managed values from the registry.
    unsafe {
        if split_active {
            let state = MASK_STATE.load(Ordering::Relaxed);
            let (seq, layer) = if state == 0 {
                (1, 0)
            } else if state >> 8 == CONSUMED_SEQ.load(Ordering::Relaxed) {
                // The previous mask was consumed; move to the next layer.
                ((state >> 8) + 1, ((state & 0xFF) as usize) % LAYER_COUNT)
            } else {
                // Not consumed yet (the game thread is running ahead of the renderer); re-apply
                // the same mask so the merged pending capture stays consistent and no layer's
                // refresh is skipped.
                (
                    state >> 8,
                    ((state & 0xFF) as usize + LAYER_COUNT - 1) % LAYER_COUNT,
                )
            };
            for (index, containers) in handles.containers.iter_mut().enumerate() {
                let in_layer = index == layer;
                for handle in containers.iter_mut() {
                    let intent = refresh_intent(handle);
                    force_visible(handle, in_layer && intent);
                }
            }
            let markers = layer == HudLayer::Markers as usize;
            for handle in handles.dynamic.iter_mut() {
                let intent = refresh_intent(handle);
                force_visible(handle, markers && intent);
            }
            MASK_STATE.store((seq << 8) | (layer as u64 + 1), Ordering::Relaxed);
        } else if MASK_STATE.swap(0, Ordering::Relaxed) != 0 {
            for handle in handles
                .containers
                .iter_mut()
                .flatten()
                .chain(handles.dynamic.iter_mut())
            {
                unforce_visible(handle);
            }
        }

        if suppress_overlays {
            for handle in &mut handles.overlays {
                refresh_intent(handle);
                force_visible(handle, false);
            }
        } else {
            for handle in &mut handles.overlays {
                if handle.forced.is_some() {
                    refresh_intent(handle);
                    unforce_visible(handle);
                }
            }
        }
    }
}

/// The UI render detour's split step: pick the layer texture matching the snapshot this render
/// will draw, clear it, bind it, run the original render, and restore the main binding. Falls
/// back to the plain original call when any precondition is missing.
///
/// Consumption discipline: the first render of an engine frame consumes the pending capture (if
/// any) and binds the layer that capture was masked for; later renders in the same frame set the
/// context's once-a-frame latch so the original redraws the same displaying snapshot, and rebind
/// the same layer. The deferred render lock is held across the decision and the render so the
/// game thread cannot slip a capture in between.
///
/// # Safety
/// Must be called from the detour on `CUIManager::Render`, on the UI render worker, with `this`
/// and `context` being the detour's own arguments.
pub unsafe fn bind_layer_and_clear(
    this: *mut UIManager,
    context: *mut std::ffi::c_void,
    views: &LayerViews,
    original: &dyn Fn(*mut UIManager, *mut std::ffi::c_void),
) {
    use std::sync::atomic::Ordering;

    // SAFETY (whole body): `this` is the live UI manager inside its own Render call path; the
    // movie and HAL pointers are checked before use; the deferred render lock is the engine's own
    // PreRender/Render exclusion and is re-entrant for the original's own acquisition.
    unsafe {
        let Some(manager) = this.as_mut() else {
            return;
        };
        let lock = manager.m_DeferredRenderLock;
        let device_context = manager
            .m_RenderHAL
            .as_ref()
            .map(|hal| hal.pDeviceContext)
            .filter(|p| !p.is_null());
        let (Some(movie_impl), Some(device_context), false) =
            (manager.m_Movie.as_mut(), device_context, lock.is_null())
        else {
            original(this, context);
            return;
        };

        EnterCriticalSection(lock as *mut _);

        let state = MASK_STATE.load(Ordering::Relaxed);
        let frame = RENDER_FRAME.load(Ordering::Relaxed);
        let first_of_frame = LAST_RENDER_FRAME.swap(frame, Ordering::Relaxed) != frame;
        let layer =
            if first_of_frame && state != 0 && state >> 8 != CONSUMED_SEQ.load(Ordering::Relaxed) {
                // First render of the frame with a fresh masked capture pending: consume it.
                let layer = ((state & 0xFF) as usize - 1).min(LAYER_COUNT - 1);
                CONSUMED_SEQ.store(state >> 8, Ordering::Relaxed);
                BOUND_LAYER.store(layer, Ordering::Relaxed);
                layer
            } else {
                // Same frame (a later eye) or no fresh capture: pin the render to the current
                // displaying snapshot and redraw the layer it was masked for. Only set the latch
                // when the original will actually render -- its early-out never reaches the
                // `HAL::EndFrame` that clears the latch, and a stuck latch stops all snapshot
                // consumption for good.
                if manager.m_RenderReady && manager.m_RenderActive && manager.m_RenderingEnabled {
                    movie_impl.RenderContext.NextCaptureCalledInFrame = true;
                }
                BOUND_LAYER.load(Ordering::Relaxed)
            };

        // Clear the layer's texture on the HAL's own device context, so the clear is ordered with
        // the draws the original is about to record, then point the UI render buffer at it.
        let (rtv, dsv) = &views.views[layer];
        let device_context =
            std::mem::ManuallyDrop::new(ID3D11DeviceContext::from_raw(device_context));
        device_context.ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 0.0]);
        device_context.ClearDepthStencilView(
            dsv,
            (D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL).0,
            1.0,
            0,
        );
        rebind_views(manager, &views.views[layer]);

        original(this, context);
        LAYER_GENERATIONS[layer].fetch_add(1, Ordering::Relaxed);

        // Leave the render buffer on the main target (the static layer's texture), the binding
        // every non-split consumer of the redirect expects.
        rebind_views(manager, &views.views[HudLayer::Static as usize]);

        LeaveCriticalSection(lock as *mut _);
    }
}

/// Re-read a clip's game intent: when the current value differs from what we last wrote (or we
/// never wrote), the game changed it in between, so the read is its intent. Returns the intent.
unsafe fn refresh_intent(handle: &mut ClipHandle) -> bool {
    // SAFETY: forwarded to get_visible; see the caller's obligations.
    let read = unsafe { get_visible(handle) };
    if handle.forced != Some(read) {
        handle.game_visible = read;
    }
    handle.game_visible
}

/// Force a clip's visibility, remembering the write for the intent tracking.
unsafe fn force_visible(handle: &mut ClipHandle, visible: bool) {
    // SAFETY: forwarded to set_visible; see the caller's obligations.
    unsafe { set_visible(handle, visible) };
    handle.forced = Some(visible);
}

/// Restore a clip to the game's intent and stop tracking it as forced.
pub(crate) unsafe fn unforce_visible(handle: &mut ClipHandle) {
    if handle.forced.take().is_some() {
        let intent = handle.game_visible;
        // SAFETY: forwarded to set_visible; see the caller's obligations.
        unsafe { set_visible(handle, intent) };
    }
}

/// Read a clip's visibility through its cached handle, defaulting to visible on failure.
///
/// # Safety
/// The caller must be on the capture (game update) thread (see [`ClipHandle`]).
pub(crate) unsafe fn get_visible(handle: &mut ClipHandle) -> bool {
    // SAFETY: the handle's value is pinned and managed; the interface pointer comes from it.
    unsafe {
        let Some(value) = handle.value.as_mut() else {
            return true;
        };
        let Some(interface) = value.pObjectInterface.as_mut() else {
            return true;
        };
        let mut info = DisplayInfo::default();
        let data = value.mValue as *mut std::ffi::c_void;
        if !interface.GetDisplayInfo(data, &mut info) {
            return true;
        }
        info.Visible
    }
}

/// Write a clip's visibility through its cached handle (a `VarsSet = V_VISIBLE` display-info
/// write: no AVM, no AS3 setters).
///
/// # Safety
/// The caller must be on the capture (game update) thread (see [`ClipHandle`]).
pub(crate) unsafe fn set_visible(handle: &mut ClipHandle, visible: bool) {
    // SAFETY: as `get_visible`.
    unsafe {
        let Some(value) = handle.value.as_mut() else {
            return;
        };
        let Some(interface) = value.pObjectInterface.as_mut() else {
            return;
        };
        let mut info = DisplayInfo::default();
        info.VarsSet = DisplayInfo::V_VISIBLE as u16;
        info.Visible = visible;
        let data = value.mValue as *mut std::ffi::c_void;
        interface.SetDisplayInfo(data, &info);
    }
}

/// Rebind the UI render buffer's views to `(rtv, dsv)`. The movie rectangle, safe area, and
/// viewport are untouched -- every layer texture shares the redirect's dimensions.
unsafe fn rebind_views(
    manager: &mut UIManager,
    (rtv, dsv): &(ID3D11RenderTargetView, ID3D11DepthStencilView),
) {
    // SAFETY: the render buffer was built by the redirect; UpdateData refcounts the views it
    // swaps.
    unsafe {
        if let Some(render_buffer) = manager.m_RenderBuffer.as_mut() {
            render_buffer.UpdateData(rtv.as_raw(), std::ptr::null_mut(), dsv.as_raw());
        }
    }
}
