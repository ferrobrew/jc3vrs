//! On-demand trace capture: records ~5 s of puffin frames into memory, then dumps them to a
//! timestamped Chrome trace-event JSON file in the session's `profile/` directory (see
//! [`crate::session`]) for offline analysis (`ui.perfetto.dev` or `chrome://tracing`).
//!
//! A capture is a puffin frame *sink*: while recording, every finished frame's data is cloned into
//! a buffer. The state machine is driven once per real frame from [`crate::profiler::new_frame`] via
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

use crate::profiler::apply_scopes_on;

/// The default capture duration, in seconds (~450 frames at 90 Hz).
pub const DEFAULT_CAPTURE_SECS: f32 = 5.0;

/// Whether a capture is actively recording frames (drives [`crate::profiler::apply_scopes_on`]).
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
    // Refuse once eject has begun: a capture started here would still be recording (or about to
    // spawn its writer thread) when the payload unmaps, and `finish` cannot be allowed to run
    // during teardown -- see the same check in `tick`.
    if crate::is_shutting_down() {
        return false;
    }

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
    apply_scopes_on();
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
    // Once eject has begun, stop advancing the state machine: `update` keeps calling this every
    // frame for as long as the game thread keeps ticking during `shutdown_startup`'s teardown, and
    // `finish` below would spawn a new writer thread that nothing then waits for outside of
    // `shutdown`'s bounded budget. An in-progress recording is simply abandoned rather than
    // flushed; the game is about to exit regardless.
    if crate::is_shutting_down() {
        return;
    }

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
    apply_scopes_on();

    GlobalProfiler::lock().remove_sink(state.sink_id);

    // Serialize on a background thread: a capture is hundreds of frames of scope data (tens to
    // hundreds of megabytes of JSON, plus per-frame decompression), and this runs from the main
    // thread's frame tick — writing inline would freeze the game (and the HMD) for seconds.
    let frames = std::mem::take(&mut *state.frames.lock());
    // `Release` so that `shutdown`'s `Acquire` poll, once it observes this, also sees every write
    // the thread below performs before it flips `WRITING` back to `false`.
    WRITING.store(true, Ordering::Release);
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
        WRITING.store(false, Ordering::Release);
    });
}

/// Whether a finished capture is still being serialized to disk on the background thread.
static WRITING: AtomicBool = AtomicBool::new(false);

pub fn is_writing() -> bool {
    WRITING.load(Ordering::Acquire)
}

/// Waits for an in-flight capture writer to finish, up to a bounded budget. Called on eject
/// alongside [`crate::vr::tail::shutdown`], which this mirrors: the writer is an unjoined thread
/// (spawned from [`finish`] because inline serialization would freeze the frame it runs on for
/// seconds), and a thread still executing when [`crate::module::exit`] unmaps the image parks
/// forever holding whatever it took, wedging the process with nothing left to log. Waiting longer
/// is strictly better than unloading underneath a live thread.
///
/// Returns whether the writer is confirmed stopped (or was never running). A `false` return means
/// the caller must not unload -- see [`crate::vr::tail::shutdown`]'s doc comment for the full
/// argument.
#[must_use = "a writer that did not finish means the payload must stay mapped"]
pub fn shutdown() -> bool {
    for poll in 0..SHUTDOWN_POLLS {
        if !WRITING.load(Ordering::Acquire) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        if poll > 0 && poll % 100 == 0 {
            tracing::debug!(
                "profiler: capture writer still running, {} s elapsed",
                poll / 100,
            );
        }
    }
    tracing::error!(
        "profiler: the capture writer did not finish within {} s; leaving the payload mapped \
         rather than unmapping under a live thread",
        SHUTDOWN_POLLS / 100,
    );
    false
}

/// How long [`shutdown`] waits for the writer, in 10 ms polls. 10 s: the capture module's own
/// comment on [`finish`] puts a dump at "tens to hundreds of megabytes of JSON, plus per-frame
/// decompression" -- generous enough to cover the largest captures this mod takes (a few hundred
/// frames of scope data) without being an unbounded wait, since the cost of waiting too long is
/// only a slower eject, while unloading too early is the unrecoverable wedge this exists to avoid.
///
/// Worst case: a 5-second capture at ~3 dispatches/frame at 90 Hz is ~1350 frames, producing tens to
/// hundreds of megabytes of JSON. On an HDD or network share, writing 200 MB+ could approach 10 s. If
/// this proves insufficient, increase to 2000 (20 s).
const SHUTDOWN_POLLS: u32 = 1000;

fn write_capture(frames: &[Arc<FrameData>]) -> anyhow::Result<PathBuf> {
    let path = capture_path()?;
    crate::profiler::chrome_trace::write_chrome_trace(
        &path,
        frames,
        crate::profiler::scope_details(),
    )?;
    Ok(path)
}

/// A timestamped output path in the session's `profile/` directory,
/// `jc3vrs-profile-<stamp>.json` (the per-file stamp disambiguates several captures in one run).
fn capture_path() -> anyhow::Result<PathBuf> {
    let dir = crate::session::subdir("profile")
        .and_then(|r| r.ok())
        .ok_or_else(|| {
            anyhow::anyhow!("profiler: could not resolve the session profile directory")
        })?;
    Ok(dir.join(format!("jc3vrs-profile-{}.json", crate::session::stamp())))
}
