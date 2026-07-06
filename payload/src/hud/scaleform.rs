//! Scaleform display-tree debugging: dump the live clip tree and toggle clip visibility by path.
//!
//! The multi-texture HUD split (issue #14) needs the runtime clip paths of the element groups
//! (`MCI_poi_stage`, `MCI_safe_area_center.MCI_reticles`, ...) and a working per-clip visibility
//! toggle. This module provides both as debug operations: the UI thread queues requests, and
//! [`process_requests`] executes them on the game update thread (the Scaleform capture thread,
//! where the display tree is stable and the engine makes its own AS3 calls).
//!
//! The tree dump walks [`Movie::GetDisplayObjectsTree`] and logs one line per clip; the toggle
//! writes `<path>.visible` through [`Movie::SetVariable`]. Both are read-modify operations on the
//! live movie, guarded by a vtable check so a mismatched `m_Movie` model logs instead of calling
//! through a wrong vtable.

use std::ffi::CString;

use jc3gi::ui::{
    scaleform::{AmpMovieObjectDesc, DisplayInfo, MOVIE_VFTABLE, Movie, MovieImpl, Value},
    ui_manager::UIManager,
};
use parking_lot::Mutex;

/// A queued Scaleform debug operation. Queued from the UI thread, executed on the game thread.
enum Request {
    /// Log the movie's full display tree, one line per clip.
    DumpTree,
    /// Set `<path>.visible` on the clip at the dot-separated `path` (from the root timeline).
    SetClipVisible { path: String, visible: bool },
}

/// The pending requests. UI thread pushes, game thread drains.
static REQUESTS: Mutex<Vec<Request>> = Mutex::new(Vec::new());

/// Whether a layout discovery is wanted (set by the split when its paths do not resolve, or by
/// the debug UI). Throttled to one walk per second by [`process_requests`].
static DISCOVERY_REQUESTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// The last discovery attempt, for the once-per-second throttle.
static LAST_DISCOVERY: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// Request a display-tree layout discovery: derive the split's clip-path prefix from the live
/// tree (by locating `MCI_safe_area_center`'s parent), collect the HUD clip's anonymous children
/// (the POI pool), and build the split's clip-handle registry.
pub fn request_layout_discovery() {
    DISCOVERY_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Whether a handle release is wanted (set at shutdown so the managed values are released on the
/// capture thread before the hooks come down).
static RELEASE_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the current handle registry was resolved during gameplay ([`HudMode::Hud`]
/// (crate::hud::HudMode)). The game re-attaches the HUD clips around the frontend/gameplay
/// transition, so handles resolved in a menu can point at a detached tree -- the render-root
/// partition must never build from those (moving nodes the game is about to re-insert into the
/// live tree double-parents them, which hangs the renderer's cache reconciliation).
static DISCOVERED_IN_HUD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the live handle registry was resolved during gameplay (see [`DISCOVERED_IN_HUD`]).
pub fn handles_hud_fresh() -> bool {
    DISCOVERED_IN_HUD.load(std::sync::atomic::Ordering::Relaxed) && CLIP_HANDLES.lock().is_some()
}

/// The number of HUD layers the registry groups containers into (see
/// [`HudLayer`](super::split::HudLayer)).
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

/// Hold the issue #8 overlay clips hidden while `suppress` is on, tracking the game's own
/// visibility intent across our writes and restoring it on the off-transition. Runs at the
/// capture seam (game update thread, deferred render lock held).
///
/// # Safety
/// Must be called from the [`MovieImpl::Capture`](jc3gi::ui::scaleform::MovieImpl) detour.
pub unsafe fn apply_overlay_suppression(suppress: bool) {
    let mut handles = CLIP_HANDLES.lock();
    let Some(handles) = handles.as_mut() else {
        return;
    };
    // SAFETY: capture-seam threading per the function contract; the handles are pinned managed
    // values from the registry.
    unsafe {
        for handle in &mut handles.overlays {
            if suppress {
                refresh_intent(handle);
                force_visible(handle, false);
            } else if handle.forced.is_some() {
                refresh_intent(handle);
                unforce_visible(handle);
            }
        }
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

/// Request the clip-handle registry's release (from the shutdown path; executed on the game
/// thread by [`process_requests`]).
pub fn request_release_handles() {
    RELEASE_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Release and drop the clip-handle registry (capture thread). Every clip is first restored to
/// the game's tracked visibility intent: the periodic rediscovery replaces the registry while
/// the split mask holds most clips hidden, and without the restore the fresh handles would read
/// that mask state back as the game's intent -- baking the clips permanently invisible, a few
/// layers' worth per rediscovery until the whole HUD is dark.
fn release_clip_handles() {
    // The render-root partition resolved its nodes through these handles; restore the render
    // tree before they go away (rebuilt from the fresh registry on the next capture).
    crate::hud::split::roots::teardown_now();
    if let Some(mut handles) = CLIP_HANDLES.lock().take() {
        // SAFETY: called on the capture thread; each handle releases through its own interface.
        unsafe {
            for handle in handles
                .containers
                .iter_mut()
                .flatten()
                .chain(handles.overlays.iter_mut())
                .chain(handles.dynamic.iter_mut())
            {
                unforce_visible(handle);
                handle.release();
            }
        }
    }
}

/// Queue a display-tree dump (from the debug UI).
pub fn request_dump_tree() {
    REQUESTS.lock().push(Request::DumpTree);
}

/// Queue a clip visibility change (from the debug UI). `path` is dot-separated from the root
/// timeline, e.g. `MCI_hud.MCI_poi_stage`.
pub fn request_set_clip_visible(path: String, visible: bool) {
    REQUESTS
        .lock()
        .push(Request::SetClipVisible { path, visible });
}

/// Execute all pending requests. Call once per frame on the game update thread (the Scaleform
/// capture thread), before the frame's UI advance.
pub fn process_requests() {
    if RELEASE_REQUESTED.swap(false, std::sync::atomic::Ordering::Relaxed) {
        release_clip_handles();
    }

    // The capture-seam hooks (the render-root partition and the overlay suppression) need the
    // handle registry; request the initial discovery when either wants it. While the partition
    // is live the registry must stay stable (a rebuild would tear the partition down for a
    // frame), so the periodic refresh only runs before the partition takes.
    let handles_needed =
        crate::config::Config::lock_query(|c| c.hud.split || c.hud.suppress_overlays);
    let handles_live = CLIP_HANDLES.lock().is_some();
    if handles_needed
        && (!handles_live
            || (!crate::hud::split::roots::live()
                && LAST_DISCOVERY
                    .lock()
                    .is_some_and(|t| t.elapsed().as_secs_f32() >= 5.0)))
    {
        // Menu-era handles resolve against a tree the game re-attaches on entering gameplay, so
        // the partition refuses to build from them (and its build-time root-chain walk catches a
        // stale registry regardless); the periodic refresh here re-resolves until a discovery
        // lands during gameplay.
        DISCOVERY_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    let requests = std::mem::take(&mut *REQUESTS.lock());
    if requests.is_empty() && !DISCOVERY_REQUESTED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let Some((movie_impl, movie_root)) = live_movie() else {
        if !requests.is_empty() {
            tracing::warn!(
                "scaleform: the UI movie is not available; dropping the queued requests"
            );
        }
        return;
    };
    for request in requests {
        match request {
            Request::DumpTree => dump_tree(movie_impl, movie_root),
            Request::SetClipVisible { path, visible } => {
                set_clip_visible(movie_root, &path, visible)
            }
        }
    }

    if DISCOVERY_REQUESTED.load(std::sync::atomic::Ordering::Relaxed) {
        let due = {
            let mut last = LAST_DISCOVERY.lock();
            let due = last.is_none_or(|t| t.elapsed().as_secs_f32() >= 1.0);
            if due {
                *last = Some(std::time::Instant::now());
            }
            due
        };
        if due && discover_layout(movie_impl, movie_root) {
            DISCOVERY_REQUESTED.store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// The live UI movie pair `(MovieImpl, MovieRoot)`, if the UI manager exists, `m_Movie` and its
/// `pASMovieRoot` are set, and the root's vtable is the `MovieRoot` vtable the bindings model. A
/// vtable mismatch means the model is wrong for this binary, so every operation refuses rather
/// than calling through it.
fn live_movie() -> Option<(&'static mut MovieImpl, &'static Movie)> {
    // SAFETY: the UI manager is a live singleton past startup; m_Movie and its AS3 root are set
    // once at UI init and stable afterwards.
    unsafe {
        let movie_impl = UIManager::get()?.m_Movie.as_mut()?;
        let movie_root = movie_impl.pASMovieRoot.as_ref()?;
        let vftable = movie_root.vftable() as usize as u64;
        if vftable != MOVIE_VFTABLE {
            tracing::error!(
                "scaleform: the AS3 root's vtable is {vftable:#x}, expected the MovieRoot vtable \
                 {:#x}; refusing to operate on it",
                MOVIE_VFTABLE
            );
            return None;
        }
        Some((movie_impl, movie_root))
    }
}

/// Log the movie's display tree, one line per clip, as `path`-style names with child counts.
fn dump_tree(movie_impl: &MovieImpl, movie_root: &Movie) {
    // SAFETY: called on the capture thread; the movie's heap is live for the whole frame. The
    // returned tree is a fresh allocation we release when done.
    unsafe {
        if movie_impl.pHeap.is_null() {
            tracing::warn!("scaleform: the movie's heap is null; cannot dump the tree");
            return;
        }
        let root = movie_root.GetDisplayObjectsTree(movie_impl.pHeap);
        let Some(root_ref) = root.as_ref() else {
            tracing::warn!("scaleform: GetDisplayObjectsTree returned no tree");
            return;
        };
        tracing::info!("scaleform: display tree dump begins");
        let mut lines = 0usize;
        dump_node(root_ref, &mut String::new(), &mut lines);
        tracing::info!("scaleform: display tree dump ends ({lines} clips)");
        root_ref.Release();

        // Probe the split's known clip paths: whether each resolves and its current visibility,
        // with the configured prefix applied. This is the ground truth for the split partition
        // and the overlay suppression (a suppressed clip that still shows means the effect lives
        // at a different path).
        let prefix = crate::config::Config::lock_query(|c| c.hud.split_path_prefix);
        let prefix = prefix.as_str();
        tracing::info!("scaleform: split path probe (prefix {prefix:?})");
        let containers = LAYER_CONTAINERS.iter().flat_map(|layer| layer.iter());
        for path in containers.chain(OVERLAY_CLIPS.iter()) {
            let full = format!("{prefix}{path}.visible\0");
            let mut value = Value::new_boolean(true);
            let ok = movie_root.GetVariable(&mut value, full.as_ptr());
            if ok && value.Type & 0x8F == Value::VT_BOOLEAN {
                let visible = value.mValue & 0xFF != 0;
                tracing::info!("scaleform: probe {path}: visible={visible}");
            } else if ok {
                tracing::info!("scaleform: probe {path}: resolves (non-boolean visible)");
            } else {
                tracing::warn!("scaleform: probe {path}: does not resolve");
            }
        }
    }
}

/// A tree node's instance name. The `name` field is a `Scaleform::String`: its `pData` carries
/// heap flags in the pointer's low bits, and the characters live at `+0xC` past the `DataDesc`
/// header (u64 size, i32 refcount), NUL-terminated.
unsafe fn node_name(node: &AmpMovieObjectDesc) -> String {
    if node.name.is_null() {
        return "<null>".to_string();
    }
    // SAFETY: forwarded from the tree walk; the string data outlives the node.
    unsafe {
        let desc = (node.name as usize & !7) as *const u8;
        std::ffi::CStr::from_ptr(desc.add(0xC) as *const i8)
            .to_string_lossy()
            .into_owned()
    }
}

/// Log one tree node and recurse into its children. `prefix` is the dot-joined path of the
/// ancestors; `lines` counts emitted nodes.
unsafe fn dump_node(node: &AmpMovieObjectDesc, prefix: &mut String, lines: &mut usize) {
    // SAFETY (whole body): the node came from a live GetDisplayObjectsTree result that is not
    // released until the walk completes; `name` points into the node's own string allocation.
    unsafe {
        let name = node_name(node);
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        tracing::info!("scaleform: {path} ({} children)", node.child_count);
        *lines += 1;

        if node.children.is_null() {
            return;
        }
        for i in 0..node.child_count as usize {
            let child = *node.children.add(i);
            if let Some(child) = child.as_ref() {
                let saved = prefix.len();
                if !prefix.is_empty() {
                    prefix.push('.');
                }
                prefix.push_str(&name);
                dump_node(child, prefix, lines);
                prefix.truncate(saved);
            }
        }
    }
}

/// Walk the live display tree once: locate `MCI_safe_area_center`, derive the split path prefix
/// from its parent's path, and collect that parent's anonymous (`instanceNNNN`) children -- the
/// POI pool -- as the split's dynamic marker clips. Returns whether the layout was found (the
/// HUD movie may not be attached yet).
fn discover_layout(movie_impl: &MovieImpl, movie_root: &Movie) -> bool {
    // SAFETY: called on the capture thread; the heap is live; the tree is released after the walk.
    unsafe {
        if movie_impl.pHeap.is_null() {
            return false;
        }
        let root = movie_root.GetDisplayObjectsTree(movie_impl.pHeap);
        let Some(root_ref) = root.as_ref() else {
            return false;
        };
        let found = find_hud_clip(root_ref, &mut String::new());
        root_ref.Release();

        let Some((prefix, anonymous)) = found else {
            return false;
        };
        if let Err(e) = crate::config::CONFIG
            .lock()
            .hud
            .split_path_prefix
            .set(&prefix)
        {
            tracing::warn!("scaleform: layout discovery: {e}");
            return false;
        }

        // Resolve a pinned managed handle per clip, on this (capture) thread. The Value is an
        // intrusive list node on the movie's external-references list once filled, so it is
        // boxed first and never moved afterwards.
        let mut resolved = 0usize;
        let mut total = 0usize;
        let mut resolve = |path: &str| -> ClipHandle {
            total += 1;
            let full = format!("{prefix}{path}\0");
            let mut value = Box::new(Value::new_boolean(false));
            let ok = movie_root.GetVariable(value.as_mut(), full.as_ptr());
            let is_display_object = ok
                && value.Type & 0x8F == Value::VT_DISPLAY_OBJECT
                && !value.pObjectInterface.is_null();
            if is_display_object {
                resolved += 1;
            } else {
                tracing::warn!("scaleform: layout discovery: {path} did not resolve to a clip");
            }
            ClipHandle::new(is_display_object.then_some(value))
        };

        let handles = ClipHandles {
            containers: std::array::from_fn(|layer| {
                LAYER_CONTAINERS[layer]
                    .iter()
                    .map(|path| resolve(path))
                    .collect()
            }),
            overlays: OVERLAY_CLIPS.iter().map(|path| resolve(path)).collect(),
            dynamic: anonymous.iter().map(|name| resolve(name)).collect(),
        };
        tracing::info!(
            "scaleform: layout discovery: prefix {prefix:?}, {resolved}/{total} clips resolved \
             ({} anonymous marker clips)",
            anonymous.len()
        );

        release_clip_handles();
        *CLIP_HANDLES.lock() = Some(handles);
        DISCOVERED_IN_HUD.store(
            crate::hud::current_mode() == crate::hud::HudMode::Hud,
            std::sync::atomic::Ordering::Relaxed,
        );
        resolved > 0
    }
}

/// Recursively search for the node named `MCI_safe_area_center`; on a hit, return its parent's
/// dot-path prefix (ending in a dot, or empty at the root) and the parent's anonymous children.
unsafe fn find_hud_clip(
    node: &AmpMovieObjectDesc,
    path: &mut String,
) -> Option<(String, Vec<String>)> {
    // SAFETY (whole body): the tree is live for the duration of the walk.
    unsafe {
        if node.children.is_null() {
            return None;
        }
        let count = node.child_count as usize;
        let children: Vec<&AmpMovieObjectDesc> = (0..count)
            .filter_map(|i| (*node.children.add(i)).as_ref())
            .collect();

        if children
            .iter()
            .any(|c| node_name(c) == "MCI_safe_area_center")
        {
            let prefix = if path.is_empty() {
                String::new()
            } else {
                format!("{path}.")
            };
            let anonymous = children
                .iter()
                .map(|c| node_name(c))
                .filter(|name| !name.starts_with("MCI_") && name != "<null>")
                .collect();
            return Some((prefix, anonymous));
        }

        for child in children {
            let name = node_name(child);
            let saved = path.len();
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(&name);
            if let Some(found) = find_hud_clip(child, path) {
                return Some(found);
            }
            path.truncate(saved);
        }
        None
    }
}

/// Set `<path>.visible` through `Movie::SetVariable`, logging the outcome.
fn set_clip_visible(movie: &Movie, path: &str, visible: bool) {
    let full_path = format!("{path}.visible");
    let Ok(c_path) = CString::new(full_path.clone()) else {
        tracing::warn!("scaleform: the clip path {full_path:?} contains a NUL; ignoring");
        return;
    };
    let value = Value::new_boolean(visible);
    // SAFETY: called on the capture thread with a checked live movie; the value is an unmanaged
    // stack boolean the movie copies.
    let ok = unsafe { movie.SetVariable(c_path.as_ptr() as *const u8, &value, 0) };
    if ok {
        tracing::info!("scaleform: set {full_path} = {visible}");
    } else {
        tracing::warn!("scaleform: SetVariable failed for {full_path} (path not found?)");
    }
}
