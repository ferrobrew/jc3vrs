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
    scaleform::{Movie, Value},
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
const LAYER_CONTAINERS: [&[&str]; LAYER_COUNT] = [
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
/// [`HudConfig::suppress_overlays`](crate::hud::HudConfig::suppress_overlays) is on.
const OVERLAY_CLIPS: &[&str] = &[
    "MCI_safe_area_center.MCI_drowning_container",
    "MCI_safe_area_center.MCI_health_damage_manager",
    "MCI_safe_area_center.MCI_character_damage_indicators",
    "MCI_safe_area_center.MCI_vehicle_damage_indicators",
    "MCI_safe_area_center.MCI_inflict_damage",
];

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
    prefix: &str,
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

        // Snapshot each container's game-driven visibility: the game shows and hides these
        // (wingsuit HUD, state-driven groups) and only writes on state changes, so forcing them
        // visible would desync its bookkeeping. Each pass shows a container only if the game had
        // it visible *and* it belongs to the pass's layer; the exact snapshot is restored after.
        let visibility = snapshot_visibility(movie_root, prefix);

        for layer in LAYERS {
            set_layer_visibility(movie_root, prefix, layer, &visibility, suppress_overlays);
            movie_impl.Capture(true);
            rebind_views(manager, &views.views[layer as usize]);
            original(this, context);
        }

        // Restore the game's own visibility (and a clean capture of it) so menus, the movie mode,
        // and a mid-frame split disable all see the HUD exactly as the game left it; hand capture
        // ownership back to its previous owner.
        restore_visibility(movie_root, prefix, &visibility);
        movie_impl.Capture(true);
        manager.m_CurrentCaptureThread = previous_capture_thread;
        movie_impl.SetCaptureThread(previous_capture_thread);
        rebind_views(manager, &views.views[HudLayer::Static as usize]);

        LeaveCriticalSection(lock as *mut _);
    }
}

/// The game-driven visibility snapshot: one flag per container (in [`LAYERS`]/
/// [`LAYER_CONTAINERS`] order, flattened) and one per overlay clip.
struct VisibilitySnapshot {
    containers: [[bool; MAX_CONTAINERS]; LAYER_COUNT],
    overlays: [bool; MAX_OVERLAYS],
}

/// The largest per-layer container list.
const MAX_CONTAINERS: usize = 7;
/// The overlay clip count.
const MAX_OVERLAYS: usize = 5;

/// Read each container's and overlay clip's current `_visible` (defaulting to visible when the
/// path does not resolve, which is also logged once by the write path).
unsafe fn snapshot_visibility(movie_root: &Movie, prefix: &str) -> VisibilitySnapshot {
    // SAFETY: the caller holds the deferred-render lock and capture ownership.
    unsafe {
        let mut snapshot = VisibilitySnapshot {
            containers: [[true; MAX_CONTAINERS]; LAYER_COUNT],
            overlays: [true; MAX_OVERLAYS],
        };
        for layer in LAYERS {
            for (i, container) in LAYER_CONTAINERS[layer as usize].iter().enumerate() {
                snapshot.containers[layer as usize][i] = get_visible(movie_root, prefix, container);
            }
        }
        for (i, clip) in OVERLAY_CLIPS.iter().enumerate() {
            snapshot.overlays[i] = get_visible(movie_root, prefix, clip);
        }
        snapshot
    }
}

/// Set the display list to show exactly `layer`'s containers that the game itself had visible
/// (plus, optionally, keep the overlay clips hidden even in their own layer's pass).
unsafe fn set_layer_visibility(
    movie_root: &Movie,
    prefix: &str,
    layer: HudLayer,
    snapshot: &VisibilitySnapshot,
    suppress_overlays: bool,
) {
    // SAFETY: the caller holds the deferred-render lock and capture ownership.
    unsafe {
        for other in LAYERS {
            for (i, container) in LAYER_CONTAINERS[other as usize].iter().enumerate() {
                let visible = other == layer && snapshot.containers[other as usize][i];
                set_visible(movie_root, prefix, container, visible);
            }
        }
        if suppress_overlays {
            for clip in OVERLAY_CLIPS {
                set_visible(movie_root, prefix, clip, false);
            }
        }
    }
}

/// Restore every container and overlay clip to its snapshotted game-driven visibility.
unsafe fn restore_visibility(movie_root: &Movie, prefix: &str, snapshot: &VisibilitySnapshot) {
    // SAFETY: the caller holds the deferred-render lock and capture ownership.
    unsafe {
        for layer in LAYERS {
            for (i, container) in LAYER_CONTAINERS[layer as usize].iter().enumerate() {
                set_visible(
                    movie_root,
                    prefix,
                    container,
                    snapshot.containers[layer as usize][i],
                );
            }
        }
        for (i, clip) in OVERLAY_CLIPS.iter().enumerate() {
            set_visible(movie_root, prefix, clip, snapshot.overlays[i]);
        }
    }
}

/// Read `<prefix><path>._visible`, defaulting to `true` when the path does not resolve or the
/// value is not a boolean.
unsafe fn get_visible(movie_root: &Movie, prefix: &str, path: &str) -> bool {
    let full = format!("{prefix}{path}._visible\0");
    let mut value = Value::new_boolean(true);
    // SAFETY: NUL-terminated path bytes; an unmanaged stack value the movie fills in. A boolean
    // result is unmanaged, so nothing needs releasing.
    let ok = unsafe { movie_root.GetVariable(&mut value, full.as_ptr()) };
    if !ok || value.Type & 0x8F != Value::VT_BOOLEAN {
        return true;
    }
    value.mValue & 0xFF != 0
}

/// Write `<prefix><path>._visible = visible` on the movie. Failures are logged once per path (the
/// prefix may be wrong until the in-game display-tree dump pins the attachment point).
unsafe fn set_visible(movie_root: &Movie, prefix: &str, path: &str, visible: bool) {
    let full = format!("{prefix}{path}._visible\0");
    let value = Value::new_boolean(visible);
    // SAFETY: NUL-terminated path bytes; an unmanaged stack boolean the movie copies.
    let ok = unsafe { movie_root.SetVariable(full.as_ptr(), &value, 0) };
    if !ok {
        log_path_failure_once(&full[..full.len() - "._visible\0".len()]);
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

/// Log a failing clip path once rather than every frame (60+ writes per second per path).
fn log_path_failure_once(path: &str) {
    use std::sync::Mutex;
    static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let mut logged = LOGGED.lock().unwrap();
    if !logged.iter().any(|p| p == path) {
        tracing::warn!(
            "hud split: SetVariable failed for {path:?}; the clip path (or the configured split \
             path prefix) does not match the runtime display tree"
        );
        logged.push(path.to_string());
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
