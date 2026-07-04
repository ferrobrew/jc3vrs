//! The multi-pass HUD split: render the HUD in three visibility passes into separate textures, so
//! each group can be composited at its own depth (issue #14).
//!
//! A single HUD texture forces every element to one depth: a crosshair crossing a distant marker
//! gets the same stereo disparity as the marker, and the full-screen overlays cover the whole
//! panel. Scaleform composites overlapping clips into the texture at draw time, so no amount of
//! texture-space work can separate them afterwards -- the split has to happen at render time, by
//! rendering the movie several times with different subsets visible.
//!
//! [`render_split`] replaces one `CUIManager::Render` call with three. `Render` draws the movie's
//! latest *captured* display-tree snapshot, so visibility changes only take effect through a fresh
//! `Movie::Capture` -- the sequence per pass is: set the pass's clip visibility (AS3 writes on the
//! `MovieRoot`), capture (on the render thread, after borrowing capture-thread ownership the same
//! way the engine's own `RenderOffScreenTextures` does), rebind the UI render buffer's views to
//! the pass's texture, and call the original `Render`. The whole sequence holds
//! [`UIManager::m_DeferredRenderLock`] -- the lock `PreRender` holds across `Advance`+`Capture` --
//! so the update thread cannot mutate the display list mid-split; the lock is a Win32 critical
//! section and therefore re-entrant when the original `Render` takes it again.
//!
//! The partition works on the ten authored top-level containers of `hud.gfx` (see
//! `jc3gi`'s `ScaleformInfo` and `docs/issue-08-14-hud-overlays-and-depth.md`), so the visibility
//! writes are a handful of `SetVariable` calls per pass. The full-screen overlays of issue #8 are
//! named children of the center container and can be held invisible in every pass.

use jc3gi::ui::{
    scaleform::{DisplayInfo, Movie, Value},
    ui_manager::UIManager,
};
use windows::{
    Win32::{
        Graphics::Direct3D11::{ID3D11DepthStencilView, ID3D11RenderTargetView},
        System::Threading::{EnterCriticalSection, GetCurrentThreadId, LeaveCriticalSection},
    },
    core::Interface as _,
};

/// The HUD layers, in composite order (bottom to top). Each is one render pass and one texture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HudLayer {
    /// The static HUD: the corner/edge safe-area containers and the weapon-selection wheel. No
    /// world anchor; lives at the panel depth.
    Static = 0,
    /// World-anchored markers: the POI stage and the target tracker / score container.
    Markers = 1,
    /// The screen-center group: weapon/grapple/mech reticles, pickups, and center indicators.
    /// Composited on top, at the aim depth once that is driven (issue #14 follow-up).
    Center = 2,
}

/// The number of layers / passes / textures.
pub const LAYER_COUNT: usize = 3;

/// All layers in pass order (which is also composite order, bottom to top).
pub const LAYERS: [HudLayer; LAYER_COUNT] = [HudLayer::Static, HudLayer::Markers, HudLayer::Center];

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
/// invisible in every pass while
/// [`HudConfig::suppress_overlays`](crate::hud::HudConfig::suppress_overlays) is on. Paths
/// confirmed against an in-game display-tree dump; `MCI_omni_damage` (the screen-wide damage
/// flash) is a direct child of the center container, not of the health-damage manager.
pub(crate) const OVERLAY_CLIPS: &[&str] = &[
    "MCI_safe_area_center.MCI_omni_damage",
    "MCI_safe_area_center.MCI_drowning_container",
    "MCI_safe_area_center.MCI_health_damage_manager",
    "MCI_safe_area_center.MCI_character_damage_indicators",
    "MCI_safe_area_center.MCI_vehicle_damage_indicators",
    "MCI_safe_area_center.MCI_inflict_damage",
    "MCI_safe_area_center.MCI_sniper",
];

/// A heap-pinned managed [`Value`] handle to a clip. The `Value` is an intrusive list node on
/// the movie's external-references list, so it must never move after `GetVariable` fills it --
/// hence the box. Release (on the capture thread) before dropping.
pub(crate) struct ClipHandle {
    /// The pinned managed value. Present until released; absent when the clip path did not
    /// resolve at discovery (the handle then reads as visible and writes are no-ops).
    pub value: Option<Box<Value>>,
}

impl ClipHandle {
    /// Release the managed value through its object interface. Must run on the capture thread
    /// (the game thread, or a split pass that owns the capture).
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

/// The clip handles the split passes toggle, grouped by role. Built by the layout discovery on
/// the game thread; used by the split passes on the render worker (which owns the capture while
/// doing so); replaced wholesale on rediscovery (old handles released first).
pub(crate) struct ClipHandles {
    /// Per layer, the named containers (in [`LAYER_CONTAINERS`] order, missing clips skipped).
    pub containers: [Vec<ClipHandle>; LAYER_COUNT],
    /// The issue #8 overlay clips.
    pub overlays: Vec<ClipHandle>,
    /// The anonymous POI pool (markers layer).
    pub dynamic: Vec<ClipHandle>,
}

// SAFETY: the raw pointers inside the handles (the pinned Values and their object interfaces)
// are only dereferenced on the capture thread or by a split pass that owns the capture, never
// concurrently -- the registry mutex plus the capture-thread discipline provide the
// synchronization the pointer types cannot express.
unsafe impl Send for ClipHandle {}

/// The live handle registry. `None` until a discovery succeeds.
pub(crate) static CLIP_HANDLES: parking_lot::Mutex<Option<ClipHandles>> =
    parking_lot::Mutex::new(None);

/// The per-pass render-target views, snapshotted from the HUD state before the passes run (so the
/// state lock is not held across the original `Render` calls). The views are COM clones, so they
/// keep their textures alive even if the state recreates its targets concurrently.
pub struct LayerViews {
    /// `(RTV, DSV)` per layer, in [`LAYERS`] order.
    pub(super) views: [(ID3D11RenderTargetView, ID3D11DepthStencilView); LAYER_COUNT],
}

/// Render the HUD in [`LAYER_COUNT`] visibility passes via repeated calls to the original
/// `CUIManager::Render`. Falls back to a single original call when any precondition is missing
/// (movie not live, vtable mismatch, lock unavailable), so a failed split degrades to the normal
/// single-texture panel rather than a blank HUD.
///
/// # Safety
/// Must be called from the detour on `CUIManager::Render`, on the UI render worker, with `this`
/// and `context` being the detour's own arguments.
pub unsafe fn render_split(
    this: *mut UIManager,
    context: *mut std::ffi::c_void,
    views: &LayerViews,
    suppress_overlays: bool,
    original: &dyn Fn(*mut UIManager, *mut std::ffi::c_void),
) {
    // SAFETY (whole body): `this` is the live UI manager inside its own Render call path; the
    // movie pointers are checked before use; the lock order (m_DeferredRenderLock, re-entered by
    // the original) mirrors the engine's own PreRender/Render exclusion.
    unsafe {
        let Some(manager) = this.as_mut() else {
            return;
        };
        let (Some(movie_impl), lock) = (manager.m_Movie.as_mut(), manager.m_DeferredRenderLock)
        else {
            original(this, context);
            return;
        };
        let Some(movie_root) = movie_impl.pASMovieRoot.as_ref() else {
            original(this, context);
            return;
        };
        if lock.is_null() || movie_root.vftable() as usize as u64 != Movie::VFTABLE {
            log_split_fallback_once();
            original(this, context);
            return;
        }

        // The clip handles are built by the layout discovery on the game thread; without them
        // the split cannot mask, so degrade to the single-texture render and (re-)request
        // discovery. The registry lock is held for the whole split, which also blocks a
        // concurrent rediscovery from releasing handles mid-use.
        let mut handles = CLIP_HANDLES.lock();
        let Some(handles) = handles.as_mut() else {
            super::scaleform::request_layout_discovery();
            original(this, context);
            return;
        };

        EnterCriticalSection(lock as *mut _);

        // Borrow capture ownership for the visibility+capture sequence. This must mirror the
        // engine's own `CUIManager::SetCaptureThread`: write `m_CurrentCaptureThread` *and* the
        // movie's capture thread. `CUIBase::Invoke` runs the AVM immediately when the calling
        // thread matches the field, so leaving it on the game thread lets event-driven invokes
        // (damage flashes, UI activations) mutate the display list concurrently with our
        // captures -- which corrupts the renderer's tree cache (elements vanish, then a crash in
        // `PrimitiveBundle::InsertEntry`). With the field pointing at this thread, those invokes
        // queue into the UI command queue and drain on the next game-thread update.
        let render_thread = GetCurrentThreadId();
        let previous_capture_thread = manager.m_CurrentCaptureThread;
        manager.m_CurrentCaptureThread = render_thread;
        movie_impl.SetCaptureThread(render_thread);

        // Snapshot each clip's game-driven visibility: the game shows and hides these (wingsuit
        // HUD, POI pool entries) and only writes on state changes, so forcing them visible would
        // desync its bookkeeping. Each pass shows a clip only if the game had it visible *and*
        // it belongs to the pass's layer; the exact snapshot is restored after. All reads and
        // writes go through GetDisplayInfo/SetDisplayInfo on cached handles -- the same direct
        // display-info channel the game's own UpdatePOIs uses -- because AVM path resolution per
        // write is slow and AS3 `visible` property setters run clip show/hide logic.
        let snapshot = snapshot_visibility(handles);

        for layer in LAYERS {
            set_layer_visibility(handles, &snapshot, layer, suppress_overlays);
            movie_impl.Capture(true);
            rebind_views(manager, &views.views[layer as usize]);
            original(this, context);
        }

        // Restore the game's own visibility (and a clean capture of it) so menus, the movie mode,
        // and a mid-frame split disable all see the HUD exactly as the game left it; hand capture
        // ownership back to its previous owner.
        restore_visibility(handles, &snapshot);
        movie_impl.Capture(true);
        manager.m_CurrentCaptureThread = previous_capture_thread;
        movie_impl.SetCaptureThread(previous_capture_thread);
        rebind_views(manager, &views.views[HudLayer::Static as usize]);

        LeaveCriticalSection(lock as *mut _);
    }
}

/// The game-driven visibility snapshot, index-aligned with [`ClipHandles`]' vectors.
struct VisibilitySnapshot {
    containers: [Vec<bool>; LAYER_COUNT],
    overlays: Vec<bool>,
    dynamic: Vec<bool>,
}

/// Read each handle's current visibility (defaulting to visible when the read fails).
unsafe fn snapshot_visibility(handles: &mut ClipHandles) -> VisibilitySnapshot {
    // SAFETY: the caller holds the deferred-render lock and capture ownership.
    unsafe {
        VisibilitySnapshot {
            containers: std::array::from_fn(|layer| {
                handles.containers[layer]
                    .iter_mut()
                    .map(|h| get_visible(h))
                    .collect()
            }),
            overlays: handles
                .overlays
                .iter_mut()
                .map(|h| get_visible(h))
                .collect(),
            dynamic: handles.dynamic.iter_mut().map(|h| get_visible(h)).collect(),
        }
    }
}

/// Set the display list to show exactly `layer`'s clips that the game itself had visible (plus,
/// optionally, keep the overlay clips hidden even in their own layer's pass).
unsafe fn set_layer_visibility(
    handles: &mut ClipHandles,
    snapshot: &VisibilitySnapshot,
    layer: HudLayer,
    suppress_overlays: bool,
) {
    // SAFETY: the caller holds the deferred-render lock and capture ownership.
    unsafe {
        for other in LAYERS {
            let in_layer = other == layer;
            for (handle, was_visible) in handles.containers[other as usize]
                .iter_mut()
                .zip(&snapshot.containers[other as usize])
            {
                set_visible(handle, in_layer && *was_visible);
            }
        }
        // The anonymous POI pool belongs to the markers layer.
        let markers = layer == HudLayer::Markers;
        for (handle, was_visible) in handles.dynamic.iter_mut().zip(&snapshot.dynamic) {
            set_visible(handle, markers && *was_visible);
        }
        if suppress_overlays {
            for handle in &mut handles.overlays {
                set_visible(handle, false);
            }
        }
    }
}

/// Restore every clip to its snapshotted game-driven visibility.
unsafe fn restore_visibility(handles: &mut ClipHandles, snapshot: &VisibilitySnapshot) {
    // SAFETY: the caller holds the deferred-render lock and capture ownership.
    unsafe {
        for layer in LAYERS {
            for (handle, was_visible) in handles.containers[layer as usize]
                .iter_mut()
                .zip(&snapshot.containers[layer as usize])
            {
                set_visible(handle, *was_visible);
            }
        }
        for (handle, was_visible) in handles.overlays.iter_mut().zip(&snapshot.overlays) {
            set_visible(handle, *was_visible);
        }
        for (handle, was_visible) in handles.dynamic.iter_mut().zip(&snapshot.dynamic) {
            set_visible(handle, *was_visible);
        }
    }
}

/// Read a clip's visibility through its cached handle, defaulting to visible on failure.
unsafe fn get_visible(handle: &mut ClipHandle) -> bool {
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
unsafe fn set_visible(handle: &mut ClipHandle, visible: bool) {
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

/// Log the vtable-mismatch fallback once rather than every frame.
fn log_split_fallback_once() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        tracing::error!(
            "hud split: the AS3 root's vtable does not match the modeled MovieRoot vtable; \
             falling back to the single-texture render"
        );
    }
}

/// Rebind the UI render buffer's views to `(rtv, dsv)` for the next pass. The movie rectangle,
/// safe area, and viewport are untouched -- every layer texture shares the redirect's dimensions.
unsafe fn rebind_views(
    manager: &mut UIManager,
    (rtv, dsv): &(ID3D11RenderTargetView, ID3D11DepthStencilView),
) {
    // SAFETY: the render buffer was built by the redirect on this thread; UpdateData refcounts the
    // views it swaps.
    unsafe {
        if let Some(render_buffer) = manager.m_RenderBuffer.as_mut() {
            render_buffer.UpdateData(rtv.as_raw(), std::ptr::null_mut(), dsv.as_raw());
        }
    }
}
