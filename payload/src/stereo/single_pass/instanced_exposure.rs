//! The already-instanced draw exposure: how much geometry reaches the eye-split as an instanced draw
//! the collapse cannot simply double, and which shaders it belongs to.
//!
//! Split out of the parent module because it is measurement rather than mechanism: nothing here
//! changes what is drawn. It is the largest single resident of that module and the one whose absence
//! makes the rest legible.

use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
};

use parking_lot::Mutex;

use crate::stereo::single_pass::{
    BOUND_VS, DIAGNOSTIC_FRAME_CADENCE, PATCHED_VS_NAMES, VIEWPORT_UNIFIED, diagnostic_frame,
};

/// Tally one already-instanced draw the eye-parity case applies to, and attribute it to the bound
/// vertex shader. `handled` distinguishes a per-eye re-issue from a draw left exposed.
pub(super) fn record_instanced_case(instance_count: u32, handled: bool) {
    if handled {
        INSTANCED_HANDLED.fetch_add(1, Ordering::Relaxed);
        INSTANCED_HANDLED_INSTANCES.fetch_add(instance_count as usize, Ordering::Relaxed);
    } else {
        INSTANCED_AFFECTED.fetch_add(1, Ordering::Relaxed);
        // A 1-instance draw has no odd instance at all, so it lands in the left eye and is simply
        // missing from the right; a multi-instance draw is split alternately, so each eye gets half the
        // batch. The two look different on screen and are worth separating.
        if instance_count <= 1 {
            INSTANCED_AFFECTED_SINGLE.fetch_add(1, Ordering::Relaxed);
        } else {
            INSTANCED_AFFECTED_MULTI.fetch_add(1, Ordering::Relaxed);
        }
        INSTANCED_AFFECTED_INSTANCES.fetch_add(instance_count as usize, Ordering::Relaxed);
    }
    INSTANCED_MAX_INSTANCES.fetch_max(instance_count, Ordering::Relaxed);
    attribute_instanced_draw(true, instance_count);
}

/// Tally one already-instanced draw the per-eye re-issue does **not** apply to, split by why: whether a
/// patched vertex shader was bound (the only shaders that read `SV_InstanceID` as an eye parity) and
/// whether the render thread was inside the G-buffer range (the only place the eye-half viewport pair
/// is bound).
///
/// The out-of-range patched bucket is the one that matters: those draws route their odd-parity
/// instances through viewport slot 1 in a pass that was never eye-split, so they depend entirely on
/// slot 1 being a valid duplicate of slot 0 (see [`unify_viewport_slots`]).
pub(super) fn record_instanced_bystander(patched: bool, in_range: bool, instance_count: u32) {
    let (draws, instances) = match (patched, in_range) {
        (true, false) => (
            &INSTANCED_OUT_OF_RANGE_PATCHED,
            &INSTANCED_OUT_OF_RANGE_PATCHED_INSTANCES,
        ),
        (false, true) => (
            &INSTANCED_IN_RANGE_UNPATCHED,
            &INSTANCED_IN_RANGE_UNPATCHED_INSTANCES,
        ),
        (false, false) => (
            &INSTANCED_OUT_OF_RANGE_UNPATCHED,
            &INSTANCED_OUT_OF_RANGE_UNPATCHED_INSTANCES,
        ),
        // A patched, in-range draw only reaches here with the collapse off, where nothing is eye-split
        // and the parity is harmless. Counted with the other in-range work rather than silently.
        (true, true) => (
            &INSTANCED_IN_RANGE_UNPATCHED,
            &INSTANCED_IN_RANGE_UNPATCHED_INSTANCES,
        ),
    };
    draws.fetch_add(1, Ordering::Relaxed);
    instances.fetch_add(instance_count as usize, Ordering::Relaxed);
    if patched {
        attribute_instanced_draw(in_range, instance_count);
    }
}

/// Attribute one already-instanced draw with a patched vertex shader bound to that shader, splitting
/// in-range from out-of-range so the per-shader table says whether the geometry families losing
/// instances outside the G-buffer range are the same ones the in-range re-issue covers.
///
/// Only patched shaders are attributed: the eye parity is theirs alone. The attribution itself only
/// runs on a [`diagnostic_frame`] -- every such draw pays [`diagnostic_frame`]'s relaxed load, but the
/// map and its mutex are reached only on the sampled frame, so the per-shader table is a sample of the
/// diagnostic cadence rather than an exhaustive tally (unlike the exposure counters in
/// [`record_instanced_case`] and [`record_instanced_bystander`], which are plain atomics and stay
/// exhaustive).
pub(super) fn attribute_instanced_draw(in_range: bool, instance_count: u32) {
    if !diagnostic_frame() {
        return;
    }
    INSTANCED_OFFENDERS
        .lock()
        .entry(BOUND_VS.load(Ordering::Relaxed))
        .or_default()
        .accumulate(in_range, instance_count);
}

/// One vertex shader's cumulative share of the already-instanced draws that a patched shader issued,
/// split by whether the draw was inside the G-buffer range.
#[derive(Clone, Copy, Default)]
struct InstancedOffender {
    draws: u64,
    instances: u64,
    out_of_range_draws: u64,
    out_of_range_instances: u64,
}

impl InstancedOffender {
    fn accumulate(&mut self, in_range: bool, instance_count: u32) {
        let (draws, instances) = if in_range {
            (&mut self.draws, &mut self.instances)
        } else {
            (
                &mut self.out_of_range_draws,
                &mut self.out_of_range_instances,
            )
        };
        *draws += 1;
        *instances += u64::from(instance_count);
    }
}

/// The already-instanced draws issued with a patched vertex shader bound, attributed to that shader
/// and keyed by its `ID3D11VertexShader` pointer. Cumulative over the [`diagnostic_frame`]s sampled
/// (see [`attribute_instanced_draw`]), and cleared alongside [`PATCHED_VS`] (whose released pointers
/// can be recycled).
static INSTANCED_OFFENDERS: Mutex<BTreeMap<usize, InstancedOffender>> = Mutex::new(BTreeMap::new());

/// One frame's already-instanced draw exposure. See [`draw_indexed_instanced_detour`].
#[derive(Clone, Copy, Default, Debug, serde::Serialize)]
pub struct InstancedExposure {
    /// Every `DrawIndexedInstanced` the game issued, anywhere in the frame.
    pub total: u32,
    /// Those the eye-parity case applies to (a patched VS, in the G-buffer range, collapsed) that were
    /// re-issued once per eye, so the parity no longer decides which eye they land in.
    pub handled: u32,
    /// The instances summed over the handled draws.
    pub handled_instances: u64,
    /// Those the case applies to that were **not** re-issued -- the flag is off, or the re-issue could
    /// not run -- and so are still split between the eyes by their instance parity.
    pub affected: u32,
    /// Affected draws with a single instance -- rendered into the left eye only.
    pub affected_single_instance: u32,
    /// Affected draws with more than one instance -- the batch split alternately between the eyes.
    pub affected_multi_instance: u32,
    /// The instances summed over the affected draws: how much geometry the split actually moves.
    pub affected_instances: u64,
    /// The largest instance count seen on a draw the case applies to, handled or not.
    pub max_instances: u32,
    /// Draws with a patched vertex shader bound **outside** the G-buffer range (the shadow, reflection
    /// and post passes). Their `SV_InstanceID & 1` still writes `SV_ViewportArrayIndex`, but the pass
    /// binds no eye-half pair, so they depend on viewport slot 1 duplicating slot 0.
    pub out_of_range_patched: u32,
    pub out_of_range_patched_instances: u64,
    /// Draws with an unpatched vertex shader inside the range: no viewport index is written, so they
    /// rasterise to slot 0 -- the left eye's half while the split is bound.
    pub in_range_unpatched: u32,
    pub in_range_unpatched_instances: u64,
    /// Draws with an unpatched vertex shader outside the range: unaffected by any of this, and the
    /// remainder that makes the four buckets sum to [`total`](Self::total).
    pub out_of_range_unpatched: u32,
    pub out_of_range_unpatched_instances: u64,
}

/// The already-instanced draw exposure, as the debug UI and the diagnostic log report it: the most
/// recent frame plus a mean over the frames since the counters were last reset.
#[derive(Clone, Copy, Default, Debug, serde::Serialize)]
pub struct InstancedExposureReport {
    pub last_frame: InstancedExposure,
    /// Frames accumulated into the means (only frames that entered the single-pass geometry range).
    pub frames: u32,
    pub mean_total: f32,
    pub mean_handled: f32,
    pub mean_handled_instances: f32,
    pub mean_affected: f32,
    pub mean_affected_single_instance: f32,
    pub mean_affected_multi_instance: f32,
    pub mean_affected_instances: f32,
    pub mean_out_of_range_patched: f32,
    pub mean_out_of_range_patched_instances: f32,
    /// The largest instance count seen on an affected draw across all accumulated frames.
    pub peak_instances: u32,
}

/// The running exposure accumulator behind [`InstancedExposureReport`].
#[derive(Default)]
struct ExposureHistory {
    last: InstancedExposure,
    frames: u32,
    total: u64,
    handled: u64,
    handled_instances: u64,
    affected: u64,
    affected_single: u64,
    affected_multi: u64,
    affected_instances: u64,
    out_of_range_patched: u64,
    out_of_range_patched_instances: u64,
    peak_instances: u32,
}

impl ExposureHistory {
    fn push(&mut self, frame: InstancedExposure) {
        self.last = frame;
        self.frames += 1;
        self.total += u64::from(frame.total);
        self.handled += u64::from(frame.handled);
        self.handled_instances += frame.handled_instances;
        self.affected += u64::from(frame.affected);
        self.affected_single += u64::from(frame.affected_single_instance);
        self.affected_multi += u64::from(frame.affected_multi_instance);
        self.affected_instances += frame.affected_instances;
        self.out_of_range_patched += u64::from(frame.out_of_range_patched);
        self.out_of_range_patched_instances += frame.out_of_range_patched_instances;
        self.peak_instances = self.peak_instances.max(frame.max_instances);
    }

    fn report(&self) -> InstancedExposureReport {
        let mean = |sum: u64| {
            if self.frames == 0 {
                0.0
            } else {
                sum as f32 / self.frames as f32
            }
        };
        InstancedExposureReport {
            last_frame: self.last,
            frames: self.frames,
            mean_total: mean(self.total),
            mean_handled: mean(self.handled),
            mean_handled_instances: mean(self.handled_instances),
            mean_affected: mean(self.affected),
            mean_affected_single_instance: mean(self.affected_single),
            mean_affected_multi_instance: mean(self.affected_multi),
            mean_affected_instances: mean(self.affected_instances),
            mean_out_of_range_patched: mean(self.out_of_range_patched),
            mean_out_of_range_patched_instances: mean(self.out_of_range_patched_instances),
            peak_instances: self.peak_instances,
        }
    }
}

static INSTANCED_HISTORY: Mutex<ExposureHistory> = Mutex::new(ExposureHistory {
    last: InstancedExposure {
        total: 0,
        handled: 0,
        handled_instances: 0,
        affected: 0,
        affected_single_instance: 0,
        affected_multi_instance: 0,
        affected_instances: 0,
        max_instances: 0,
        out_of_range_patched: 0,
        out_of_range_patched_instances: 0,
        in_range_unpatched: 0,
        in_range_unpatched_instances: 0,
        out_of_range_unpatched: 0,
        out_of_range_unpatched_instances: 0,
    },
    frames: 0,
    total: 0,
    handled: 0,
    handled_instances: 0,
    affected: 0,
    affected_single: 0,
    affected_multi: 0,
    affected_instances: 0,
    out_of_range_patched: 0,
    out_of_range_patched_instances: 0,
    peak_instances: 0,
});

/// Fold the frame's exposure counters into the history and clear them. Called once per frame from
/// [`log_draw_split`], at the end of the G-buffer range -- so the out-of-range buckets carry the
/// passes that ran *before* the range this frame (shadow, reflection) together with the ones that ran
/// *after* it last frame (scene tail, post, UI). In steady state the totals are the frame's; a
/// single frame's split is only approximately aligned with it.
pub(super) fn accumulate_instanced_exposure() -> InstancedExposure {
    let frame = InstancedExposure {
        total: INSTANCED_TOTAL.swap(0, Ordering::Relaxed) as u32,
        handled: INSTANCED_HANDLED.swap(0, Ordering::Relaxed) as u32,
        handled_instances: INSTANCED_HANDLED_INSTANCES.swap(0, Ordering::Relaxed) as u64,
        affected: INSTANCED_AFFECTED.swap(0, Ordering::Relaxed) as u32,
        affected_single_instance: INSTANCED_AFFECTED_SINGLE.swap(0, Ordering::Relaxed) as u32,
        affected_multi_instance: INSTANCED_AFFECTED_MULTI.swap(0, Ordering::Relaxed) as u32,
        affected_instances: INSTANCED_AFFECTED_INSTANCES.swap(0, Ordering::Relaxed) as u64,
        max_instances: INSTANCED_MAX_INSTANCES.swap(0, Ordering::Relaxed),
        out_of_range_patched: INSTANCED_OUT_OF_RANGE_PATCHED.swap(0, Ordering::Relaxed) as u32,
        out_of_range_patched_instances: INSTANCED_OUT_OF_RANGE_PATCHED_INSTANCES
            .swap(0, Ordering::Relaxed) as u64,
        in_range_unpatched: INSTANCED_IN_RANGE_UNPATCHED.swap(0, Ordering::Relaxed) as u32,
        in_range_unpatched_instances: INSTANCED_IN_RANGE_UNPATCHED_INSTANCES
            .swap(0, Ordering::Relaxed) as u64,
        out_of_range_unpatched: INSTANCED_OUT_OF_RANGE_UNPATCHED.swap(0, Ordering::Relaxed) as u32,
        out_of_range_unpatched_instances: INSTANCED_OUT_OF_RANGE_UNPATCHED_INSTANCES
            .swap(0, Ordering::Relaxed) as u64,
    };
    INSTANCED_HISTORY.lock().push(frame);
    frame
}

/// The already-instanced draw exposure so far this session. See [`InstancedExposureReport`].
pub fn instanced_exposure() -> InstancedExposureReport {
    INSTANCED_HISTORY.lock().report()
}

/// Clear the accumulated exposure history and per-shader attribution, so the reported numbers cover
/// one clean pass over the shader set (called from [`reset_patched_vs`], on a shader reload).
pub(super) fn reset_instanced_exposure() {
    *INSTANCED_HISTORY.lock() = ExposureHistory::default();
    INSTANCED_OFFENDERS.lock().clear();
}

/// One entry of [`instanced_offenders`]: a vertex shader and the already-instanced draws of its that
/// the eye-parity case applies to, whether they were re-issued per eye or left exposed. Sampled on
/// [`diagnostic_frame`]s only (see [`attribute_instanced_draw`]), so the counts are proportional to,
/// not equal to, the shader's actual share of the exhaustive totals in [`InstancedExposure`].
#[derive(Clone, Debug, serde::Serialize)]
pub struct InstancedOffenderReport {
    /// The shader's engine name (`CreateVertexProgramParams.m_Name`), or `None` when the shader was
    /// created through the re-acquire path, which carries no name.
    pub name: Option<String>,
    /// The `ID3D11VertexShader` pointer -- the only identity an unnamed shader has.
    pub shader: usize,
    /// Draws inside the G-buffer range on a sampled frame: the ones the per-eye re-issue handles.
    pub draws: u64,
    pub instances: u64,
    /// Draws outside it on a sampled frame: the shadow, reflection and post passes, where nothing
    /// eye-splits and the parity must be neutralised by the viewport slots being identical instead.
    pub out_of_range_draws: u64,
    pub out_of_range_instances: u64,
}

/// The vertex shaders responsible for the already-instanced draws the eye-parity case applies to, the
/// busiest first, capped at `limit`. Which shaders these are is what says how much of the extra
/// submission cost each family is carrying, and which of them a per-block re-issue (bark, foliage,
/// occluder) already covers. Built from [`INSTANCED_OFFENDERS`], which only accumulates on
/// [`diagnostic_frame`]s, so this is a sample of the diagnostic cadence, not an exhaustive tally --
/// treat the ranking as indicative and the raw counts as proportional rather than absolute.
pub fn instanced_offenders(limit: usize) -> Vec<InstancedOffenderReport> {
    let names = PATCHED_VS_NAMES.lock();
    let mut offenders: Vec<InstancedOffenderReport> = INSTANCED_OFFENDERS
        .lock()
        .iter()
        .map(|(&shader, tally)| InstancedOffenderReport {
            name: names.get(&shader).cloned(),
            shader,
            draws: tally.draws,
            instances: tally.instances,
            out_of_range_draws: tally.out_of_range_draws,
            out_of_range_instances: tally.out_of_range_instances,
        })
        .collect();
    // Ranked by the shader's whole instanced load, so a family that only draws outside the range --
    // the case the split was added to expose -- cannot be ranked off the end of the list.
    let load = |o: &InstancedOffenderReport| o.draws + o.out_of_range_draws;
    let weight = |o: &InstancedOffenderReport| o.instances + o.out_of_range_instances;
    offenders.sort_by(|a, b| load(b).cmp(&load(a)).then(weight(b).cmp(&weight(a))));
    offenders.truncate(limit);
    offenders
}

/// Log the already-instanced draw handling, at the same 120-frame cadence as the draw split. Emitted
/// only once there is something to report, so a session where the case never arises stays quiet.
pub(super) fn log_instanced_exposure(frame: InstancedExposure) {
    if frame.handled == 0 && frame.affected == 0 && frame.out_of_range_patched == 0 {
        return;
    }
    let report = instanced_exposure();
    let offenders: Vec<String> = instanced_offenders(6)
        .into_iter()
        .map(|o| {
            let name = o
                .name
                .unwrap_or_else(|| format!("<unnamed {:#x}>", o.shader));
            format!(
                "{name} (in {} draws/{} inst, out {} draws/{} inst)",
                o.draws, o.instances, o.out_of_range_draws, o.out_of_range_instances
            )
        })
        .collect();
    tracing::info!(
        target: "single_pass",
        "instanced eye-parity: {} re-issued per eye ({} instances) and {} still exposed ({} \
         single-instance, {} multi-instance, {} instances) of {} DrawIndexedInstanced this frame, max \
         {} instances | mean over {} frames: {:.1} handled + {:.1} exposed of {:.1}, {:.1} handled \
         instances, peak {} | top (sampled every {}th frame, not the exhaustive total above): {}",
        frame.handled,
        frame.handled_instances,
        frame.affected,
        frame.affected_single_instance,
        frame.affected_multi_instance,
        frame.affected_instances,
        frame.total,
        frame.max_instances,
        report.frames,
        report.mean_handled,
        report.mean_affected,
        report.mean_total,
        report.mean_handled_instances,
        report.peak_instances,
        DIAGNOSTIC_FRAME_CADENCE,
        if offenders.is_empty() { "-".to_string() } else { offenders.join(", ") },
    );
    tracing::info!(
        target: "single_pass",
        "instanced by range: in-range {} patched ({} handled + {} exposed) + {} unpatched ({} \
         instances) | out-of-range {} patched ({} instances) + {} unpatched ({} instances) | mean \
         out-of-range patched over {} frames: {:.1} draws, {:.1} instances | slot-1 repairs: {}",
        frame.handled + frame.affected,
        frame.handled,
        frame.affected,
        frame.in_range_unpatched,
        frame.in_range_unpatched_instances,
        frame.out_of_range_patched,
        frame.out_of_range_patched_instances,
        frame.out_of_range_unpatched,
        frame.out_of_range_unpatched_instances,
        report.frames,
        report.mean_out_of_range_patched,
        report.mean_out_of_range_patched_instances,
        VIEWPORT_UNIFIED.swap(0, Ordering::Relaxed),
    );
}

pub(super) static INSTANCED_TOTAL: AtomicUsize = AtomicUsize::new(0);
static INSTANCED_HANDLED: AtomicUsize = AtomicUsize::new(0);
static INSTANCED_HANDLED_INSTANCES: AtomicUsize = AtomicUsize::new(0);
static INSTANCED_AFFECTED: AtomicUsize = AtomicUsize::new(0);
static INSTANCED_AFFECTED_SINGLE: AtomicUsize = AtomicUsize::new(0);
static INSTANCED_AFFECTED_MULTI: AtomicUsize = AtomicUsize::new(0);
static INSTANCED_AFFECTED_INSTANCES: AtomicUsize = AtomicUsize::new(0);
static INSTANCED_MAX_INSTANCES: AtomicU32 = AtomicU32::new(0);
static INSTANCED_OUT_OF_RANGE_PATCHED: AtomicUsize = AtomicUsize::new(0);
static INSTANCED_OUT_OF_RANGE_PATCHED_INSTANCES: AtomicUsize = AtomicUsize::new(0);
static INSTANCED_IN_RANGE_UNPATCHED: AtomicUsize = AtomicUsize::new(0);
static INSTANCED_IN_RANGE_UNPATCHED_INSTANCES: AtomicUsize = AtomicUsize::new(0);
static INSTANCED_OUT_OF_RANGE_UNPATCHED: AtomicUsize = AtomicUsize::new(0);
static INSTANCED_OUT_OF_RANGE_UNPATCHED_INSTANCES: AtomicUsize = AtomicUsize::new(0);
