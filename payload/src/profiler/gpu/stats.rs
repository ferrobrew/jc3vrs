//! The rolling busy/starved/idle accumulator behind the periodic log line and the UI readout.

use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::profiler::gpu::{pass_timestamps_enabled, queries::DispatchLane};

/// A completed summary window's per-frame averages, in milliseconds.
#[derive(Clone, Copy)]
pub struct GpuSummary {
    pub frames: u32,
    pub dispatches_per_frame: f32,
    pub busy_ms: f32,
    pub starved_ms: f32,
    pub idle_ms: f32,
    pub submit_ms: f32,
}

/// The most recent GPU summary window, or `None` before the first one completes.
pub fn summary() -> Option<GpuSummary> {
    *SUMMARY.lock()
}

/// The last completed summary window, for the UI readout.
static SUMMARY: Mutex<Option<GpuSummary>> = Mutex::new(None);

/// One dispatch's decomposition, as accumulated into the rolling summary.
#[derive(Clone, Copy, Default)]
pub(super) struct DispatchMetrics {
    pub(super) span_ns: i64,
    pub(super) busy_ns: i64,
    pub(super) starved_ns: i64,
    /// The starvation gap before this dispatch (between it and the previous one).
    pub(super) idle_ns: i64,
    /// The wall time the draw thread spent recording the dispatch.
    pub(super) submit_ns: i64,
}

/// The rolling window behind the periodic log line and the UI readout. Totals are accumulated per
/// dispatch and normalized per frame on report: eye-0 dispatches count frames, since every stereo
/// frame runs exactly one.
pub(super) struct Stats {
    window_start: Instant,
    frames: u32,
    dispatches: u32,
    total: DispatchMetrics,
}

impl Stats {
    pub(super) fn new() -> Self {
        Self {
            window_start: Instant::now(),
            frames: 0,
            dispatches: 0,
            total: DispatchMetrics::default(),
        }
    }

    pub(super) fn record(&mut self, lane: DispatchLane, metrics: DispatchMetrics) {
        if matches!(lane, DispatchLane::Eye0) {
            self.frames += 1;
        }
        self.dispatches += 1;
        self.total.span_ns += metrics.span_ns;
        self.total.busy_ns += metrics.busy_ns;
        self.total.starved_ns += metrics.starved_ns;
        self.total.idle_ns += metrics.idle_ns;
        self.total.submit_ns += metrics.submit_ns;
    }

    /// Publishes and logs the window once it is full, then starts a fresh one.
    pub(super) fn log_if_due(&mut self) {
        if self.window_start.elapsed() < SUMMARY_WINDOW {
            return;
        }
        let frames = self.frames;
        if frames == 0 {
            *self = Self::new();
            return;
        }
        let per_frame = |ns: i64| ms(ns) as f32 / frames as f32;
        let summary = GpuSummary {
            frames,
            dispatches_per_frame: self.dispatches as f32 / frames as f32,
            busy_ms: per_frame(self.total.busy_ns),
            starved_ms: per_frame(self.total.starved_ns),
            idle_ms: per_frame(self.total.idle_ns),
            submit_ms: per_frame(self.total.submit_ns),
        };
        *SUMMARY.lock() = Some(summary);
        // Busy is an upper bound and starved a lower bound (see the module docs); the submit total
        // is what says whether the GPU is being fed or is genuinely working.
        tracing::info!(
            "profiler: GPU over {frames} frames ({:.1} dispatches/frame): busy <= {:.2} ms/frame, \
             starved >= {:.2} ms/frame, idle between dispatches {:.2} ms/frame, CPU submit {:.2} \
             ms/frame{}",
            summary.dispatches_per_frame,
            summary.busy_ms,
            summary.starved_ms,
            summary.idle_ms,
            summary.submit_ms,
            if pass_timestamps_enabled() {
                ""
            } else {
                " (per-pass subdivision off: busy is the whole span)"
            },
        );
        crate::profiler::blocks::log_summary();
        *self = Self::new();
    }
}

/// How often the GPU layer logs its rolling busy/starved/idle summary while collection is on.
const SUMMARY_WINDOW: Duration = Duration::from_secs(5);

pub(super) fn ms(ns: i64) -> f64 {
    ns as f64 / 1_000_000.0
}
