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
//! Inside a dispatch, each render pass ([`pass_interval`], driven from the `RenderPass::DoDraw`
//! hook) gets its own timestamp pair nested in its seam. That subdivision is what makes the lane
//! readable as work rather than as a span: a seam's outer bracket covers the GPU's *whole*
//! execution window for that seam, idle included, so a starved dispatch and a shading-bound one
//! look identical at seam granularity. Summing the pass intervals instead gives a **busy** figure,
//! and the holes between them are reported as explicit **"GPU starved"** scopes.
//!
//! Both figures are bounds, not exact: starvation finer than one pass (the GPU draining between
//! individual draws because the draw thread cannot record them fast enough) is invisible to any
//! affordable timestamp granularity, and lands inside "busy". So **busy is an upper bound on real
//! GPU work and starved is a lower bound on real GPU idle**. The honest discriminator to read
//! alongside them is the per-dispatch CPU submit span (the wall time the draw thread spent
//! recording the dispatch, reported as `submit` in the lane's scope data and the periodic log):
//! a GPU span that tracks the submit span dispatch after dispatch is the submission-bound
//! signature, whereas a GPU span that outruns it is real shading cost.
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
    time::{Duration, Instant},
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

/// Whether the per-pass GPU subdivision is on. It is the only thing that separates GPU work from
/// GPU starvation inside a dispatch, so it defaults on; the toggle exists to A/B its own cost
/// (two timestamp queries per render pass per dispatch).
pub fn pass_timestamps_enabled() -> bool {
    PASS_TIMESTAMPS.load(Ordering::Relaxed)
}

pub fn set_pass_timestamps_enabled(enabled: bool) {
    PASS_TIMESTAMPS.store(enabled, Ordering::Relaxed);
}

static PASS_TIMESTAMPS: AtomicBool = AtomicBool::new(true);

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
pub unsafe fn seam(ctx: *mut HContext_t, seam: GpuSeam) -> Option<IntervalGuard> {
    if !enabled() {
        return None;
    }
    let mut guard = STATE.lock();
    let state = guard.as_mut()?;
    let begin = unsafe { state.record_timestamp(ctx)? };
    Some(IntervalGuard {
        ctx: SendPtr(ctx),
        label: IntervalLabel::Seam(seam),
        begin: SendPtr(begin),
    })
}

/// Brackets one render pass with a GPU timestamp pair, nested inside the seam that is drawing it.
/// `scope` names the pass on the lane (the caller already resolved it for the CPU scope).
///
/// Unlike [`seam`], this takes no context: it records on the immediate context the enclosing
/// dispatch was opened with, which is the same handle the seams run on. Returns `None` while
/// disabled, outside a dispatch, or once the dispatch's pass budget
/// ([`MAX_PASS_INTERVALS`]) is spent.
pub fn pass_interval(scope: ScopeId) -> Option<IntervalGuard> {
    if !enabled() || !pass_timestamps_enabled() {
        return None;
    }
    let mut guard = STATE.lock();
    let state = guard.as_mut()?;
    let ctx = state.current.as_ref()?.ctx.0;
    if state.current.as_ref()?.pass_intervals >= MAX_PASS_INTERVALS {
        return None;
    }
    // SAFETY: a dispatch is open, so the context it was opened with is still live (the dispatch
    // is closed on the same thread, inside the same `HandleDrawThreadTask`).
    let begin = unsafe { state.record_timestamp(ctx)? };
    if let Some(dispatch) = state.current.as_mut() {
        dispatch.pass_intervals += 1;
    }
    Some(IntervalGuard {
        ctx: SendPtr(ctx),
        label: IntervalLabel::Pass(scope),
        begin: SendPtr(begin),
    })
}

/// Records the end timestamp of a seam or pass and files the interval into the current dispatch.
pub struct IntervalGuard {
    ctx: SendPtr<HContext_t>,
    label: IntervalLabel,
    begin: SendPtr<HTimeStampQuery_t>,
}

impl Drop for IntervalGuard {
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
        state.push_interval(self.label, self.begin.0, end);
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

/// The per-dispatch cap on pass intervals, bounding the timestamp-query pool (each interval is two
/// queries, and up to [`MAX_PENDING`] dispatches are in flight). Comfortably above the ~100 render
/// passes a dispatch draws; a dispatch that somehow exceeds it loses its tail passes' subdivision,
/// never its seams'.
const MAX_PASS_INTERVALS: u32 = 256;

/// The shortest intra-dispatch hole reported as its own "GPU starved" scope. Shorter holes are
/// still counted in the starved total; charting them all would bury the lane in slivers.
const MIN_REPORTED_GAP_NS: i64 = 50_000;

/// How often the GPU layer logs its rolling busy/starved/idle summary while collection is on.
const SUMMARY_WINDOW: Duration = Duration::from_secs(5);

/// What a GPU interval measures: one of the coarse render seams, or one render pass nested inside
/// a seam.
#[derive(Clone, Copy)]
enum IntervalLabel {
    Seam(GpuSeam),
    Pass(ScopeId),
}

struct Interval {
    label: IntervalLabel,
    begin: *mut HTimeStampQuery_t,
    end: *mut HTimeStampQuery_t,
}

struct Dispatch {
    lane: DispatchLane,
    /// The immediate context the dispatch was opened on; pass intervals record against it.
    ctx: SendPtr<HContext_t>,
    disjoint: *mut HTimeStampDisjointQuery_t,
    /// CPU nanoseconds at the dispatch's begin and end: their difference is the wall time the draw
    /// thread spent recording the dispatch (the submit span).
    cpu_ref_ns: i64,
    cpu_end_ns: i64,
    intervals: Vec<Interval>,
    pass_intervals: u32,
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
    /// The "GPU idle" (between dispatches) and "GPU starved" (inside a dispatch) scopes,
    /// registered alongside the lane scopes.
    idle_scope: Option<ScopeId>,
    starved_scope: Option<ScopeId>,
    /// The rolling busy/starved/idle accumulator behind the periodic log line and the UI readout.
    stats: Stats,
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
            starved_scope: None,
            stats: Stats::new(),
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
            ctx: SendPtr(ctx),
            disjoint,
            cpu_ref_ns: puffin::now_ns(),
            cpu_end_ns: 0,
            intervals: Vec::new(),
            pass_intervals: 0,
        });
    }

    /// # Safety
    /// `ctx` is the dispatch's live immediate context.
    unsafe fn end_dispatch(&mut self, ctx: *mut HContext_t) {
        if let Some(mut dispatch) = self.current.take() {
            dispatch.cpu_end_ns = puffin::now_ns();
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
        label: IntervalLabel,
        begin: *mut HTimeStampQuery_t,
        end: *mut HTimeStampQuery_t,
    ) {
        if let Some(dispatch) = self.current.as_mut() {
            dispatch.intervals.push(Interval { label, begin, end });
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
        let mut resolved: Vec<Resolved> = Vec::with_capacity(dispatch.intervals.len());
        for interval in &dispatch.intervals {
            let begin = unsafe { graphics_engine::QueryTimeStamp(ctx, interval.begin) };
            let end = unsafe { graphics_engine::QueryTimeStamp(ctx, interval.end) };
            if begin == 0 || end == 0 || end < begin {
                continue;
            }
            // The intervals are filed in completion order, so an early `begin` can arrive after a
            // later one; take the minimum tick seen so far as the base and rebase the earlier
            // entries when it moves.
            let base = match base_ticks {
                Some(base) if base <= begin => base,
                previous => {
                    if let Some(previous) = previous {
                        let shift = tick_to_ns(previous - begin);
                        for r in &mut resolved {
                            r.start_ns += shift;
                            r.stop_ns += shift;
                        }
                    }
                    base_ticks = Some(begin);
                    begin
                }
            };
            last_ticks = last_ticks.max(end);
            let start_ns = dispatch.cpu_ref_ns + tick_to_ns(begin.saturating_sub(base));
            let stop_ns = dispatch.cpu_ref_ns + tick_to_ns(end.saturating_sub(base));
            resolved.push(Resolved {
                label: interval.label,
                start_ns,
                stop_ns,
            });
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
            let outer_start = resolved.iter().map(|r| r.start_ns).min().unwrap();
            let shift = (self.lane_cursor_ns + idle_ns.unwrap_or(0) - outer_start).max(0);
            for r in &mut resolved {
                r.start_ns += shift;
                r.stop_ns += shift;
            }
            let outer_start = outer_start + shift;
            let outer_stop = resolved.iter().map(|r| r.stop_ns).max().unwrap();
            self.lane_cursor_ns = outer_stop;

            // Subdivide the span into work and holes, then accumulate the rolling summary.
            let gaps = starvation_gaps(&resolved, outer_start, outer_stop);
            let starved_ns: i64 = gaps.iter().map(|&(a, b)| b - a).sum();
            let span_ns = outer_stop - outer_start;
            let submit_ns = (dispatch.cpu_end_ns - dispatch.cpu_ref_ns).max(0);
            self.stats.record(
                dispatch.lane,
                DispatchMetrics {
                    span_ns,
                    busy_ns: span_ns - starved_ns,
                    starved_ns,
                    idle_ns: idle_ns.unwrap_or(0),
                    submit_ns,
                },
            );
            self.stats.log_if_due();

            self.ensure_scopes();
            let mut scopes = Vec::with_capacity(resolved.len() + gaps.len() + 2);
            if let Some(gap_ns) = idle_ns {
                scopes.push(GpuScope {
                    id: self.idle_scope.expect("registered with the lanes"),
                    start_ns: outer_start - gap_ns,
                    stop_ns: outer_start,
                    data: String::new(),
                });
            }
            scopes.push(GpuScope {
                id: self.lane_scopes[dispatch.lane as usize],
                start_ns: outer_start,
                stop_ns: outer_stop,
                data: format!(
                    "busy {:.2} ms / starved {:.2} ms of {:.2} ms; CPU submit {:.2} ms",
                    ms(span_ns - starved_ns),
                    ms(starved_ns),
                    ms(span_ns),
                    ms(submit_ns),
                ),
            });
            let starved_scope = self.starved_scope.expect("registered with the lanes");
            scopes.extend(
                gaps.iter()
                    .filter(|&&(a, b)| b - a >= MIN_REPORTED_GAP_NS)
                    .map(|&(start_ns, stop_ns)| GpuScope {
                        id: starved_scope,
                        start_ns,
                        stop_ns,
                        data: String::new(),
                    }),
            );
            scopes.extend(resolved.iter().map(|r| GpuScope {
                id: match r.label {
                    IntervalLabel::Seam(seam) => self.seam_scopes[seam_index(seam)],
                    IntervalLabel::Pass(scope) => scope,
                },
                start_ns: r.start_ns,
                stop_ns: r.stop_ns,
                data: String::new(),
            }));
            report_gpu_frame(scopes);
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
        let holes = profiler.register_user_scopes(&[
            ScopeDetails::from_scope_name("GPU idle"),
            ScopeDetails::from_scope_name("GPU starved"),
        ]);
        self.idle_scope = holes.first().copied();
        self.starved_scope = holes.get(1).copied();
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

/// One GPU interval mapped onto the CPU timeline.
struct Resolved {
    label: IntervalLabel,
    start_ns: i64,
    stop_ns: i64,
}

/// The holes in a dispatch's GPU timeline: the parts of `[start_ns, stop_ns]` covered by no
/// interval at the finest granularity available, in ascending order and disjoint.
///
/// Resolution is two-level. Between the seams, a hole is time the GPU spent outside any seam.
/// Inside a seam, the pass intervals nested in it resolve the seam's own span; a seam with no pass
/// intervals (subdivision off, or a seam that draws no passes) is treated as solid work, since
/// nothing finer was measured. Holes are therefore a *lower* bound on GPU idle: starvation between
/// individual draws inside one pass is not visible here.
fn starvation_gaps(resolved: &[Resolved], start_ns: i64, stop_ns: i64) -> Vec<(i64, i64)> {
    let mut seams: Vec<(i64, i64)> = resolved
        .iter()
        .filter(|r| matches!(r.label, IntervalLabel::Seam(_)))
        .map(|r| (r.start_ns, r.stop_ns))
        .collect();
    seams.sort_unstable();
    let mut passes: Vec<(i64, i64)> = resolved
        .iter()
        .filter(|r| matches!(r.label, IntervalLabel::Pass(_)))
        .map(|r| (r.start_ns, r.stop_ns))
        .collect();
    passes.sort_unstable();

    let mut gaps = Vec::new();
    let mut push = |from: i64, to: i64| {
        if to > from {
            gaps.push((from, to));
        }
    };
    let mut cursor = start_ns;
    for &(seam_start, seam_stop) in &seams {
        push(cursor, seam_start);
        cursor = cursor.max(seam_stop);
        // The passes this seam drew, in order; anything outside every seam is ignored (a pass is
        // always recorded inside the seam that draws it).
        let nested = passes
            .iter()
            .copied()
            .filter(|&(a, b)| a >= seam_start && b <= seam_stop);
        let mut inner = seam_start;
        let mut any = false;
        for (pass_start, pass_stop) in nested {
            any = true;
            push(inner, pass_start);
            inner = inner.max(pass_stop);
        }
        if any {
            push(inner, seam_stop);
        }
    }
    push(cursor, stop_ns);
    gaps
}

/// One dispatch's decomposition, as accumulated into the rolling summary.
#[derive(Clone, Copy, Default)]
struct DispatchMetrics {
    span_ns: i64,
    busy_ns: i64,
    starved_ns: i64,
    /// The starvation gap before this dispatch (between it and the previous one).
    idle_ns: i64,
    /// The wall time the draw thread spent recording the dispatch.
    submit_ns: i64,
}

/// The rolling window behind the periodic log line and the UI readout. Totals are accumulated per
/// dispatch and normalized per frame on report: eye-0 dispatches count frames, since every stereo
/// frame runs exactly one.
struct Stats {
    window_start: Instant,
    frames: u32,
    dispatches: u32,
    total: DispatchMetrics,
}

/// The last completed summary window, for the UI readout.
static SUMMARY: Mutex<Option<GpuSummary>> = Mutex::new(None);

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

impl Stats {
    fn new() -> Self {
        Self {
            window_start: Instant::now(),
            frames: 0,
            dispatches: 0,
            total: DispatchMetrics::default(),
        }
    }

    fn record(&mut self, lane: DispatchLane, metrics: DispatchMetrics) {
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
    fn log_if_due(&mut self) {
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
        *self = Self::new();
    }
}

fn ms(ns: i64) -> f64 {
    ns as f64 / 1_000_000.0
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

/// A scope to place on the "GPU" lane, in CPU-timeline nanoseconds.
struct GpuScope {
    id: ScopeId,
    start_ns: i64,
    stop_ns: i64,
    data: String,
}

/// Builds a single-thread puffin stream for the "GPU" lane out of `scopes` and reports it into the
/// current puffin frame. The scopes are an arbitrary set of ranges — the per-dispatch outer scope,
/// its seams, the passes nested in those, and the measured idle/starvation holes — so the nesting
/// is reconstructed here by containment: sorted by start (longest first on a tie), a stack yields
/// exactly the tree puffin's strictly-LIFO stream format needs. A child whose end overshoots its
/// parent's (equal ticks at a boundary) is clamped to it.
fn report_gpu_frame(scopes: Vec<GpuScope>) {
    let Ok(stream_info) = StreamInfo::parse(build_stream(scopes)) else {
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

/// The nesting reconstruction behind [`report_gpu_frame`], split out so it can be tested.
fn build_stream(mut scopes: Vec<GpuScope>) -> puffin::Stream {
    scopes.sort_by(|a, b| {
        a.start_ns
            .cmp(&b.start_ns)
            .then_with(|| b.stop_ns.cmp(&a.stop_ns))
    });

    let mut stream = puffin::Stream::default();
    let mut open: Vec<(usize, i64)> = Vec::new();
    for scope in &scopes {
        while let Some(&(offset, stop_ns)) = open.last() {
            if scope.start_ns >= stop_ns {
                stream.end_scope(offset, stop_ns);
                open.pop();
            } else {
                break;
            }
        }
        let stop_ns = open
            .last()
            .map_or(scope.stop_ns, |&(_, parent_stop)| {
                scope.stop_ns.min(parent_stop)
            })
            .max(scope.start_ns);
        let (offset, _) = stream.begin_scope(|| scope.start_ns, scope.id, &scope.data);
        open.push((offset, stop_ns));
    }
    while let Some((offset, stop_ns)) = open.pop() {
        stream.end_scope(offset, stop_ns);
    }
    stream
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    fn seam(start_ns: i64, stop_ns: i64) -> Resolved {
        Resolved {
            label: IntervalLabel::Seam(GpuSeam::GBuffer),
            start_ns,
            stop_ns,
        }
    }

    fn pass(start_ns: i64, stop_ns: i64) -> Resolved {
        Resolved {
            label: IntervalLabel::Pass(ScopeId(NonZeroU32::new(1).unwrap())),
            start_ns,
            stop_ns,
        }
    }

    #[test]
    fn seams_without_passes_are_solid_work() {
        // Nothing finer was measured inside the seams, so only the holes between them count.
        let resolved = [seam(0, 40), seam(50, 90)];
        assert_eq!(
            starvation_gaps(&resolved, 0, 100),
            vec![(40, 50), (90, 100)]
        );
    }

    #[test]
    fn passes_resolve_their_seam_into_work_and_holes() {
        let resolved = [seam(0, 100), pass(10, 20), pass(60, 70)];
        assert_eq!(
            starvation_gaps(&resolved, 0, 100),
            vec![(0, 10), (20, 60), (70, 100)]
        );
    }

    #[test]
    fn a_fully_covered_dispatch_has_no_holes() {
        let resolved = [seam(0, 100), pass(0, 50), pass(50, 100)];
        assert!(starvation_gaps(&resolved, 0, 100).is_empty());
    }

    #[test]
    fn passes_are_attributed_to_their_own_seam() {
        // A pass nested in the second seam must not be read as filling the first.
        let resolved = [seam(0, 40), seam(40, 100), pass(50, 90)];
        assert_eq!(
            starvation_gaps(&resolved, 0, 100),
            vec![(40, 50), (90, 100)]
        );
    }

    #[test]
    fn the_stream_nests_by_containment() {
        // The scopes arrive unordered and at three depths; the stream must still parse, which is
        // puffin's check that every scope is closed in LIFO order inside its parent.
        let id = ScopeId(NonZeroU32::new(1).unwrap());
        let scope = |start_ns: i64, stop_ns: i64| GpuScope {
            id,
            start_ns,
            stop_ns,
            data: String::new(),
        };
        let scopes = vec![
            scope(10, 20),
            scope(0, 100),
            scope(60, 70),
            scope(-5, 0),
            scope(0, 50),
            scope(50, 100),
        ];
        let info = StreamInfo::parse(build_stream(scopes)).expect("a well-nested stream");
        assert_eq!(info.num_scopes, 6);
        assert_eq!(info.range_ns, (-5, 100));
    }

    #[test]
    fn a_child_overshooting_its_parent_is_clamped() {
        let id = ScopeId(NonZeroU32::new(1).unwrap());
        let scopes = vec![
            GpuScope {
                id,
                start_ns: 0,
                stop_ns: 100,
                data: String::new(),
            },
            GpuScope {
                id,
                start_ns: 90,
                stop_ns: 130,
                data: String::new(),
            },
        ];
        let info = StreamInfo::parse(build_stream(scopes)).expect("a well-nested stream");
        assert_eq!(info.range_ns, (0, 100));
    }
}
