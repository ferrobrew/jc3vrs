//! Stereo draw-sequence diff: the feasibility probe for single-pass stereo (halving the per-frame
//! draw submission, the measured bottleneck -- ~20k draw calls/frame at ~41% GPU utilisation).
//!
//! Any technique that renders both eyes from one CPU walk requires the two eyes' draw sequences to
//! be identical -- same passes, same draw counts, in the same order. The mod already pins the
//! render lists, constant-buffer ring, and frame counters identical between eyes, so they *should*
//! be; this measures whether they actually are. During a stereo frame it records, per render pass,
//! the number of GPU ops (`Draw*` + `Dispatch`) each eye issues, then diffs the two sequences and
//! logs which passes match (replayable from one walk) and which diverge (the per-eye
//! special-casing burden -- expected: the screen-space passes that read per-eye history, SSAO / GI
//! / SSR / post). The fraction of ops in matching passes is the feasibility verdict.
//!
//! Off by default; toggled from the Diagnostics-adjacent Render controls. Reads the live
//! per-eye draw counters ([`crate::hooks::draw_count::DRAW_COUNTS`]) around each `DoDraw`, so it
//! adds only two atomic loads per pass when active and nothing when off.

use std::sync::atomic::Ordering;

use parking_lot::Mutex;

use crate::hooks::draw_count::DRAW_COUNTS;

/// Whether the probe is recording this session (the `diagnose_stereo_draw_diff` toggle).
pub fn is_active() -> bool {
    crate::config::Config::lock_query(|c| c.stereo.diagnose_stereo_draw_diff)
}

/// The running total of GPU ops issued this dispatch (draws + indexed draws + compute dispatches).
/// `DoDraw` deltas of this attribute ops to a pass. The counters are cleared per dispatch by the
/// draw driver, so the delta across one `DoDraw` is that pass's op count.
pub fn op_total() -> u32 {
    let c = &DRAW_COUNTS;
    (c.draw.load(Ordering::Relaxed)
        + c.draw_indexed.load(Ordering::Relaxed)
        + c.dispatch.load(Ordering::Relaxed)) as u32
}

/// Records one pass's op count for `eye` (`0` or `1`). Appended in draw order, so the two eyes'
/// sequences can be compared position-by-position.
pub fn record_pass(pass_id: i16, eye: usize, ops: u32) {
    let mut state = STATE.lock();
    let seq = if eye == 0 {
        &mut state.eye0
    } else {
        &mut state.eye1
    };
    seq.push((pass_id, ops));
}

/// Diffs the two eyes' recorded sequences, logs the verdict, and clears for the next frame. Called
/// once per stereo frame after both eyes' dispatches; throttled so it logs roughly once a second.
/// A no-op if either eye recorded nothing.
///
/// Aligns **by pass id**, not position: eye 1 legitimately runs fewer passes than eye 0 when the
/// view-independent prepasses (reflections, shadow atlas, water sim) are shared -- it skips the
/// ones eye 0 already rendered -- so a positional diff mis-aligns at the first skipped pass. The
/// per-eye scene work is the passes *both* eyes run; that set's op-count agreement is the real
/// single-pass-stereo verdict. Passes only one eye runs are reported separately (eye-0-only =
/// already-shared prepasses; eye-1-only = unexpected).
pub fn report_and_clear() {
    let (eye0, eye1) = {
        let mut state = STATE.lock();
        (
            std::mem::take(&mut state.eye0),
            std::mem::take(&mut state.eye1),
        )
    };
    if eye0.is_empty() || eye1.is_empty() {
        return;
    }

    let n = FRAME.with(|c| {
        let n = c.get() + 1;
        c.set(n);
        n
    });
    if !n.is_multiple_of(LOG_EVERY) {
        return;
    }

    let map0 = aggregate(&eye0);
    let map1 = aggregate(&eye1);

    let mut both_ops = 0u32; // eye0 ops in passes both eyes run
    let mut matched_ops = 0u32; // ...of those, passes whose counts agree
    let mut divergent: Vec<(i16, u32, u32)> = Vec::new();
    let mut eye0_only_ops = 0u32;
    let mut eye0_only = 0usize;
    for &(id, n0) in &map0 {
        match map1.iter().find(|&&(i, _)| i == id) {
            Some(&(_, n1)) => {
                both_ops += n0;
                if n0 == n1 {
                    matched_ops += n0;
                } else {
                    divergent.push((id, n0, n1));
                }
            }
            None => {
                eye0_only += 1;
                eye0_only_ops += n0;
            }
        }
    }
    let eye1_only: Vec<i16> = map1
        .iter()
        .filter(|&&(id, _)| !map0.iter().any(|&(i, _)| i == id))
        .map(|&(id, _)| id)
        .collect();

    // Whether the shared passes keep the same relative order in both eyes (single-pass needs the
    // one walk to visit them in an order valid for both).
    let order_ok = same_relative_order(&eye0, &eye1);

    let replayable_pct = if both_ops == 0 {
        0.0
    } else {
        100.0 * f64::from(matched_ops) / f64::from(both_ops)
    };
    tracing::info!(
        target: "stereo_diff",
        "stereo draw diff: {} shared passes ({both_ops} eye0 ops), {replayable_pct:.1}% with \
         matching op counts; {eye0_only} eye-0-only passes ({eye0_only_ops} ops, shared prepasses); \
         {} eye-1-only; shared order {}",
        map0.len() - eye0_only,
        eye1_only.len(),
        if order_ok { "consistent" } else { "DIVERGENT" },
    );
    for (id, n0, n1) in &divergent {
        tracing::info!(
            target: "stereo_diff",
            "  divergent shared pass {} ({id}): eye0 {n0} ops, eye1 {n1} ops (per-eye work)",
            pass_name(*id),
        );
    }
    for id in &eye1_only {
        tracing::warn!(
            target: "stereo_diff",
            "  eye-1-only pass {} ({id}) -- unexpected; eye 1 should be a subset of eye 0",
            pass_name(*id),
        );
    }
}

/// Sums ops per pass id, preserving first-seen order.
fn aggregate(seq: &[(i16, u32)]) -> Vec<(i16, u32)> {
    let mut out: Vec<(i16, u32)> = Vec::new();
    for &(id, ops) in seq {
        match out.iter_mut().find(|(i, _)| *i == id) {
            Some((_, n)) => *n += ops,
            None => out.push((id, ops)),
        }
    }
    out
}

/// Whether the passes common to both eyes appear in the same relative order in each sequence.
fn same_relative_order(eye0: &[(i16, u32)], eye1: &[(i16, u32)]) -> bool {
    let common: Vec<i16> = eye0
        .iter()
        .map(|&(id, _)| id)
        .filter(|id| eye1.iter().any(|&(i, _)| i == *id))
        .collect();
    let eye1_common: Vec<i16> = eye1
        .iter()
        .map(|&(id, _)| id)
        .filter(|id| common.contains(id))
        .collect();
    common == eye1_common
}

thread_local! {
    /// Frame counter for log throttling; the report runs on the main (game) thread.
    static FRAME: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Log the diff every this many stereo frames (~1 s at 60 fps).
const LOG_EVERY: u64 = 60;

/// The engine's debug name for a render-pass id, via the pass-name table. Uses a raw `i32`
/// signature because `m_Index` can hold an unnamed id that is not a valid `RenderPassId`
/// discriminant (transmuting it would be UB); the engine returns `"NONE"` for those.
fn pass_name(pass_id: i16) -> &'static str {
    use jc3gi::graphics_engine::render_engine::GetRenderPassName_ADDRESS;
    // SAFETY: the lookup reads only its argument and returns a static string in the module image.
    unsafe {
        let lookup: unsafe extern "system" fn(i32) -> *const u8 =
            std::mem::transmute(GetRenderPassName_ADDRESS);
        let ptr = lookup(i32::from(pass_id));
        if ptr.is_null() {
            return "?";
        }
        std::ffi::CStr::from_ptr(ptr.cast()).to_str().unwrap_or("?")
    }
}

static STATE: Mutex<StereoDiff> = Mutex::new(StereoDiff::new());

struct StereoDiff {
    eye0: Vec<(i16, u32)>,
    eye1: Vec<(i16, u32)>,
}

impl StereoDiff {
    const fn new() -> Self {
        Self {
            eye0: Vec::new(),
            eye1: Vec::new(),
        }
    }
}
