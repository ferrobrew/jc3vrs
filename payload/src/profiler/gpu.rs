//! A synthetic GPU timeline lane in puffin, built from the engine's own D3D11 timestamp queries.
//!
//! The render seams (`PreDraw` / `DrawGBuffer` / `Draw` / `DrawPosteffects` / `PostDraw`) each
//! bracket their work with a pair of timestamp queries on the immediate context, and the whole
//! dispatch is wrapped in a disjoint query that yields the GPU tick frequency. Because the draw
//! thread runs the seams once per dispatch — twice per frame in stereo, three times on far-field
//! share frames — each dispatch is tagged with its eye and reported as its own scope on the "GPU"
//! lane.
//!
//! GPU results lag the CPU by a few frames, so queries are read back lazily: a dispatch's queries
//! are polled each frame and only consumed once the GPU has resolved them. Timestamps are mapped
//! into puffin's CPU nanosecond timeline via a CPU reference captured at the dispatch's start, so
//! the GPU lane sits just after the matching CPU work in the flame graph (GPU trailing CPU is the
//! true relationship).
//!
//! Because a dispatch's GPU work reports against the *current* puffin frame a few frames after its
//! CPU submission, each puffin frame's range stretches back over the reporting latency (~2-3
//! frames). The offline Chrome trace is unaffected (absolute time), but the live flame graph's
//! frame bars read wider than the true frame time; treat the GPU lane's *durations*, not the frame
//! bars, as the signal.
//!
//! The lane also carries explicit **"GPU idle"** scopes: the measured gap between one dispatch's
//! last timestamp and the next one's first. The GPU executes dispatches serially, so these gaps
//! are true starvation bubbles (the GPU waiting while the CPU builds the next dispatch), and
//! their share of the frame is the direct measure of how much the serialized dispatch pipeline
//! costs. Comparing ticks across disjoint brackets is formally out of contract for D3D11, but
//! under DXVK timestamps are one monotonic Vulkan clock at a constant frequency, which the
//! frequency-match guard on the comparison also verifies; an implausible gap (negative, or over
//! [`MAX_CREDIBLE_IDLE_NS`]) is discarded rather than reported.
//!
//! All query use is serialized under the [`STATE`] mutex, and begin/end/seam/read-back all run on
//! whichever thread executes `HandleDrawThreadTask` — a CPU-fragment worker normally, or the main
//! thread inline on single-core setups; the two roles never run concurrently. The raw handles are
//! wrapped in [`Send`] assertions on that basis.

use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, Ordering},
};

use jc3gi::graphics_engine::graphics_engine::{
    self, FrequencyStatus, GraphicsEngine, HContext_t, HDevice_t, HTimeStampDisjointQuery_t,
    HTimeStampQuery_t,
};
use parking_lot::Mutex;
use puffin::{GlobalProfiler, ScopeDetails, ScopeId, StreamInfo, ThreadInfo};

/// The coarse render seams bracketed on the GPU timeline, in draw order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GpuSeam {
    PreDraw,
    GBuffer,
    Scene,
    PostEffects,
    PostDraw,
}

impl GpuSeam {
    fn name(self) -> &'static str {
        match self {
            GpuSeam::PreDraw => "PreDraw",
            GpuSeam::GBuffer => "DrawGBuffer",
            GpuSeam::Scene => "Draw (scene)",
            GpuSeam::PostEffects => "DrawPosteffects",
            GpuSeam::PostDraw => "PostDraw",
        }
    }
}

/// Whether the GPU layer should issue queries this frame (tied to puffin's scope switch).
pub fn enabled() -> bool {
    puffin::are_scopes_on()
}

/// Opens a GPU dispatch on `ctx`, tagged with the current eye. Begins the disjoint query and
/// captures the CPU reference time. A no-op while disabled or before the graphics device exists.
/// Lazily acquires the device from the graphics engine on first use.
///
/// # Safety
/// `ctx` must be the live immediate-context handle for this dispatch.
pub unsafe fn begin_dispatch(ctx: *mut HContext_t) {
    if !enabled() {
        return;
    }
    // SAFETY: reads the live graphics-engine singleton's device pointer on the draw thread.
    let device = unsafe {
        GraphicsEngine::get()
            .map(|ge| ge.m_Device)
            .filter(|d| !d.is_null())
    };
    let Some(device) = device else {
        return;
    };
    // The far dispatch of a share frame gets its own lane label; it reports eye 0 but is a
    // different workload (the G-buffer-only far-field render).
    let lane = if crate::stereo::far_phase() {
        DispatchLane::Far
    } else if crate::stereo::draw_index() == 0 {
        DispatchLane::Eye0
    } else {
        DispatchLane::Eye1
    };

    let mut guard = STATE.lock();
    let state = guard.get_or_insert_with(GpuProfiler::new);
    if state.device.0.is_null() {
        state.device = SendPtr(device.cast());
    }
    unsafe { state.begin_dispatch(ctx, lane) };
    HAS_WORK.store(true, Ordering::Relaxed);
}

/// Closes the current GPU dispatch on `ctx`: ends the disjoint query and queues the dispatch for
/// read-back. Also polls previously queued dispatches and reports any that the GPU has finished.
/// Runs even while collection is off so an in-flight dispatch or backlog is always drained; a
/// relaxed atomic keeps the fully idle case to a single load.
///
/// # Safety
/// `ctx` must be the same immediate-context handle passed to [`begin_dispatch`].
pub unsafe fn end_dispatch(ctx: *mut HContext_t) {
    if !HAS_WORK.load(Ordering::Relaxed) {
        return;
    }
    let mut guard = STATE.lock();
    let Some(state) = guard.as_mut() else {
        return;
    };
    unsafe { state.end_dispatch(ctx) };
    if state.current.is_none() && state.pending.is_empty() {
        HAS_WORK.store(false, Ordering::Relaxed);
    }
}

/// Whether any dispatch is open or awaiting read-back; the [`end_dispatch`] fast path.
static HAS_WORK: AtomicBool = AtomicBool::new(false);

/// Brackets a render seam with a GPU timestamp pair on `ctx`. The returned guard records the end
/// timestamp when dropped, so wrap the original seam call in its lifetime:
///
/// ```ignore
/// let _g = unsafe { gpu::seam(ctx, GpuSeam::GBuffer) };
/// original.call(this, ctx, a3, a4);
/// ```
///
/// Returns `None` while disabled, before the device is known, or outside a dispatch.
///
/// # Safety
/// `ctx` must be the live immediate-context handle for the enclosing dispatch.
pub unsafe fn seam(ctx: *mut HContext_t, seam: GpuSeam) -> Option<SeamGuard> {
    if !enabled() {
        return None;
    }
    let mut guard = STATE.lock();
    let state = guard.as_mut()?;
    let begin = unsafe { state.record_timestamp(ctx)? };
    Some(SeamGuard {
        ctx: SendPtr(ctx),
        seam,
        begin: SendPtr(begin),
    })
}

/// Records the end timestamp of a seam and files the interval into the current dispatch.
pub struct SeamGuard {
    ctx: SendPtr<HContext_t>,
    seam: GpuSeam,
    begin: SendPtr<HTimeStampQuery_t>,
}

impl Drop for SeamGuard {
    fn drop(&mut self) {
        let mut guard = STATE.lock();
        let Some(state) = guard.as_mut() else {
            return;
        };
        // SAFETY: `ctx` is the dispatch's live context; `begin` was allocated by this state.
        let Some(end) = (unsafe { state.record_timestamp(self.ctx.0) }) else {
            state.recycle_timestamp(self.begin.0);
            return;
        };
        state.push_interval(self.seam, self.begin.0, end);
    }
}

static STATE: Mutex<Option<GpuProfiler>> = Mutex::new(None);

/// A raw handle wrapper asserting [`Send`]. Sound because every handle is created, recorded, and
/// read back solely on the draw thread; the mutex is never locked from another thread with intent
/// to touch these pointers.
struct SendPtr<T>(*mut T);
unsafe impl<T> Send for SendPtr<T> {}

/// Dispatches of read-back backlog to keep polling before giving up and recycling the oldest
/// (about three frames' worth at two to three dispatches per frame).
const MAX_PENDING: usize = 8;

/// The largest inter-dispatch gap reported as a "GPU idle" scope. Anything longer is a pause
/// (collection toggled, a load, a hitch), not a pipeline bubble worth charting.
const MAX_CREDIBLE_IDLE_NS: i64 = 50_000_000;

struct Interval {
    seam: GpuSeam,
    begin: *mut HTimeStampQuery_t,
    end: *mut HTimeStampQuery_t,
}

struct Dispatch {
    lane: DispatchLane,
    disjoint: *mut HTimeStampDisjointQuery_t,
    cpu_ref_ns: i64,
    intervals: Vec<Interval>,
}

struct PrevDispatchEnd {
    ticks: u64,
    frequency: u64,
}

/// Which workload a dispatch renders, naming its outer scope on the GPU lane.
#[derive(Clone, Copy)]
enum DispatchLane {
    Eye0 = 0,
    Eye1 = 1,
    /// The far-field share frame's G-buffer-only far render (issue #32); reported separately from
    /// the eye-0 near render it shares a frame with.
    Far = 2,
}

struct GpuProfiler {
    device: SendPtr<HDevice_t>,
    // (fields below hold raw query handles in `Vec`s and `Dispatch`s; see the `Send` note below.)
    current: Option<Dispatch>,
    pending: VecDeque<Dispatch>,
    ts_pool: Vec<*mut HTimeStampQuery_t>,
    disjoint_pool: Vec<*mut HTimeStampDisjointQuery_t>,
    /// The end of the last span reported to the GPU lane. The GPU executes dispatches serially,
    /// so a dispatch whose CPU-anchored mapping would start before the previous one ended is
    /// shifted to follow it; without this, GPU-bound frames produce overlapping spans on the one
    /// lane, which the Chrome viewer renders as a garbled track.
    lane_cursor_ns: i64,
    /// The previous resolved dispatch's final GPU timestamp and its tick frequency, for the
    /// inter-dispatch idle measurement (see the module docs on cross-disjoint comparability).
    prev_end: Option<PrevDispatchEnd>,
    /// Cached puffin scope ids for the per-lane outer scopes (indexed by [`DispatchLane`]) and the
    /// per-seam inner scopes.
    lane_scopes: Vec<ScopeId>,
    seam_scopes: Vec<ScopeId>,
    /// The "GPU idle" scope, registered alongside the lane scopes.
    idle_scope: Option<ScopeId>,
}

// Sound for the same reason as [`SendPtr`]: all query use is serialized under [`STATE`], and every
// call site runs on whichever thread executes `HandleDrawThreadTask` (see the module docs).
unsafe impl Send for GpuProfiler {}

impl GpuProfiler {
    fn new() -> Self {
        Self {
            device: SendPtr(std::ptr::null_mut()),
            current: None,
            pending: VecDeque::new(),
            ts_pool: Vec::new(),
            disjoint_pool: Vec::new(),
            lane_cursor_ns: 0,
            prev_end: None,
            lane_scopes: Vec::new(),
            seam_scopes: Vec::new(),
            idle_scope: None,
        }
    }

    /// # Safety
    /// `ctx` is the dispatch's live immediate context.
    unsafe fn begin_dispatch(&mut self, ctx: *mut HContext_t, lane: DispatchLane) {
        if self.device.0.is_null() {
            return;
        }
        if self.current.is_some() {
            // The previous dispatch never reached its PostDraw (an engine early-out mid-draw); its
            // disjoint now spans two dispatches, so the sample is wrong once. Self-healing.
            tracing::warn!(
                "profiler: a GPU dispatch was still open at the next dispatch's begin; one sample \
                 will be misattributed"
            );
            return;
        }
        let Some(disjoint) = self.alloc_disjoint() else {
            return;
        };
        unsafe { graphics_engine::BeginTimeStampDisjointQuery(ctx, disjoint) };
        self.current = Some(Dispatch {
            lane,
            disjoint,
            cpu_ref_ns: puffin::now_ns(),
            intervals: Vec::new(),
        });
    }

    /// # Safety
    /// `ctx` is the dispatch's live immediate context.
    unsafe fn end_dispatch(&mut self, ctx: *mut HContext_t) {
        if let Some(dispatch) = self.current.take() {
            unsafe { graphics_engine::EndTimeStampDisjointQuery(ctx, dispatch.disjoint) };
            self.pending.push_back(dispatch);
        }
        // SAFETY: `ctx` is the live immediate context; read-back only reads the queries.
        unsafe { self.drain_pending(ctx) };
    }

    /// Allocates and records a timestamp query on `ctx`, returning its handle for pairing.
    ///
    /// # Safety
    /// `ctx` is a live immediate context.
    unsafe fn record_timestamp(&mut self, ctx: *mut HContext_t) -> Option<*mut HTimeStampQuery_t> {
        self.current.as_ref()?;
        let query = self.alloc_timestamp()?;
        unsafe { graphics_engine::SetTimeStampQuery(ctx, query) };
        Some(query)
    }

    fn push_interval(
        &mut self,
        seam: GpuSeam,
        begin: *mut HTimeStampQuery_t,
        end: *mut HTimeStampQuery_t,
    ) {
        if let Some(dispatch) = self.current.as_mut() {
            dispatch.intervals.push(Interval { seam, begin, end });
        } else {
            self.recycle_timestamp(begin);
            self.recycle_timestamp(end);
        }
    }

    /// Polls queued dispatches oldest-first; reports and recycles each one the GPU has resolved.
    /// Stops at the first still-pending dispatch to preserve ordering, and force-recycles anything
    /// beyond [`MAX_PENDING`] so a lost query can never leak the pool.
    ///
    /// # Safety
    /// `ctx` is a live immediate context for the read-back `GetData` calls.
    unsafe fn drain_pending(&mut self, ctx: *mut HContext_t) {
        while let Some(front) = self.pending.front() {
            let mut frequency = 0u64;
            let status = unsafe {
                graphics_engine::QueryTimeStampFrequency(ctx, front.disjoint, &mut frequency)
            };
            match status {
                FrequencyStatus::Pending if self.pending.len() <= MAX_PENDING => break,
                FrequencyStatus::Ok if frequency != 0 => {
                    let dispatch = self.pending.pop_front().unwrap();
                    unsafe { self.report_dispatch(ctx, dispatch, frequency) };
                }
                // Disjoint (unreliable), zero frequency, or an over-deep backlog: drop it.
                _ => {
                    let dispatch = self.pending.pop_front().unwrap();
                    self.recycle_dispatch(dispatch);
                }
            }
        }
    }

    /// Reads a resolved dispatch's timestamps and reports its intervals as a GPU-lane scope frame.
    ///
    /// # Safety
    /// `ctx` is a live immediate context for the timestamp `GetData` calls.
    unsafe fn report_dispatch(&mut self, ctx: *mut HContext_t, dispatch: Dispatch, frequency: u64) {
        let tick_to_ns =
            |ticks: u64| -> i64 { (ticks as i128 * 1_000_000_000 / frequency as i128) as i64 };

        // Resolve each interval to CPU-timeline nanoseconds relative to the first timestamp. The
        // seams are issued and executed in order, so the first interval's begin is the dispatch's
        // earliest tick; track the latest end tick for the idle measurement.
        let mut base_ticks: Option<u64> = None;
        let mut last_ticks: u64 = 0;
        let mut resolved: Vec<(GpuSeam, i64, i64)> = Vec::with_capacity(dispatch.intervals.len());
        for interval in &dispatch.intervals {
            let begin = unsafe { graphics_engine::QueryTimeStamp(ctx, interval.begin) };
            let end = unsafe { graphics_engine::QueryTimeStamp(ctx, interval.end) };
            if begin == 0 || end == 0 || end < begin {
                continue;
            }
            let base = *base_ticks.get_or_insert(begin);
            last_ticks = last_ticks.max(end);
            let start_ns = dispatch.cpu_ref_ns + tick_to_ns(begin.saturating_sub(base));
            let stop_ns = dispatch.cpu_ref_ns + tick_to_ns(end.saturating_sub(base));
            resolved.push((interval.seam, start_ns, stop_ns));
        }

        if let Some(first_ticks) = base_ticks {
            // The gap since the previous dispatch's last timestamp is true GPU starvation (the GPU
            // runs dispatches serially). Only comparable while the tick frequency is unchanged,
            // and only credible below the pause threshold.
            let idle_ns = self
                .prev_end
                .as_ref()
                .filter(|prev| prev.frequency == frequency && first_ticks > prev.ticks)
                .map(|prev| tick_to_ns(first_ticks - prev.ticks))
                .filter(|&gap| gap > 0 && gap < MAX_CREDIBLE_IDLE_NS);
            self.prev_end = Some(PrevDispatchEnd {
                ticks: last_ticks,
                frequency,
            });

            // The GPU executes dispatches serially: place this dispatch no earlier than the
            // previous span's end plus the measured idle gap, so the lane reconstructs the true
            // busy/idle alternation (and never overlaps; see `lane_cursor_ns`).
            let outer_start = resolved.iter().map(|&(_, s, _)| s).min().unwrap();
            let shift = (self.lane_cursor_ns + idle_ns.unwrap_or(0) - outer_start).max(0);
            for (_, start_ns, stop_ns) in &mut resolved {
                *start_ns += shift;
                *stop_ns += shift;
            }
            self.lane_cursor_ns = resolved.iter().map(|&(_, _, e)| e).max().unwrap();

            self.ensure_scopes();
            let lane_scope = self.lane_scopes[dispatch.lane as usize];
            let idle =
                idle_ns.map(|gap| (self.idle_scope.expect("registered with the lanes"), gap));
            report_gpu_frame(lane_scope, &self.seam_scopes, idle, &resolved);
        }

        self.recycle_dispatch(dispatch);
    }

    fn recycle_dispatch(&mut self, dispatch: Dispatch) {
        self.disjoint_pool.push(dispatch.disjoint);
        for interval in dispatch.intervals {
            self.ts_pool.push(interval.begin);
            self.ts_pool.push(interval.end);
        }
    }

    fn recycle_timestamp(&mut self, query: *mut HTimeStampQuery_t) {
        self.ts_pool.push(query);
    }

    fn alloc_timestamp(&mut self) -> Option<*mut HTimeStampQuery_t> {
        if let Some(query) = self.ts_pool.pop() {
            return Some(query);
        }
        let query = unsafe { graphics_engine::CreateTimeStampQuery(self.device.0) };
        (!query.is_null()).then_some(query)
    }

    fn alloc_disjoint(&mut self) -> Option<*mut HTimeStampDisjointQuery_t> {
        if let Some(query) = self.disjoint_pool.pop() {
            return Some(query);
        }
        let query = unsafe { graphics_engine::CreateTimeStampDisjointQuery(self.device.0) };
        (!query.is_null()).then_some(query)
    }

    /// Registers the per-lane and per-seam puffin scopes once.
    fn ensure_scopes(&mut self) {
        if !self.lane_scopes.is_empty() {
            return;
        }
        let mut profiler = GlobalProfiler::lock();
        // Indexed by `DispatchLane as usize`.
        self.lane_scopes = profiler.register_user_scopes(&[
            ScopeDetails::from_scope_name("GPU eye 0"),
            ScopeDetails::from_scope_name("GPU eye 1"),
            ScopeDetails::from_scope_name("GPU far field"),
        ]);
        self.idle_scope = profiler
            .register_user_scopes(&[ScopeDetails::from_scope_name("GPU idle")])
            .first()
            .copied();
        let seams = [
            GpuSeam::PreDraw,
            GpuSeam::GBuffer,
            GpuSeam::Scene,
            GpuSeam::PostEffects,
            GpuSeam::PostDraw,
        ];
        let details: Vec<ScopeDetails> = seams
            .iter()
            .map(|s| ScopeDetails::from_scope_name(s.name()))
            .collect();
        self.seam_scopes = profiler.register_user_scopes(&details);
    }
}

/// The index of a [`GpuSeam`] into the registered `seam_scopes` list (draw order).
fn seam_index(seam: GpuSeam) -> usize {
    match seam {
        GpuSeam::PreDraw => 0,
        GpuSeam::GBuffer => 1,
        GpuSeam::Scene => 2,
        GpuSeam::PostEffects => 3,
        GpuSeam::PostDraw => 4,
    }
}

/// Builds a single-thread puffin stream for the "GPU" lane: an optional "GPU idle" scope covering
/// the measured starvation gap since the previous dispatch, then an outer per-lane scope spanning
/// the dispatch, with one inner scope per resolved seam; reports it into the current puffin frame.
/// `resolved` must be non-empty.
fn report_gpu_frame(
    lane_scope: ScopeId,
    seam_scopes: &[ScopeId],
    idle: Option<(ScopeId, i64)>,
    resolved: &[(GpuSeam, i64, i64)],
) {
    let outer_start = resolved.iter().map(|&(_, s, _)| s).min().unwrap();
    let outer_stop = resolved.iter().map(|&(_, _, e)| e).max().unwrap();

    let mut stream = puffin::Stream::default();
    if let Some((idle_scope, gap_ns)) = idle {
        let (off, _) = stream.begin_scope(|| outer_start - gap_ns, idle_scope, "");
        stream.end_scope(off, outer_start);
    }
    let (outer_off, _) = stream.begin_scope(|| outer_start, lane_scope, "");
    for &(seam, start_ns, stop_ns) in resolved {
        let scope_id = seam_scopes[seam_index(seam)];
        let (off, _) = stream.begin_scope(|| start_ns, scope_id, "");
        stream.end_scope(off, stop_ns);
    }
    stream.end_scope(outer_off, outer_stop);

    let Ok(stream_info) = StreamInfo::parse(stream) else {
        return;
    };
    // A fixed `ThreadInfo` keys every dispatch onto the one "GPU" lane; a varying key (e.g. the
    // dispatch's start time) would give puffin a fresh lane per dispatch and splinter the flame
    // graph. `Some(0)` also gives the lane a stable sort position.
    GlobalProfiler::lock().report_user_scopes(
        ThreadInfo {
            start_time_ns: Some(0),
            name: "GPU".to_owned(),
        },
        &stream_info.as_stream_into_ref(),
    );
}
