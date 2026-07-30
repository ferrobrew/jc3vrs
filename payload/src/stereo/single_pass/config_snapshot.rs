//! The single-pass config snapshot: the flags the render path reads, sampled off the mutex-guarded
//! configuration and held in an atomic bitfield.
//!
//! Two snapshots, for one reason. Every gate in the feature is consulted from viewport, draw, and
//! render-block detours, so reading the configuration behind its mutex would cost a lock per draw
//! submission -- the exact cost single-pass exists to remove. And with the frame tail deferred, the
//! game thread is a frame ahead of the draw thread, so a flag that moved between an arm and its
//! restore would leave that state raised for the rest of the frame. The game thread therefore
//! refreshes a process-wide snapshot per frame, and the draw thread pins its own per dispatch.

use std::{
    cell::Cell,
    sync::atomic::{AtomicU32, Ordering},
};

use crate::config::Config;

/// One flag in the per-frame single-pass config snapshot.
#[derive(Clone, Copy)]
pub(super) enum Flag {
    SinglePass,
    DryRun,
    DualEye,
    Collapse,
    DoubleWide,
    Reproject,
    ReprojectCameraOnly,
    Terrain,
    TreeImpostors,
    Bark,
    Foliage,
    Occluder,
    InstancedPerEye,
    IndirectPerEye,
    UniformViewportSlots,
    ViewportFollowsTarget,
    Slot13PerEye,
}

/// The single-pass config flags, sampled once per frame and pinned for the duration of a dispatch.
#[derive(Clone, Copy)]
pub(super) struct ConfigFlags(u32);

impl ConfigFlags {
    pub(super) fn has(self, flag: Flag) -> bool {
        self.0 & (1 << flag as u32) != 0
    }
}

/// The flags this draw thread's dispatch was opened with, else the game thread's frame snapshot, else
/// a fresh read if neither has been taken yet.
///
/// Every gate in this module is consulted from the render thread on paths that run per draw -- the
/// viewport, indexed-draw and render-block detours -- so reading them through `Config`'s mutex made
/// the feature pay a lock acquisition per draw submission, in a feature whose purpose is to reduce
/// per-draw cost.
///
/// There are two snapshots because the two consumers want different things, and one snapshot cannot
/// give both. The **draw thread** wants a value that does not move for the whole dispatch: nearly
/// every gate here brackets something (a pass range, an eye-half viewport, an armed reprojection,
/// a per-eye re-issue loop), and a flag that changes between the arm and the restore leaks that state
/// into the rest of the frame -- so it pins its own copy in [`DISPATCH_FLAGS`] at
/// [`begin_dispatch`]. Everything else -- the game thread's resolution and camera decisions, and the
/// shader-creation hooks, which must see a toggle in the *same* frame the debug UI turns it on,
/// because that frame's shader reload is what applies it -- reads [`CONFIG_FLAGS`], resampled at frame
/// start ([`refresh_config_flags`]).
///
/// The frame snapshot deliberately does **not** reach the draw thread mid-dispatch. It is written on
/// the game thread, which with `stereo.defer_frame_tail` on runs concurrently with the previous
/// frame's still-walking dispatch -- the same interleaving that made the game-thread G-buffer range
/// clear tear a live range (see [`begin_dispatch`]). A flag changing there would land between an arm
/// and its restore, which is why the draw thread reads a pinned copy instead.
pub(super) fn config_flags() -> ConfigFlags {
    if let Some(flags) = DISPATCH_FLAGS.get() {
        return flags;
    }
    let snapshot = CONFIG_FLAGS.load(Ordering::Relaxed);
    if snapshot & CONFIG_FLAGS_VALID != 0 {
        return ConfigFlags(snapshot);
    }
    ConfigFlags(store_config_flags())
}

/// Re-read the single-pass config flags into the frame snapshot. Called at frame start, on the game
/// thread, and read by everything that is not inside a draw-thread dispatch -- see [`config_flags`]
/// for why the draw thread pins its own copy instead of reading this one.
pub fn refresh_config_flags() {
    store_config_flags();
}

/// Pin the config flags for the dispatch this thread is opening, so every gate it consults answers
/// the same for the whole of it. Called from [`begin_dispatch`].
pub(super) fn pin_dispatch_config_flags() {
    DISPATCH_FLAGS.set(Some(ConfigFlags(store_config_flags())));
}

fn store_config_flags() -> u32 {
    let bit = |on: bool, flag: Flag| u32::from(on) << flag as u32;
    let snapshot = Config::lock_query(|c| {
        let s = &c.stereo.single_pass;
        bit(s.enabled, Flag::SinglePass)
            | bit(s.patch_dryrun, Flag::DryRun)
            | bit(s.dual_eye, Flag::DualEye)
            | bit(s.collapse, Flag::Collapse)
            | bit(s.double_wide, Flag::DoubleWide)
            | bit(s.reproject, Flag::Reproject)
            | bit(s.reproject_camera_only, Flag::ReprojectCameraOnly)
            | bit(s.terrain, Flag::Terrain)
            | bit(s.tree_impostors, Flag::TreeImpostors)
            | bit(s.bark, Flag::Bark)
            | bit(s.foliage, Flag::Foliage)
            | bit(s.occluder, Flag::Occluder)
            | bit(s.instanced_per_eye, Flag::InstancedPerEye)
            | bit(s.indirect_per_eye, Flag::IndirectPerEye)
            | bit(s.uniform_viewport_slots, Flag::UniformViewportSlots)
            | bit(
                s.collapse_viewport_follows_target,
                Flag::ViewportFollowsTarget,
            )
            | bit(s.slot13_per_eye, Flag::Slot13PerEye)
    }) | CONFIG_FLAGS_VALID;
    CONFIG_FLAGS.store(snapshot, Ordering::Relaxed);
    snapshot
}

/// Marks [`CONFIG_FLAGS`] as having been sampled at least once, so a zero snapshot before the first
/// frame is not mistaken for "everything off".
const CONFIG_FLAGS_VALID: u32 = 1 << 31;
static CONFIG_FLAGS: AtomicU32 = AtomicU32::new(0);

thread_local! {
    /// The flags [`begin_dispatch`] pinned for the dispatch in flight on this thread, or `None` on a
    /// thread that has never opened one. Thread-local rather than global because the point is to keep
    /// the draw thread's view of the config still while the game thread is free to resample its own.
    static DISPATCH_FLAGS: Cell<Option<ConfigFlags>> = const { Cell::new(None) };
}
