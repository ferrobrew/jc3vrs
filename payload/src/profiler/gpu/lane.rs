//! The puffin bridge: turning a dispatch's scope ranges into a single-thread stream on the "GPU"
//! lane.

use puffin::{GlobalProfiler, ScopeId, StreamInfo, ThreadInfo};

/// A scope to place on the "GPU" lane, in CPU-timeline nanoseconds.
pub(super) struct GpuScope {
    pub(super) id: ScopeId,
    pub(super) start_ns: i64,
    pub(super) stop_ns: i64,
    pub(super) data: String,
}

/// Builds a single-thread puffin stream for the "GPU" lane out of `scopes` and reports it into the
/// current puffin frame. The scopes are an arbitrary set of ranges — the per-dispatch outer scope,
/// its seams, the passes nested in those, and the measured idle/starvation holes — so the nesting
/// is reconstructed here by containment: sorted by start (longest first on a tie), a stack yields
/// exactly the tree puffin's strictly-LIFO stream format needs. A child whose end overshoots its
/// parent's (equal ticks at a boundary) is clamped to it.
pub(super) fn report_gpu_frame(scopes: Vec<GpuScope>) {
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

    use puffin::{ScopeId, StreamInfo};

    use crate::profiler::gpu::lane::{GpuScope, build_stream};

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
