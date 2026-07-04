//! Scaleform display-tree debugging: dump the live clip tree and toggle clip visibility by path.
//!
//! The multi-texture HUD split (issue #14) needs the runtime clip paths of the element groups
//! (`MCI_poi_stage`, `MCI_safe_area_center.MCI_reticles`, ...) and a working per-clip visibility
//! toggle. This module provides both as debug operations: the UI thread queues requests, and
//! [`process_requests`] executes them on the game update thread (the Scaleform capture thread,
//! where the display tree is stable and the engine makes its own AS3 calls).
//!
//! The tree dump walks [`Movie::GetDisplayObjectsTree`] and logs one line per clip; the toggle
//! writes `<path>._visible` through [`Movie::SetVariable`]. Both are read-modify operations on the
//! live movie, guarded by a vtable check so a mismatched `m_Movie` model logs instead of calling
//! through a wrong vtable.

use std::ffi::CString;

use jc3gi::ui::{
    scaleform::{AmpMovieObjectDesc, Movie, MovieImpl, Value},
    ui_manager::UIManager,
};
use parking_lot::Mutex;

/// A queued Scaleform debug operation. Queued from the UI thread, executed on the game thread.
enum Request {
    /// Log the movie's full display tree, one line per clip.
    DumpTree,
    /// Set `<path>._visible` on the clip at the dot-separated `path` (from the root timeline).
    SetClipVisible { path: String, visible: bool },
}

/// The pending requests. UI thread pushes, game thread drains.
static REQUESTS: Mutex<Vec<Request>> = Mutex::new(Vec::new());

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
    let requests = std::mem::take(&mut *REQUESTS.lock());
    if requests.is_empty() {
        return;
    }
    let Some((movie_impl, movie_root)) = live_movie() else {
        tracing::warn!("scaleform: the UI movie is not available; dropping the queued requests");
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
        if vftable != Movie::VFTABLE {
            tracing::error!(
                "scaleform: the AS3 root's vtable is {vftable:#x}, expected the MovieRoot vtable \
                 {:#x}; refusing to operate on it",
                Movie::VFTABLE
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
        let containers = crate::hud::split::LAYER_CONTAINERS
            .iter()
            .flat_map(|layer| layer.iter());
        for path in containers.chain(crate::hud::split::OVERLAY_CLIPS.iter()) {
            let full = format!("{prefix}{path}._visible\0");
            let mut value = Value::new_boolean(true);
            let ok = movie_root.GetVariable(&mut value, full.as_ptr());
            if ok && value.Type & 0x8F == Value::VT_BOOLEAN {
                let visible = value.mValue & 0xFF != 0;
                tracing::info!("scaleform: probe {path}: visible={visible}");
            } else if ok {
                tracing::info!("scaleform: probe {path}: resolves (non-boolean _visible)");
            } else {
                tracing::warn!("scaleform: probe {path}: does not resolve");
            }
        }
    }
}

/// Log one tree node and recurse into its children. `prefix` is the dot-joined path of the
/// ancestors; `lines` counts emitted nodes.
unsafe fn dump_node(node: &AmpMovieObjectDesc, prefix: &mut String, lines: &mut usize) {
    // SAFETY (whole body): the node came from a live GetDisplayObjectsTree result that is not
    // released until the walk completes; `name` points into the node's own string allocation.
    unsafe {
        let name = if node.name.is_null() {
            "<null>".to_string()
        } else {
            std::ffi::CStr::from_ptr(node.name as *const i8)
                .to_string_lossy()
                .into_owned()
        };
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

/// Set `<path>._visible` through `Movie::SetVariable`, logging the outcome.
fn set_clip_visible(movie: &Movie, path: &str, visible: bool) {
    let full_path = format!("{path}._visible");
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
