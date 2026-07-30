//! Per-render-block-type draw counters: how many blocks each type drew, without a puffin scope per
//! run.
//!
//! A pass draws its sorted list in runs of one block type, switching with
//! `CRenderPass::ChangeRenderBlockType`. Naming each run as its own puffin scope answers "how many
//! draws of each family" — at the price of a lock and a stream push per run on the draw-submission
//! path, which is exactly the path a submission-bound frame is trying to measure. These counters
//! answer the same question for a relaxed atomic add: the switch hook reads the outgoing run's
//! block count (the engine's own running counter, which the switch is about to zero) and adds it
//! to that type's slot.
//!
//! The hot path never locks and never allocates: a type's slot index is memoized per thread by the
//! address of its name string (a static in the game image, so one address per type), and only the
//! first sight of a type on a thread takes the registry lock.

use std::{
    cell::RefCell,
    sync::atomic::{AtomicU64, Ordering},
};

use parking_lot::Mutex;

/// Adds `blocks` drawn by the render-block type named `name` (a static string in the game image;
/// its address identifies the type). Two relaxed atomic adds plus a short thread-local scan.
pub fn record(name: &'static str, blocks: u32) {
    let Some(index) = slot_for(name) else {
        return;
    };
    let slot = &SLOTS[index];
    slot.blocks.fetch_add(u64::from(blocks), Ordering::Relaxed);
    slot.runs.fetch_add(1, Ordering::Relaxed);
}

/// Every type seen so far with its totals since the last [`take`], busiest first.
pub fn snapshot() -> Vec<BlockCount> {
    let names = NAMES.lock();
    let mut counts: Vec<BlockCount> = names
        .iter()
        .enumerate()
        .map(|(index, &name)| BlockCount {
            name,
            blocks: SLOTS[index].blocks.load(Ordering::Relaxed),
            runs: SLOTS[index].runs.load(Ordering::Relaxed),
        })
        .filter(|c| c.runs > 0)
        .collect();
    counts.sort_unstable_by_key(|c| std::cmp::Reverse(c.blocks));
    counts
}

/// [`snapshot`], zeroing the counters so the next call reports the next window.
pub fn take() -> Vec<BlockCount> {
    let counts = snapshot();
    for slot in &SLOTS {
        slot.blocks.store(0, Ordering::Relaxed);
        slot.runs.store(0, Ordering::Relaxed);
    }
    counts
}

/// One render-block type's totals over a window.
pub struct BlockCount {
    pub name: &'static str,
    /// Blocks drawn. Undercounts by the final run of each pass draw: the engine closes that run at
    /// the pass tail rather than through a type switch, so its count is never observed here.
    pub blocks: u64,
    /// Type runs (switches into this type).
    pub runs: u64,
}

/// Logs the window's busiest types alongside the GPU summary, then resets them.
pub fn log_summary() {
    let counts = take();
    if counts.is_empty() {
        return;
    }
    let total: u64 = counts.iter().map(|c| c.blocks).sum();
    let top: Vec<String> = counts
        .iter()
        .take(TOP_LOGGED)
        .map(|c| format!("{} {}", c.name, c.blocks))
        .collect();
    tracing::info!(
        "profiler: render blocks drawn over the window: {total} total; {}",
        top.join(", ")
    );
}

/// How many types the periodic log line names.
const TOP_LOGGED: usize = 8;

/// The slot table's fixed capacity. The game registers a few dozen render-block types; a type past
/// the cap is simply not counted.
const MAX_TYPES: usize = 96;

struct Slot {
    blocks: AtomicU64,
    runs: AtomicU64,
}

#[expect(
    clippy::declare_interior_mutable_const,
    reason = "the const is only an initializer for the array; every element is a distinct slot"
)]
const EMPTY_SLOT: Slot = Slot {
    blocks: AtomicU64::new(0),
    runs: AtomicU64::new(0),
};

static SLOTS: [Slot; MAX_TYPES] = [EMPTY_SLOT; MAX_TYPES];

/// The registered type names, indexed like [`SLOTS`]. Only touched on a thread's first sight of a
/// type and by the reporting paths.
static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

thread_local! {
    /// This thread's memo of name address to slot index, linear-scanned (a few dozen entries).
    static MEMO: RefCell<Vec<(*const u8, usize)>> = const { RefCell::new(Vec::new()) };
}

fn slot_for(name: &'static str) -> Option<usize> {
    MEMO.with_borrow_mut(|memo| {
        let key = name.as_ptr();
        if let Some(&(_, index)) = memo.iter().find(|&&(k, _)| k == key) {
            return Some(index);
        }
        let index = register(name)?;
        memo.push((key, index));
        Some(index)
    })
}

fn register(name: &'static str) -> Option<usize> {
    let mut names = NAMES.lock();
    // Another thread may have registered the same type already; match on the text, since two
    // threads reaching the same type always see the same static address anyway.
    if let Some(index) = names.iter().position(|&n| n == name) {
        return Some(index);
    }
    if names.len() >= MAX_TYPES {
        return None;
    }
    names.push(name);
    Some(names.len() - 1)
}
