//! The timestamp-query ring: the query pools, the in-flight dispatch, the read-back backlog, and
//! the resolution of a finished dispatch's ticks into CPU-timeline scopes.

use std::collections::VecDeque;

use jc3gi::graphics_engine::graphics_engine::{
    self, FrequencyStatus, HContext_t, HDevice_t, HTimeStampDisjointQuery_t, HTimeStampQuery_t,
};
use puffin::{GlobalProfiler, ScopeDetails, ScopeId};

use crate::profiler::gpu::{
    GpuSeam,
    gaps::{Resolved, starvation_gaps},
    lane::{GpuScope, report_gpu_frame},
    stats::{DispatchMetrics, Stats, ms},
};

/// A raw handle wrapper asserting [`Send`]. Sound because every handle is created, recorded, and
/// read back solely on the draw thread; the mutex is never locked from another thread with intent
/// to touch these pointers.
pub(super) struct SendPtr<T>(pub(super) *mut T);
unsafe impl<T> Send for SendPtr<T> {}

/// Dispatches of read-back backlog to keep polling before giving up and recycling the oldest
/// (about three frames' worth at two to three dispatches per frame).
pub(super) const MAX_PENDING: usize = 8;

/// The largest inter-dispatch gap reported as a "GPU idle" scope. Anything longer is a pause
/// (collection toggled, a load, a hitch), not a pipeline bubble worth charting.
pub(super) const MAX_CREDIBLE_IDLE_NS: i64 = 50_000_000;

/// What a GPU interval measures: one of the coarse render seams, or one render pass nested inside
/// a seam.
#[derive(Clone, Copy)]
pub(super) enum IntervalLabel {
    Seam(GpuSeam),
    Pass(ScopeId),
}

pub(super) struct Dispatch {
    lane: DispatchLane,
    /// The immediate context the dispatch was opened on; pass intervals record against it.
    pub(super) ctx: SendPtr<HContext_t>,
    disjoint: *mut HTimeStampDisjointQuery_t,
    /// CPU nanoseconds at the dispatch's begin and end: their difference is the wall time the draw
    /// thread spent recording the dispatch (the submit span).
    cpu_ref_ns: i64,
    cpu_end_ns: i64,
    intervals: Vec<Interval>,
    pub(super) pass_intervals: u32,
}

/// Which workload a dispatch renders, naming its outer scope on the GPU lane.
#[derive(Clone, Copy)]
pub(super) enum DispatchLane {
    Eye0 = 0,
    Eye1 = 1,
    /// The far-field share frame's G-buffer-only far render (issue #32); reported separately from
    /// the eye-0 near render it shares a frame with.
    Far = 2,
}

pub(super) struct GpuProfiler {
    pub(super) device: SendPtr<HDevice_t>,
    // (fields below hold raw query handles in `Vec`s and `Dispatch`s; see the `Send` note below.)
    pub(super) current: Option<Dispatch>,
    pub(super) pending: VecDeque<Dispatch>,
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

// Sound for the same reason as [`SendPtr`]: all query use is serialized under [`crate::profiler::gpu::STATE`],
// and every call site runs on whichever thread executes `HandleDrawThreadTask` (see the module docs).
unsafe impl Send for GpuProfiler {}

impl GpuProfiler {
    pub(super) fn new() -> Self {
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
    pub(super) unsafe fn begin_dispatch(&mut self, ctx: *mut HContext_t, lane: DispatchLane) {
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
    pub(super) unsafe fn end_dispatch(&mut self, ctx: *mut HContext_t) {
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
    pub(super) unsafe fn record_timestamp(
        &mut self,
        ctx: *mut HContext_t,
    ) -> Option<*mut HTimeStampQuery_t> {
        self.current.as_ref()?;
        let query = self.alloc_timestamp()?;
        unsafe { graphics_engine::SetTimeStampQuery(ctx, query) };
        Some(query)
    }

    pub(super) fn push_interval(
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

    pub(super) fn recycle_timestamp(&mut self, query: *mut HTimeStampQuery_t) {
        self.ts_pool.push(query);
    }

    /// Destroys every query this profiler owns — the pools, anything pending read-back, and the
    /// in-flight dispatch, if any — through the device the pools were allocated from. Consumes the
    /// profiler; see [`crate::profiler::gpu::teardown`] for why it is torn down wholesale.
    pub(super) fn destroy(mut self) {
        if let Some(dispatch) = self.current.take() {
            self.recycle_dispatch(dispatch);
        }
        while let Some(dispatch) = self.pending.pop_front() {
            self.recycle_dispatch(dispatch);
        }
        if !self.device.0.is_null() {
            // SAFETY: `self.device` is the device the pooled queries were created from (see
            // `alloc_timestamp`/`alloc_disjoint`), and no other thread touches these handles (see
            // the module docs' `Send` note).
            unsafe {
                for query in self.ts_pool.drain(..) {
                    graphics_engine::DestroyTimeStampQuery(self.device.0, query);
                }
                for disjoint in self.disjoint_pool.drain(..) {
                    graphics_engine::DestroyTimeStampDisjointQuery(self.device.0, disjoint);
                }
            }
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

struct Interval {
    label: IntervalLabel,
    begin: *mut HTimeStampQuery_t,
    end: *mut HTimeStampQuery_t,
}

struct PrevDispatchEnd {
    ticks: u64,
    frequency: u64,
}

/// The shortest intra-dispatch hole reported as its own "GPU starved" scope. Shorter holes are
/// still counted in the starved total; charting them all would bury the lane in slivers.
const MIN_REPORTED_GAP_NS: i64 = 50_000;

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
