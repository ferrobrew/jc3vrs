//! The deferred frame tail: a dedicated worker thread that drains the final dispatch, blits the
//! eyes, presents the mirror, and submits the XR frame, so the main thread returns to the next
//! frame's sim tick instead of serializing behind the draw thread and the GPU.
//!
//! The profiler measured the cost of the inline tail as the frame-boundary "GPU idle" bubble
//! (~8.5 ms/frame at 56% GPU utilization): after the last dispatch, the main thread drains the
//! draw thread, blits, submits, and then runs the whole inter-frame relay (sim, engine render
//! update, `xrWaitFrame`) while the GPU has nothing queued. Deferring the tail overlaps the relay
//! with the draw thread's eye-1 walk and the GPU's tail.
//!
//! Synchronization is by construction rather than by flushing:
//!
//! - The [`super::FrameContext`] in the job holds the VR runtime lock (`parking_lot` with
//!   `send_guard` makes the guard movable). The next frame's `vr::update` / `frame_begin` block on
//!   that lock, so no next-frame VR work — and nothing downstream of it, including the next
//!   dispatches — can start before the tail submits.
//! - The blit and mirror are recorded on the engine's immediate context under the context mutex
//!   *before* the lock is released, so the next frame's `PostDraw` capture copies cannot overwrite
//!   the eye textures the tail reads.
//! - The worker drains the draw thread itself (CPU-side submission completeness) before recording
//!   the blit, preserving same-context command order for the eyes.
//!
//! The worker must be stopped ([`shutdown`]) before the DLL unloads; a live thread in an unmapped
//! image is a process crash.

use std::{
    sync::{OnceLock, mpsc},
    time::{Duration, Instant},
};

use jc3gi::{cpu_fragment::CpuPrimaryCount, graphics_engine::graphics_engine::GraphicsEngine};
use windows::Win32::{Foundation::HANDLE, System::Threading::WaitForSingleObject};

use super::{FrameContext, config::VrConfig};

/// One deferred frame tail.
pub struct TailJob {
    /// The frame to submit; carries the VR runtime lock.
    pub frame: FrameContext,
    /// The frame's VR config snapshot (blit gamma, mirror settings).
    pub vr_cfg: VrConfig,
    /// Whether to present the desktop mirror (a VR session is running and the mirror is on).
    pub mirror: bool,
}

enum Message {
    Job(Box<TailJob>),
    Quit,
}

static SENDER: OnceLock<mpsc::Sender<Message>> = OnceLock::new();

/// Hands the frame tail to the worker, spawning it on first use. Returns the job back to the
/// caller (to run inline) if the worker is unavailable (shutting down).
pub fn submit(job: Box<TailJob>) -> Result<(), Box<TailJob>> {
    let sender = SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("jc3vrs-frame-tail".to_owned())
            .spawn(move || worker(rx))
            .expect("spawning the frame-tail worker");
        tx
    });
    sender.send(Message::Job(job)).map_err(|e| {
        let Message::Job(job) = e.0 else {
            unreachable!()
        };
        job
    })
}

/// Stops the worker and waits briefly for it to exit. Called on shutdown before the hooks are
/// torn down; the in-flight job (if any) finishes first because the channel is drained in order.
pub fn shutdown() {
    if let Some(sender) = SENDER.get() {
        let _ = sender.send(Message::Quit);
        // The worker acknowledges by closing the ack channel; poll its liveness cheaply instead of
        // holding a JoinHandle across the OnceLock (the handle is not clonable out of the init
        // closure). A bounded wait keeps shutdown from hanging on a stuck tail.
        for _ in 0..100 {
            if QUIT_ACKED.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        tracing::warn!(target: "vr", "the frame-tail worker did not stop within 1 s");
    }
}

static QUIT_ACKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn worker(rx: mpsc::Receiver<Message>) {
    #[cfg(feature = "profiler")]
    crate::profiler::label_thread("tail");
    while let Ok(message) = rx.recv() {
        match message {
            Message::Job(job) => run_tail(*job),
            Message::Quit => break,
        }
    }
    QUIT_ACKED.store(true, std::sync::atomic::Ordering::Release);
}

/// How long the tail waits on any one draw-completion signal before proceeding anyway. A stuck
/// draw thread has already lost the frame; proceeding lets the submit release the VR lock rather
/// than wedging the whole loop behind it.
const DRAIN_TIMEOUT: Duration = Duration::from_millis(500);

/// Spin-waits `signal` non-zero with a yield per iteration, giving up at `deadline`.
///
/// # Safety
/// `signal` must point to a live `u32` for the duration of the poll.
unsafe fn poll_signal(signal: *const u32, deadline: Instant, what: &str) {
    while unsafe { std::ptr::read_volatile(signal) } == 0 {
        if Instant::now() > deadline {
            tracing::warn!(target: "vr", "the frame tail timed out draining the {what} signal");
            return;
        }
        std::thread::yield_now();
    }
}

fn run_tail(job: TailJob) {
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("Frame tail");

    // Drain the draw thread (CPU submission completeness) so the blit records after the last
    // eye's commands on the immediate context.
    //
    // Deliberately NOT `WaitForCPUDrawToFinish` + `drain_draw_thread_fragment`: both funnel into
    // `CpuFragmentWaitUntilSignalIsNonZero`, whose wait loop is *work-stealing* -- it pops queued
    // fragments and runs them on the calling thread. This thread is not registered with the
    // fragment system, so executing engine jobs here faults instantly (the first tail-defer
    // crash). Instead, observe the same completion state directly: poll the outstanding
    // fragment's signal, wait the finished event, and poll the draw-dispatch work signal.
    // SAFETY: reads the live graphics-engine singleton's fields; no engine code runs.
    unsafe {
        if let Some(ge) = GraphicsEngine::get() {
            #[cfg(feature = "profiler")]
            puffin::profile_scope!("WaitForCPUDraw + drain (tail)");
            let deadline = Instant::now() + DRAIN_TIMEOUT;

            let fragment_signal = ge.m_DrawFragmentSignal;
            if !fragment_signal.is_null() {
                poll_signal(fragment_signal, deadline, "draw fragment");
            }
            if !ge.m_CPUDrawFinished && !ge.m_CPUFinishedDrawingEvent.is_null() {
                WaitForSingleObject(
                    HANDLE(ge.m_CPUFinishedDrawingEvent),
                    DRAIN_TIMEOUT.as_millis() as u32,
                );
            }
            if crate::config::Config::lock_query(|c| c.stereo.drain_draw_fragment)
                && CpuPrimaryCount() > 1
            {
                poll_signal(
                    &raw const ge.m_DrawThreadWorkSignal,
                    deadline,
                    "draw dispatch",
                );
            }
        }
    }

    // The final dispatch is now fully drained, so the per-dispatch stereo state it read is no
    // longer live: reset it here rather than on the main thread, which would race the in-flight
    // eye-1 render (freezing the right eye -- see the deferred-reset note in `game_update_render`).
    crate::hooks::game::reset_dispatch_state();

    // Mirror before the submit: both must be recorded before the runtime lock is released, and
    // the submit releasing the lock is what keeps the next frame off the capture textures. The
    // flat overlay was rendered into its texture on eye 0's post-draw, so the mirror only
    // composites it -- safe off-thread.
    if job.mirror {
        #[cfg(feature = "profiler")]
        puffin::profile_scope!("Desktop mirror (tail)");
        super::present_mirror(usize::from(job.vr_cfg.mirror_eye));
    }

    let should_render = job.frame.should_render();
    {
        #[cfg(feature = "profiler")]
        puffin::profile_scope!("VR present + submit (tail)");
        super::present_and_submit(job.frame, &job.vr_cfg);
    }
    crate::hooks::game::log_vr_frame_health(should_render);
}
