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
//! [`queries::MAX_CREDIBLE_IDLE_NS`]) is discarded rather than reported.
//!
//! All query use is serialized under the [`STATE`] mutex, and begin/end/seam/read-back all run on
//! whichever thread executes `HandleDrawThreadTask` — a CPU-fragment worker normally, or the main
//! thread inline on single-core setups; the two roles never run concurrently. The raw handles are
//! wrapped in [`Send`] assertions on that basis.
//!
//! The layer is split by concern: [`queries`] owns the query pools and the dispatch read-back,
//! [`gaps`] subdivides a resolved dispatch into work and holes, [`lane`] reports the result to
//! puffin, and [`stats`] keeps the rolling summary. This module holds the entry points the hooks
//! call and the state they share.

mod gaps;
mod lane;
mod queries;
mod stats;

use std::sync::atomic::{AtomicBool, Ordering};

use jc3gi::graphics_engine::graphics_engine::{GraphicsEngine, HContext_t, HTimeStampQuery_t};
use parking_lot::Mutex;
use puffin::ScopeId;

use crate::profiler::gpu::queries::{DispatchLane, GpuProfiler, IntervalLabel, SendPtr};

pub use crate::profiler::gpu::stats::summary;

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
    // [`teardown`] runs from the cleanups, which fire while these hooks are still installed and the
    // game thread keeps ticking for a few more frames. Without this the next dispatch would rebuild
    // the state and allocate a fresh pool that nothing is left to destroy -- reintroducing, in that
    // window, the exact leak the teardown exists to close.
    if crate::is_shutting_down() {
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

/// Destroys every query the profiler owns — the pools, anything pending read-back, and the
/// in-flight dispatch, if any — through the device the pools were allocated from, and drops the
/// state. A subsequent [`begin_dispatch`] finds `STATE` empty and builds a fresh `GpuProfiler`,
/// so the profiler works normally across an eject/reinject cycle rather than reusing handles that
/// died with the device.
///
/// Registered with [`crate::lifecycle::on_cleanup`]; see `profiler::ensure_gpu_cleanup_registered`
/// for why that call site runs exactly once.
pub fn teardown() {
    let mut guard = STATE.lock();
    let Some(state) = guard.take() else {
        return;
    };
    state.destroy();
    HAS_WORK.store(false, Ordering::Relaxed);
}

static STATE: Mutex<Option<GpuProfiler>> = Mutex::new(None);

/// The per-dispatch cap on pass intervals, bounding the timestamp-query pool (each interval is two
/// queries, and up to [`queries::MAX_PENDING`] dispatches are in flight). Comfortably above the
/// ~100 render passes a dispatch draws; a dispatch that somehow exceeds it loses its tail passes'
/// subdivision, never its seams'.
const MAX_PASS_INTERVALS: u32 = 256;
