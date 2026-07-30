//! Subdivision of a dispatch's resolved intervals into GPU work and the holes between it.

use crate::profiler::gpu::queries::IntervalLabel;

/// One GPU interval mapped onto the CPU timeline.
pub(super) struct Resolved {
    pub(super) label: IntervalLabel,
    pub(super) start_ns: i64,
    pub(super) stop_ns: i64,
}

/// The holes in a dispatch's GPU timeline: the parts of `[start_ns, stop_ns]` covered by no
/// interval at the finest granularity available, in ascending order and disjoint.
///
/// Resolution is two-level. Between the seams, a hole is time the GPU spent outside any seam.
/// Inside a seam, the pass intervals nested in it resolve the seam's own span; a seam with no pass
/// intervals (subdivision off, or a seam that draws no passes) is treated as solid work, since
/// nothing finer was measured. Holes are therefore a *lower* bound on GPU idle: starvation between
/// individual draws inside one pass is not visible here.
pub(super) fn starvation_gaps(
    resolved: &[Resolved],
    start_ns: i64,
    stop_ns: i64,
) -> Vec<(i64, i64)> {
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

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use puffin::ScopeId;

    use crate::profiler::gpu::{
        GpuSeam,
        gaps::{Resolved, starvation_gaps},
        queries::IntervalLabel,
    };

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
}
