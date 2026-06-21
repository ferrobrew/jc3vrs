//! Render-call tracing subsystem: collects per-call NDJSON records over a fixed number of frames and
//! dumps them next to the injected DLL, with a self-describing manifest as the first record.

use std::sync::atomic::{AtomicI32, Ordering};

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
}
static TRACE_STATE: Mutex<TraceState> = Mutex::new(TraceState::new());

/// One render-trace record, serialized to NDJSON; the `ev` tag names the event. Pipeline-hook
/// variants omit `eye` -- it's injected by [`TraceState::record_eye`]; the markers carry it directly.
#[derive(serde::Serialize)]
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
        #[serde(skip_serializing_if = "Option::is_none")]
        present_eye: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        restore_counters: Option<bool>,
    },
    #[serde(rename = "draw_begin")]
    DrawBegin { eye: usize },
    #[serde(rename = "draw_end")]
    DrawEnd {
        eye: usize,
        draw: usize,
        draw_indexed: usize,
        dispatch: usize,
    },
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

impl TraceState {
    const fn new() -> Self {
        Self {
            log: Vec::new(),
            stamp: String::new(),
            last_path: None,
        }
    }

    /// Append one already-serialized record (caller holds the lock).
    fn push(&mut self, record: String) {
        self.log.push(record);
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

    /// Append a frame/eye marker record (carries its own `eye`) while a trace is active (auto-locks).
    pub fn record(event: TraceEvent) {
        if tracing_active()
            && let Ok(s) = serde_json::to_string(&event)
        {
            TRACE_STATE.lock().push(s);
        }
    }

    /// Append a per-dispatch record, injecting the current eye, while a trace is active (auto-locks).
    pub fn record_eye(event: TraceEvent) {
        if tracing_active()
            && let Ok(serde_json::Value::Object(mut map)) = serde_json::to_value(&event)
        {
            map.insert("eye".to_string(), crate::stereo::draw_index().into());
            TRACE_STATE
                .lock()
                .push(serde_json::Value::Object(map).to_string());
        }
    }

    /// Begin a render-call trace covering the next `frames` real frames (auto-locks). The manifest is
    /// written as the first record while the counter is still 0 (lock held), then the counter is
    /// armed -- so render-thread hooks that observe it can never race ahead of the manifest.
    pub fn start(frames: i32) {
        let stamp = jiff::Zoned::now().strftime("%Y%m%d-%H%M%S").to_string();
        {
            let mut state = TRACE_STATE.lock();
            state.log.clear();
            state.stamp = stamp.clone();
            if let Ok(s) = serde_json::to_string(&build_manifest(stamp)) {
                state.push(s);
            }
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
        if TRACE_FRAMES.fetch_sub(1, Ordering::Relaxed) - 1 <= 0 {
            TRACE_STATE.lock().dump();
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
