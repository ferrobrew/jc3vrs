//! The frame-loop phase ring: milestones marked by the render path, dumped by a crash record when
//! the backtrace alone cannot say where in the frame execution was.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::crash::line::Line;

/// A frame-loop milestone, recorded into the [`BREADCRUMBS`] ring by [`mark`]. When a crash's stack is
/// unreliable (COMDAT-folded generics, a smashed stack, an unwind that can't cross the exception
/// frame), the ordered ring of recent phases still says *where in the frame* execution was -- which the
/// backtrace alone often cannot. Reading it in the handler is a plain array load, so it can never
/// itself fault.
#[derive(Clone, Copy)]
#[repr(u32)]
pub enum Phase {
    UpdateRenderEnter = 1,
    OriginalUpdateRender,
    Eye0Snapshot,
    Eye0Draw,
    Eye0Drain,
    Eye0Post,
    BetweenEyesRestore,
    Eye1Draw,
    Eye1Drain,
    Eye1Post,
    Present,
    NonStereoDraw,
    FrameEnd,
}

/// Record a frame-loop milestone. A single relaxed store into a ring -- no I/O, no lock, cheap enough
/// to call on every phase transition each frame. The handler dumps the ring on a crash.
pub fn mark(phase: Phase) {
    let pos = BREADCRUMB_POS.fetch_add(1, Ordering::Relaxed);
    BREADCRUMBS[pos % BREADCRUMB_COUNT].store(phase as u32, Ordering::Relaxed);
}

/// Dump the breadcrumb ring, newest first (so truncation drops the oldest, never the crash point).
pub(super) fn log_breadcrumbs() {
    let pos = BREADCRUMB_POS.load(Ordering::Relaxed);
    if pos == 0 {
        return;
    }
    let count = pos.min(BREADCRUMB_COUNT);
    let mut line = Line::new();
    line.str("recent phases (newest first): ");
    for i in 0..count {
        let code = BREADCRUMBS[(pos - 1 - i) % BREADCRUMB_COUNT].load(Ordering::Relaxed);
        line.str(phase_name(code));
        if i + 1 < count {
            line.str(" <- ");
        }
    }
    line.flush();
}

const BREADCRUMB_COUNT: usize = 24;
static BREADCRUMBS: [AtomicU32; BREADCRUMB_COUNT] = [const { AtomicU32::new(0) }; BREADCRUMB_COUNT];
static BREADCRUMB_POS: AtomicUsize = AtomicUsize::new(0);

fn phase_name(code: u32) -> &'static str {
    match code {
        1 => "UpdateRenderEnter",
        2 => "OriginalUpdateRender",
        3 => "Eye0Snapshot",
        4 => "Eye0Draw",
        5 => "Eye0Drain",
        6 => "Eye0Post",
        7 => "BetweenEyesRestore",
        8 => "Eye1Draw",
        9 => "Eye1Drain",
        10 => "Eye1Post",
        11 => "Present",
        12 => "NonStereoDraw",
        13 => "FrameEnd",
        _ => "?",
    }
}
