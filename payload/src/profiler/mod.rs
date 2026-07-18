//! The in-game profiler (issue #34), built on puffin: CPU scopes across the mod's hooks and the
//! engine's frame phases, a GPU timestamp layer over the engine's own query wrappers, a real-time
//! flame graph in the Performance tab, and an on-demand ~5 s trace capture dumped as Chrome
//! trace-event JSON for offline analysis.
//!
//! Everything here is compiled under the `profiler` cargo feature. The runtime cost while the
//! profiler is disabled (panel closed, no capture) is an atomic check per scope plus a few
//! thread-local touches per dispatch: puffin's `are_scopes_on()` gates scope recording, and the
//! GPU layer issues no queries while it is off.
//!
//! Threading: the game calls `Game::Update` once per real frame on the main thread
//! ([`new_frame`] runs there), and the draw thread executes `HandleDrawThreadTask` once per
//! dispatch (two per frame in stereo, three on far-field share frames). puffin keeps one scope
//! stream per thread, so main-thread and draw-thread scopes land in separate lanes; the GPU layer
//! reports a third, synthetic "GPU" lane.

pub mod capture;
pub mod chrome_trace;
pub mod gpu;
pub mod ui;

use std::{
    collections::HashMap,
    ffi::CStr,
    sync::atomic::{AtomicBool, Ordering},
};

use jc3gi::graphics_engine::{render_engine::GetRenderPassName_ADDRESS, render_pass::RenderPass};
use parking_lot::Mutex;
use puffin::{GlobalProfiler, ScopeDetails, ScopeId, ThreadProfiler};

/// Whether the profiler UI has scope collection enabled (independent of an active capture, which
/// forces collection on for its duration).
static UI_ENABLED: AtomicBool = AtomicBool::new(false);

/// Turns scope collection on or off from the UI. Collection stays on while a capture is running
/// regardless.
pub fn set_ui_enabled(enabled: bool) {
    UI_ENABLED.store(enabled, Ordering::Relaxed);
    apply_scopes_on();
}

pub fn ui_enabled() -> bool {
    UI_ENABLED.load(Ordering::Relaxed)
}

/// Whether puffin scope collection is currently on (UI toggle or an active capture). A single
/// relaxed atomic load; the gate on every profiled hook.
pub fn are_scopes_on() -> bool {
    puffin::are_scopes_on()
}

/// Recomputes puffin's global scope switch from the UI toggle and the capture state.
pub(crate) fn apply_scopes_on() {
    puffin::set_scopes_on(UI_ENABLED.load(Ordering::Relaxed) || capture::is_recording());
}

/// Called once at the start of every real frame, on the main thread (from `crate::update`).
/// Finishes the previous puffin frame (collecting every thread's stream and feeding the sinks)
/// and advances the capture state machine.
pub fn new_frame() {
    label_thread("game");
    install_details_sink();
    // Advance the frame even while collection is off: a stream flushed just as collection was
    // toggled off would otherwise sit parked in the profiler and be glued onto the front of the
    // next enabled frame, minutes later, ruining its range (and a capture's timestamp base).
    // An empty frame costs one mutex lock and is discarded by puffin.
    GlobalProfiler::lock().new_frame();
    capture::tick();
}

/// Labels the current thread's puffin lane. The game's threads are foreign (not Rust-spawned), so
/// they all report an empty thread name; without a label the main and draw threads collapse into
/// one indistinguishable lane. First write wins per thread, so on the single-core path — where the
/// draw task runs inline on the main thread — the main thread keeps its "game" label.
pub fn label_thread(label: &'static str) {
    THREAD_LABEL.with(|slot| {
        if slot.get().is_empty() {
            slot.set(label);
            ThreadProfiler::initialize(puffin::now_ns, labelled_reporter);
        }
    });
}

thread_local! {
    static THREAD_LABEL: std::cell::Cell<&'static str> = const { std::cell::Cell::new("") };
}

/// The per-thread stream reporter: fills in the thread-local label when the OS thread has no name
/// of its own, then forwards to the global profiler as usual.
fn labelled_reporter(
    mut info: puffin::ThreadInfo,
    scope_details: &[ScopeDetails],
    stream_scope_times: &puffin::StreamInfoRef<'_>,
) {
    if info.name.is_empty() {
        info.name = THREAD_LABEL.with(std::cell::Cell::get).to_owned();
    }
    puffin::internal_profile_reporter(info, scope_details, stream_scope_times);
}

/// Every scope's details, harvested continuously by a permanent puffin sink. Captures resolve
/// names from this rather than from their own frames' deltas alone: puffin's scope-snapshot
/// re-emit is discarded if the next frame happens to be empty (e.g. collection enabled by F9
/// between frames), which otherwise strips a capture's names entirely.
static SCOPE_DETAILS: Mutex<Option<puffin::ScopeCollection>> = Mutex::new(None);

/// A snapshot of every scope's details seen so far.
pub fn scope_details() -> puffin::ScopeCollection {
    SCOPE_DETAILS.lock().clone().unwrap_or_default()
}

/// Installs the details-harvesting sink once. The sink runs under the profiler's lock each
/// `new_frame`; it takes only the `SCOPE_DETAILS` lock, which nothing holds while locking the
/// profiler, so the order is acyclic.
fn install_details_sink() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let mut profiler = GlobalProfiler::lock();
        profiler.add_sink(Box::new(|frame| {
            if frame.scope_delta.is_empty() {
                return;
            }
            let mut details = SCOPE_DETAILS.lock();
            let details = details.get_or_insert_with(Default::default);
            for d in &frame.scope_delta {
                details.insert(d.clone());
            }
        }));
        profiler.emit_scope_snapshot();
    });
}

/// A registry of puffin scope ids for engine-supplied names that only exist at runtime (render
/// pass names, render-block-type names). Keyed by the name text: the set of distinct names is
/// small and fixed, so a single registration per name amortizes to nothing.
static NAMED_SCOPES: Mutex<Option<HashMap<String, ScopeId>>> = Mutex::new(None);

/// Opens a scope for the engine-supplied `name`, registering it on first sight. Returns `None`
/// while scope collection is off.
pub fn scope_for_name(name: &str) -> Option<EngineScope> {
    let id = named_scope_id(name)?;
    EngineScope::begin(id, "")
}

/// Opens a scope named for the render pass `pass` is drawing, resolved through the engine's own
/// pass-name table. Returns `None` while scope collection is off or the name is unavailable.
///
/// # Safety
/// `pass` must be null or a live [`RenderPass`] whose `m_Index` is its render-pass id.
pub unsafe fn pass_scope(pass: *mut RenderPass) -> Option<EngineScope> {
    if !are_scopes_on() || pass.is_null() {
        return None;
    }
    // Call the pass-name lookup through a raw `i32` signature rather than the generated
    // `RenderPassId` enum: `m_Index` can hold an unnamed id (e.g. `0x82`) that is not a valid enum
    // discriminant, and transmuting such a value into the enum would be undefined behaviour. The
    // engine's lookup returns `"NONE"` for those.
    let name = unsafe {
        let index = i32::from((*pass).m_Index);
        let lookup: unsafe extern "system" fn(i32) -> *const u8 =
            std::mem::transmute(GetRenderPassName_ADDRESS);
        let ptr = lookup(index);
        if ptr.is_null() {
            return None;
        }
        CStr::from_ptr(ptr.cast()).to_str().ok()?
    };
    scope_for_name(name)
}

fn named_scope_id(name: &str) -> Option<ScopeId> {
    let mut registry = NAMED_SCOPES.lock();
    let registry = registry.get_or_insert_with(HashMap::new);
    if let Some(&id) = registry.get(name) {
        return Some(id);
    }
    let id = *GlobalProfiler::lock()
        .register_user_scopes(&[ScopeDetails::from_scope_name(name.to_owned())])
        .first()?;
    registry.insert(name.to_owned(), id);
    Some(id)
}

/// An RAII guard for a scope opened by id on the current thread. Dropping it closes the scope.
/// puffin scopes are per-thread and strictly nested, so the guard must be dropped on the thread
/// that created it, in LIFO order.
pub struct EngineScope {
    start_offset: usize,
}

impl EngineScope {
    /// Opens a scope for `id` with optional `data` shown alongside the name in the flame graph.
    /// Returns `None` while scope collection is off.
    pub fn begin(id: ScopeId, data: &str) -> Option<Self> {
        if !puffin::are_scopes_on() {
            return None;
        }
        let start_offset = ThreadProfiler::call(|tp| tp.begin_scope(id, data));
        Some(Self { start_offset })
    }
}

impl Drop for EngineScope {
    fn drop(&mut self) {
        ThreadProfiler::call(|tp| tp.end_scope(self.start_offset));
    }
}

/// The single render-block-type scope active on the current thread, if any. A render pass draws
/// its blocks in type runs, switching type via `CRenderPass::ChangeRenderBlockType`; that hook
/// replaces this slot (closing the previous type's scope, opening the next), and the pass draw
/// (`CRenderPass::DoDraw`) clears it at the tail once the final run is done.
pub mod type_scope {
    use std::cell::RefCell;

    use super::EngineScope;

    thread_local! {
        static ACTIVE: RefCell<Option<EngineScope>> = const { RefCell::new(None) };
    }

    /// Stores `scope` as the active type scope. The caller must [`clear`] first (before the new
    /// scope is even *created*): puffin streams are strictly LIFO per thread, so the previous
    /// scope's end has to be recorded before the next one's begin. The `take` here is only a
    /// safety net for a caller that skipped `clear`.
    pub fn replace(scope: Option<EngineScope>) {
        ACTIVE.with_borrow_mut(|slot| {
            slot.take();
            *slot = scope;
        });
    }

    /// Closes the active type scope, if any. Called at the end of a pass draw.
    pub fn clear() {
        ACTIVE.with_borrow_mut(|slot| {
            slot.take();
        });
    }
}
