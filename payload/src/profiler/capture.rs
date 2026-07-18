//! On-demand trace capture: records ~5 s of puffin frames into memory, then dumps them to a
//! timestamped Chrome trace-event JSON file next to the log for offline analysis (`ui.perfetto.dev`
//! or `chrome://tracing`).
//!
//! A capture is a puffin frame *sink*: while recording, every finished frame's data is cloned into
//! a buffer. The state machine is driven once per real frame from [`super::new_frame`] via
//! [`tick`], and started from the UI button or the F9 hotkey via [`start`]. Because a capture
//! forces scope collection on for its duration, a trace can be taken even with the profiler panel
//! closed and in-headset.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use parking_lot::Mutex;
use puffin::{FrameData, FrameSinkId, GlobalProfiler};

/// The default capture duration, in seconds (~450 frames at 90 Hz).
pub const DEFAULT_CAPTURE_SECS: f32 = 5.0;

/// Whether a capture is actively recording frames (drives [`super::apply_scopes_on`]).
static RECORDING: AtomicBool = AtomicBool::new(false);

pub fn is_recording() -> bool {
    RECORDING.load(Ordering::Relaxed)
}

struct CaptureState {
    /// The puffin sink feeding [`frames`], removed when the capture ends.
    sink_id: FrameSinkId,
    frames: Arc<Mutex<Vec<Arc<FrameData>>>>,
    started: Instant,
    duration_secs: f32,
}

static CAPTURE: Mutex<Option<CaptureState>> = Mutex::new(None);

/// The outcome of the most recent capture, surfaced in the UI (the written path, or an error).
static LAST_RESULT: Mutex<Option<Result<PathBuf, String>>> = Mutex::new(None);

/// Snapshots the last capture's result for display. `Ok` carries the written file path.
pub fn last_result() -> Option<Result<PathBuf, String>> {
    LAST_RESULT.lock().clone()
}

/// Begins a capture of `duration_secs` seconds. A no-op (returns `false`) if one is already
/// running. Registers a puffin sink, forces scope collection on, and asks the profiler to emit a
/// full scope snapshot so the buffered frames can resolve every scope name at dump time.
pub fn start(duration_secs: f32) -> bool {
    let mut capture = CAPTURE.lock();
    if capture.is_some() {
        return false;
    }

    let frames = Arc::new(Mutex::new(Vec::new()));
    let sink_frames = frames.clone();
    let mut profiler = GlobalProfiler::lock();
    let sink_id = profiler.add_sink(Box::new(move |frame| {
        sink_frames.lock().push(frame);
    }));
    // The sink attaches after scopes have already been registered this session, so request a
    // snapshot of all scope details -- otherwise early scopes would be missing their names.
    profiler.emit_scope_snapshot();
    drop(profiler);

    *capture = Some(CaptureState {
        sink_id,
        frames,
        started: Instant::now(),
        duration_secs,
    });
    RECORDING.store(true, Ordering::Relaxed);
    super::apply_scopes_on();
    tracing::info!("profiler: capturing {duration_secs:.1}s of frames");
    true
}

/// The remaining capture time as a `(elapsed, total)` pair of seconds, or `None` when idle.
pub fn progress() -> Option<(f32, f32)> {
    CAPTURE
        .lock()
        .as_ref()
        .map(|c| (c.started.elapsed().as_secs_f32(), c.duration_secs))
}

/// Advances the capture state machine; called once per real frame. When the capture window
/// elapses, detaches the sink, writes the trace, and records the result.
pub fn tick() {
    let done = {
        let capture = CAPTURE.lock();
        match capture.as_ref() {
            Some(c) => c.started.elapsed().as_secs_f32() >= c.duration_secs,
            None => false,
        }
    };
    if done {
        finish();
    }
}

fn finish() {
    let Some(state) = CAPTURE.lock().take() else {
        return;
    };
    RECORDING.store(false, Ordering::Relaxed);
    super::apply_scopes_on();

    GlobalProfiler::lock().remove_sink(state.sink_id);

    // Serialize on a background thread: a capture is hundreds of frames of scope data (tens to
    // hundreds of megabytes of JSON, plus per-frame decompression), and this runs from the main
    // thread's frame tick — writing inline would freeze the game (and the HMD) for seconds.
    let frames = std::mem::take(&mut *state.frames.lock());
    WRITING.store(true, Ordering::Relaxed);
    std::thread::spawn(move || {
        let result = write_capture(&frames);
        match &result {
            Ok(path) => tracing::info!(
                "profiler: captured {} frames -> {}",
                frames.len(),
                path.display()
            ),
            Err(e) => tracing::error!("profiler: capture dump failed: {e}"),
        }
        *LAST_RESULT.lock() = Some(result.map_err(|e| e.to_string()));
        WRITING.store(false, Ordering::Relaxed);
    });
}

/// Whether a finished capture is still being serialized to disk on the background thread.
static WRITING: AtomicBool = AtomicBool::new(false);

pub fn is_writing() -> bool {
    WRITING.load(Ordering::Relaxed)
}

fn write_capture(frames: &[Arc<FrameData>]) -> anyhow::Result<PathBuf> {
    let path = capture_path()?;
    super::chrome_trace::write_chrome_trace(&path, frames, super::scope_details())?;
    Ok(path)
}

/// A timestamped output path next to the payload DLL: `jc3vrs-profile-YYYYMMDD-HHMMSS.json`.
fn capture_path() -> anyhow::Result<PathBuf> {
    let dir = crate::module::get_path()
        .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
        .ok_or_else(|| anyhow::anyhow!("profiler: could not resolve the payload DLL directory"))?;
    let stamp = jiff::Zoned::now().strftime("%Y%m%d-%H%M%S").to_string();
    Ok(dir.join(format!("jc3vrs-profile-{stamp}.json")))
}
