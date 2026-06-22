//! Render-call tracing subsystem: collects per-call NDJSON records over a fixed number of frames and
//! dumps them next to the injected DLL, with a self-describing manifest as the first record.

use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use parking_lot::Mutex;

/// Frames left in the active trace; 0 = inactive. Kept as a standalone atomic so the hot-path
/// [`tracing_active`] gate is lock-free -- the heavier state lives in [`TRACE_STATE`]. Driven by the
/// "Dump render trace" button.
static TRACE_FRAMES: AtomicI32 = AtomicI32::new(0);

/// The in-progress trace's collected-but-unwritten state.
pub struct TraceState {
    /// NDJSON records collected so far.
    log: Vec<String>,
    /// Local-time stamp (YYYYMMDD-HHMMSS) for this trace's filename, set at [`TraceState::start`].
    stamp: String,
    /// Absolute path of the most recent dump, shown in the UI so it's findable.
    last_path: Option<String>,
    /// Monotonic origin set at [`TraceState::start`]; each record's `time_ns` is measured from it.
    started: Option<std::time::Instant>,
    /// Real-frame index within the current trace (0-based; bumped each [`TraceState::end_frame`]).
    frame: u32,
}

/// One dumped trace record: a [`TraceEvent`] plus when/where it happened. Round-trips through serde,
/// so a dumped trace reads back into `Vec<TraceRecord>` for analysis.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TraceRecord {
    /// Nanoseconds since the trace started (monotonic), for ordering + intra-frame timing.
    pub time_ns: u64,
    /// Real-frame index within the trace (0-based).
    pub frame: u32,
    /// The eye being drawn, for per-dispatch events; omitted for frame/driver markers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eye: Option<usize>,
    /// The event itself.
    pub event: TraceEvent,
}
static TRACE_STATE: Mutex<TraceState> = Mutex::new(TraceState::new());

/// GPU draw-call counts for one eye's Draw. The live global counter lives in `hooks::draw_count`;
/// this same type is the [`TraceEvent::DrawEnd`] field, with serde de/serializing the atomics as
/// their plain values.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct DrawCounts {
    pub draw: AtomicUsize,
    pub draw_indexed: AtomicUsize,
    pub dispatch: AtomicUsize,
}
impl DrawCounts {
    pub const fn new() -> Self {
        Self {
            draw: AtomicUsize::new(0),
            draw_indexed: AtomicUsize::new(0),
            dispatch: AtomicUsize::new(0),
        }
    }

    /// Reset all three to zero (at each eye's `draw_begin`).
    pub fn clear(&self) {
        self.draw.store(0, Ordering::Relaxed);
        self.draw_indexed.store(0, Ordering::Relaxed);
        self.dispatch.store(0, Ordering::Relaxed);
    }

    /// A frozen copy of the current values for the `draw_end` event (the live `static` can't move).
    pub fn snapshot(&self) -> Self {
        Self {
            draw: AtomicUsize::new(self.draw.load(Ordering::Relaxed)),
            draw_indexed: AtomicUsize::new(self.draw_indexed.load(Ordering::Relaxed)),
            dispatch: AtomicUsize::new(self.dispatch.load(Ordering::Relaxed)),
        }
    }
}

/// One render event, serialized inside a [`TraceRecord`] (which stamps timing / frame / eye). The
/// `ev` tag names the event. Frame/driver markers that carry their own `eye` field set it explicitly.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "ev")]
pub enum TraceEvent {
    /// First record of every trace: a snapshot of the active runtime toggles, so a capture is
    /// self-describing (which gates / skips / the exposure pin were on) without external notes.
    #[serde(rename = "manifest")]
    Manifest {
        /// Local capture time (YYYYMMDD-HHMMSS), matching the trace filename.
        timestamp: String,
        /// The full runtime configuration snapshot (nests cleanly via its own `Serialize`).
        config: crate::config::Config,
    },
    #[serde(rename = "frame_begin")]
    FrameBegin {
        stereo: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        present_eye: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restore_counters: Option<bool>,
    },
    #[serde(rename = "draw_begin")]
    DrawBegin { eye: usize },
    #[serde(rename = "draw_end")]
    DrawEnd { eye: usize, counts: DrawCounts },
    /// Between-eyes per-pass add-list reset; `cleared` = passes whose draw-time list still had
    /// items from eye 0 (so a non-zero count here is the doubling being prevented).
    #[serde(rename = "ResetPerEye")]
    ResetPerEye { cleared: u32 },
    #[serde(rename = "SetupRenderCamera")]
    SetupRenderCamera,
    #[serde(rename = "RotateRenderFrameData")]
    RotateRenderFrameData { gated: bool },
    #[serde(rename = "SetupRenderFrameData")]
    SetupRenderFrameData { gated: bool },
    #[serde(rename = "HandBackBuffers")]
    HandBackBuffers { gated: bool },
    #[serde(rename = "SmoothedExposureUpdate")]
    SmoothedExposureUpdate { gated: bool, exposure: f32 },
    #[serde(rename = "CalcHistogramMidBright")]
    CalcHistogramMidBright { gated: bool },
    /// Per-frame exposure internals, read from inside `ToneMappingEffect::Update` (the canonical
    /// exposure write, with the live effect in hand). `divisor` is `m_Histogram2`'s mid-point -- the
    /// value the converged exposure actually tracks (`target = target_num / divisor`). The stereo
    /// darkening should show up here as `divisor` (hence `target`/`exposure`) differing from
    /// non-stereo, isolating whether it's the second histogram's metering or its ping-pong readback.
    #[serde(rename = "ExposureInternals")]
    ExposureInternals {
        exposure: f32,
        target_num: f32,
        divisor: f32,
        target: f32,
        hist1_bright: f32,
        hist1_mid: f32,
        hist1_buckets: Vec<u32>,
        hist2_bright: f32,
        hist2_mid: f32,
        hist2_buckets: Vec<u32>,
        num_buckets: u32,
        /// The metering ping-pong selector (`this[168]`), to spot a buffer-swap mismatch under stereo.
        pingpong: u32,
        forced: bool,
    },
    #[serde(rename = "GenerateHistogram")]
    GenerateHistogram { skip: bool },
    #[serde(rename = "DrawHistogramWindow")]
    DrawHistogramWindow { skip: bool },
    #[serde(rename = "ApplyWorldFilters")]
    ApplyWorldFilters { gated: bool },
    #[serde(rename = "ApplyGlobalFilters")]
    ApplyGlobalFilters { gated: bool },
    #[serde(rename = "SsaoDraw")]
    SsaoDraw { temporal_disabled: bool },
    #[serde(rename = "DoF::Apply")]
    DofApply { input: u32, skip: bool },
    #[serde(rename = "MotionBlur::Apply")]
    MotionBlurApply { input: u32, skip: bool },
    #[serde(rename = "Glare::Apply")]
    GlareApply { skip: bool },
    #[serde(rename = "Fade::Apply")]
    FadeApply { skip: bool },
    #[serde(rename = "PlayerDamage::Apply")]
    PlayerDamageApply { input: u32, skip: bool },
    #[serde(rename = "SunHalo::PreApply")]
    SunHaloPreApply { skip: bool },
    #[serde(rename = "SunHalo::Apply")]
    SunHaloApply { skip: bool },
    #[serde(rename = "PostDraw")]
    PostDraw,
    #[serde(rename = "Flip")]
    Flip { blocked: bool },
    // Buffer-flow events (raw pointers as u64 so render-setup / texture instances can be compared
    // across eyes -- same pointer = same target, different pointer = a swapped instance).
    #[serde(rename = "SetRenderSetup")]
    SetRenderSetup {
        setup: u64,
        /// Draws/dispatches issued into the *previous* target since the last bind (this thread).
        draws: usize,
        indexed: usize,
        dispatch: usize,
    },
    #[serde(rename = "Clear")]
    Clear { color: [f32; 4] },
    #[serde(rename = "CopySurfaceToTexture")]
    CopySurfaceToTexture { dst: u64, src: u64 },
    #[serde(rename = "ResolveSurface")]
    ResolveSurface,
}

/// Whether a render trace is currently collecting. Lock-free; lets hooks skip readback work when off.
pub fn tracing_active() -> bool {
    TRACE_FRAMES.load(Ordering::Relaxed) > 0
}

/// Frames left in the active trace (0 = inactive), for the debug UI's countdown.
pub fn active_frames() -> i32 {
    TRACE_FRAMES.load(Ordering::Relaxed)
}

impl Default for TraceState {
    /// Delegates to the const [`TraceState::new`] -- the `TRACE_STATE` static needs a const
    /// initializer, which a derived `Default` can't provide, so `new` stays the single source.
    fn default() -> Self {
        Self::new()
    }
}

impl TraceState {
    const fn new() -> Self {
        Self {
            log: Vec::new(),
            stamp: String::new(),
            last_path: None,
            started: None,
            frame: 0,
        }
    }

    /// Build a [`TraceRecord`] (stamping the current time + frame) and append it (caller holds the
    /// lock).
    fn push_event(&mut self, eye: Option<usize>, event: TraceEvent) {
        let record = TraceRecord {
            time_ns: self
                .started
                .map(|s| s.elapsed().as_nanos() as u64)
                .unwrap_or(0),
            frame: self.frame,
            eye,
            event,
        };
        if let Ok(s) = serde_json::to_string(&record) {
            self.log.push(s);
        }
    }

    /// Write the collected log to an NDJSON file next to the injected DLL (same place as `jc3vrs.log`)
    /// and record its path for the UI (caller holds the lock).
    fn dump(&mut self) {
        let name = if self.stamp.is_empty() {
            "jc3vrs_render_trace.ndjson".to_string()
        } else {
            format!("jc3vrs_render_trace_{}.ndjson", self.stamp)
        };
        let path = crate::module::get_path()
            .and_then(|p| p.parent().map(|dir| dir.join(&name)))
            .unwrap_or_else(|| std::path::PathBuf::from(&name));
        match std::fs::write(&path, self.log.join("\n")) {
            Ok(()) => {
                let shown = path.display().to_string();
                tracing::info!(
                    "Render trace dumped: {} records -> {}",
                    self.log.len(),
                    shown
                );
                self.last_path = Some(shown);
            }
            Err(e) => tracing::error!("Failed to write render trace: {e}"),
        }
    }

    /// Append a frame/driver marker record (no dispatch eye) while a trace is active (auto-locks).
    pub fn record(event: TraceEvent) {
        if tracing_active() {
            TRACE_STATE.lock().push_event(None, event);
        }
    }

    /// Append a per-dispatch record tagged with the current eye while a trace is active (auto-locks).
    pub fn record_eye(event: TraceEvent) {
        if tracing_active() {
            let eye = crate::stereo::draw_index();
            TRACE_STATE.lock().push_event(Some(eye), event);
        }
    }

    /// Begin a render-call trace covering the next `frames` real frames (auto-locks). The manifest is
    /// written as the first record while the counter is still 0 (lock held), then the counter is
    /// armed -- so render-thread hooks that observe it can never race ahead of the manifest.
    pub fn start(frames: i32) {
        let stamp = jiff::Zoned::now().strftime("%Y%m%d-%H%M%S").to_string();
        {
            let mut state = TRACE_STATE.lock();
            let last_path = state.last_path.take();
            *state = TraceState {
                last_path,
                stamp: stamp.clone(),
                started: Some(std::time::Instant::now()),
                ..Default::default()
            };
            state.push_event(None, build_manifest(stamp));
        }
        TRACE_FRAMES.store(frames, Ordering::Relaxed);
        tracing::info!("Render trace started ({frames} frames)");
    }

    /// Tick one real frame (called by the Draw driver); on the final frame, dump the NDJSON
    /// (auto-locks).
    pub fn end_frame() {
        if TRACE_FRAMES.load(Ordering::Relaxed) <= 0 {
            return;
        }
        let finished = TRACE_FRAMES.fetch_sub(1, Ordering::Relaxed) - 1 <= 0;
        let mut state = TRACE_STATE.lock();
        state.frame += 1;
        if finished {
            state.dump();
        }
    }

    /// The most recent dumped trace file's path, for the UI (auto-locks).
    pub fn last_path() -> Option<String> {
        TRACE_STATE.lock().last_path.clone()
    }
}

/// Snapshot the active runtime config into a [`TraceEvent::Manifest`] (the first record of a trace).
fn build_manifest(timestamp: String) -> TraceEvent {
    TraceEvent::Manifest {
        timestamp,
        config: crate::config::get(),
    }
}
