//! The monoscopic far field (issue #32), increment 1: identify and skip the far-regime scene work
//! so it can later be rendered once and shared between the eyes.
//!
//! Two mechanisms, matched to how the engine organizes the work (see
//! `docs/issues/32-monoscopic-far-field.md`):
//!
//! * **Per-entry draw-list split** for the model-family passes, whose blocks carry real
//!   per-instance `RBIInfo` world transforms. Every render block reports its squared camera
//!   distance at sort time, and each pass's `FrontToBackBucketed` sort quantizes that distance
//!   through the pass's depth-bucket table — but almost every pass ships with a single bucket, so
//!   the quantization degenerates and the list is only type-batched. Registering a boundary at the
//!   threshold (plus a beyond-everything sentinel, because the engine's bucket search never
//!   separates the *last* table entry) makes the once-per-rotation sort produce a contiguous
//!   `[near][far]` list, which `DoDraw` can be windowed onto by temporarily narrowing the list
//!   header it walks. Both eyes draw the same sorted list, so the window is per-dispatch state.
//!
//! * **Type gating** for the block types that are *inherently* far-regime: the volumetric terrain
//!   patches draw only the distant terrain (near terrain hands off to other block types as the
//!   camera approaches), so the whole type can be skipped/shared without any distance split. The
//!   gated types' `IsEnabled` vtable slots are pointed at a stub that consults the far-field mode
//!   per dispatch — `CRenderPass::DoDraw` dispatches `IsEnabled` per type run, so the gate follows
//!   the eye without repatching. The type list is user-editable (find candidates with the
//!   Diagnostics tab's registry bisect).
//!
//! This increment carries the split, the gate, the dial-in modes (skip far / skip near / skip far
//! on eye 1 only), and the counters/dump the Render and Performance tabs read. Rendering the far
//! work once and compositing it under both eyes builds on this: the far image relates to each eye
//! by an exact 2D homography (same camera centre, different projection), so the composite is a
//! full-screen warp, with only the IPD translation left as the threshold-bounded parallax error.

use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicI32, Ordering},
    time::Instant,
};

use jc3gi::graphics_engine::{
    graphics_engine::RenderContext,
    render_engine::RenderBlockTypeRegistry,
    render_pass::{RBILists, RenderInstance, RenderPass, RenderPassSortMethod, SortContext},
};
use parking_lot::Mutex;

use crate::config::{Config, FarFieldMode};

pub mod share;

/// Per-pass split counters for the debug UI.
#[derive(Clone, Copy)]
pub struct PassSplit {
    /// Entries below the threshold in the last-drawn list.
    pub near: u32,
    /// Entries at or beyond the threshold.
    pub far: u32,
    /// Whether the last draw was windowed (a run was actually skipped).
    pub windowed: bool,
    /// When this pass last drew, so the UI can drop stale entries.
    pub updated: Instant,
}

/// Snapshot the per-pass counters, keyed by pass id. Entries persist until overwritten; filter on
/// [`PassSplit::updated`] for recency.
pub fn stats_snapshot() -> Vec<(i16, PassSplit)> {
    STATS.lock().iter().map(|(k, v)| (*k, *v)).collect()
}

/// Request a one-shot state dump: the next `budget` split-pass draws log their full classification
/// state (buckets and keys, or the filter reason) to the log at INFO under the `far_field` target.
pub fn request_dump(budget: i32) {
    DUMP_BUDGET.store(budget, Ordering::Relaxed);
}

/// Sync the gated-type `IsEnabled` overrides to `names` (comma/whitespace separated type names, as
/// shown in the render-block-type registry). Newly listed types get their slot pointed at the
/// mode-following stub; delisted ones are reverted. Call whenever the config list changes (the
/// slots are also reverted wholesale on uninject by the patcher).
pub fn sync_type_gates(names: &str) {
    {
        let mut last = LAST_SYNCED_GATES.lock();
        if last.as_deref() == Some(names) {
            return;
        }
        *last = Some(names.to_owned());
    }
    let wanted: Vec<&str> = names
        .split([',', ' '])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    let mut gated = GATED_TYPE_SLOTS.lock();
    let Some(mut patcher) = crate::hooks::patcher() else {
        return;
    };
    // SAFETY: the registry is static engine storage with live type singletons; the slot writes are
    // aligned qword stores through the patcher, mirroring the diagnostics registry kill switch.
    unsafe {
        let Some(registry) = RenderBlockTypeRegistry::get() else {
            return;
        };
        let entries = registry.as_slice();
        if entries.is_empty() || entries.len() > 256 {
            return;
        }
        let mut wanted_slots = BTreeMap::new();
        for entry in entries {
            let Some(ty) = entry.m_Type.as_mut() else {
                continue;
            };
            let Some(name) = ty.get_type_name_str() else {
                continue;
            };
            if wanted.iter().any(|w| w.eq_ignore_ascii_case(name)) {
                wanted_slots.insert(
                    (&raw const (*ty.vftable()).IsEnabled) as usize,
                    name.to_owned(),
                );
            }
        }
        for slot in gated.keys().copied().collect::<Vec<_>>() {
            if !wanted_slots.contains_key(&slot) {
                patcher.unpatch(slot);
                gated.remove(&slot);
            }
        }
        let stub = far_gated_type_is_enabled as *const () as usize;
        for (slot, name) in wanted_slots {
            if gated.contains_key(&slot) {
                continue;
            }
            patcher.patch(slot, &stub.to_le_bytes());
            tracing::info!(target: "far_field", "gating far-regime type {name}");
            gated.insert(slot, name);
        }
    }
}

/// The names of the currently gated types, for the UI.
pub fn gated_type_names() -> Vec<String> {
    GATED_TYPE_SLOTS.lock().values().cloned().collect()
}

/// The temporary draw-list window; restores the list header on drop (same render thread, within
/// one `DoDraw` call).
pub struct ListWindow {
    list: *mut RBILists,
    saved_list: *mut RenderInstance,
    saved_count: u32,
}

impl Drop for ListWindow {
    fn drop(&mut self) {
        // SAFETY: the list header outlives the enclosing `DoDraw` call this window is scoped to,
        // and only the render thread reads it during a draw.
        unsafe {
            (*self.list).m_List = self.saved_list;
            (*self.list).m_NumElements = self.saved_count;
        }
    }
}

/// Called by the `DoDraw` detour before the original runs: maintains this pass's depth-bucket
/// registration, makes sure this rotation's sort ran, records the near/far counters, and — when
/// the mode calls for it — narrows the draw list to one run, returning the guard that restores it
/// after the original draw.
///
/// # Safety
///
/// `pass` and `ctx` are the live pass and render context of a `DoDraw` call on the render thread.
pub unsafe fn before_do_draw(pass: *mut RenderPass, ctx: *mut RenderContext) -> Option<ListWindow> {
    let cfg = Config::lock_query(|c| c.far_field.clone());
    let pass = unsafe { pass.as_mut() }?;
    let ctx = unsafe { ctx.as_ref() }?;

    let id = pass.m_Index;
    if !cfg.enabled || !SPLIT_PASSES.contains(&id) {
        restore_buckets(pass);
        return None;
    }
    // Only the default bucketed sort partitions by bucket index: alpha passes resolve `Auto` to
    // back-to-front (raw distance keys), and `None`/`SortID` passes never compute depth keys.
    if pass.m_SortMethod != RenderPassSortMethod::RenderPassSortMethod_Auto
        || pass.m_RenderBackToFront != 0
        || !apply_buckets(pass, cfg.threshold_m)
    {
        if DUMP_BUDGET.load(Ordering::Relaxed) > 0 {
            DUMP_BUDGET.fetch_sub(1, Ordering::Relaxed);
            tracing::info!(
                target: "far_field",
                pass = format!("{id:#04X}"),
                sort_method = ?pass.m_SortMethod,
                back_to_front = pass.m_RenderBackToFront,
                buckets = pass.m_NumDepthBuckets,
                "far-field pass filtered out"
            );
        }
        return None;
    }

    // Sort before scanning for the split. The engine sorts at most once per list rotation (the
    // m_Sorted latch, under the pass spinlock), so when the sort task or an earlier draw already
    // ran this is a no-op; the sort context mirrors the one `DoDraw` itself builds.
    let sc = SortContext {
        m_RenderPassIndex: ctx.m_ActiveRenderPass,
        m_RenderFrameIndex: ctx.m_TransformIndex,
        m_CameraPosition: ctx.m_CameraPosition,
        m_CameraNear: ctx.CameraNear,
    };
    unsafe { pass.SortList(&sc) };

    let list = pass.m_CurrentDrawList;
    let header = unsafe { list.as_ref() }?;
    let count = u32::from(header.m_ListSize).min(header.m_NumElements);
    // A pass that has never received an entry keeps a null array, so the null check is
    // load-bearing (`from_raw_parts` requires non-null even for an empty slice).
    if count == 0 || header.m_List.is_null() {
        return None;
    }
    let (saved_list, saved_count) = (header.m_List, header.m_NumElements);
    // SAFETY: the entry array is live and at least `count` long for the whole dispatch; `DoDraw`
    // is about to walk exactly this range.
    let entries = unsafe { std::slice::from_raw_parts(header.m_List, count as usize) };

    // With our table `{0, threshold², sentinel}` the far bucket is the last usable one
    // (`count - 1`); the sorted list is ascending in bucket index, so the far run is the suffix.
    let far_key = f32::from(pass.m_NumDepthBuckets - 1);
    let split = entries.partition_point(|e| e.m_Depth < far_key) as u32;

    if DUMP_BUDGET.load(Ordering::Relaxed) > 0 && DUMP_BUDGET.fetch_sub(1, Ordering::Relaxed) > 0 {
        let keys: Vec<f32> = entries.iter().map(|e| e.m_Depth).collect();
        tracing::info!(
            target: "far_field",
            pass = format!("{id:#04X}"),
            camera = ?ctx.m_CameraPosition.data,
            buckets = ?&pass.m_DepthSqTable[..(pass.m_NumDepthBuckets as usize).min(16)],
            count = entries.len(),
            split,
            ?keys,
            "far-field split state"
        );
    }

    let window = match cfg.mode {
        FarFieldMode::Collect => None,
        FarFieldMode::SkipFar => Some((0, split)),
        FarFieldMode::SkipNear => Some((split, count)),
        FarFieldMode::SkipFarEye1 => crate::stereo::is_second_eye().then_some((0, split)),
        // A share frame's far dispatch draws only the far run; the near dispatches only the
        // near run. Outside a share frame (stereo off), Share behaves like Collect.
        FarFieldMode::Share => {
            if crate::stereo::far_phase() {
                Some((split, count))
            } else if crate::stereo::share_frame() {
                Some((0, split))
            } else {
                None
            }
        }
    };
    STATS.lock().insert(
        id,
        PassSplit {
            near: split,
            far: count - split,
            windowed: window.is_some_and(|(lo, hi)| !(lo == 0 && hi == count)),
            updated: Instant::now(),
        },
    );

    let (lo, hi) = window?;
    if lo == 0 && hi == count {
        return None;
    }
    // SAFETY: scoped narrowing of the live list header, restored by the returned guard before
    // anything else reads it (the sort is latched, and adds go to the other parity's list).
    unsafe {
        (*list).m_List = saved_list.add(lo as usize);
        (*list).m_NumElements = hi - lo;
    }
    Some(ListWindow {
        list,
        saved_list,
        saved_count,
    })
}

/// The per-pass split counters, keyed by pass id.
static STATS: Mutex<BTreeMap<i16, PassSplit>> = Mutex::new(BTreeMap::new());

/// Remaining passes the one-shot dump will log.
static DUMP_BUDGET: AtomicI32 = AtomicI32::new(0);

/// The `IsEnabled` vtable slots currently pointed at [`far_gated_type_is_enabled`], keyed by slot
/// address, with the type name for the UI.
static GATED_TYPE_SLOTS: Mutex<BTreeMap<usize, String>> = Mutex::new(BTreeMap::new());

/// The last list [`sync_type_gates`] applied, so the per-frame UI call no-ops when unchanged.
static LAST_SYNCED_GATES: Mutex<Option<String>> = Mutex::new(None);

/// The passes eligible for the per-entry split: the model family and creatures, whose blocks
/// carry real per-instance `RBIInfo` world transforms. Terrain, vegetation, and forest blocks
/// never get their transforms written (their distance query degenerates to the camera's distance
/// from the world origin), and the screen-space passes hold a handful of transform-less
/// fullscreen blocks that must never be skipped; the far-regime block types among them are gated
/// at type level instead ([`sync_type_gates`]).
const SPLIT_PASSES: &[i16] = &[
    0x41, // RP_MODELS_DYNAMIC
    0x42, // RP_MODELS_DYNAMIC_MASK_DAMAGE_POST_EFFECT
    0x43, // RP_MODELS_STATIC
    0x44, // RP_MODELS_REFLECTION
    0x4B, // RP_CREATURES
];

/// The `IsEnabled` replacement for the gated far-regime types: report disabled exactly when the
/// far-field mode is skipping the far field for the current dispatch's eye. `CRenderPass::DoDraw`
/// dispatches this per type run, so the gate follows the eye and the live config without any
/// repatching. (The stock release implementation compiles to a constant `true`.)
unsafe extern "system" fn far_gated_type_is_enabled(_this: *mut ::core::ffi::c_void) -> bool {
    let (enabled, mode) = Config::lock_query(|c| (c.far_field.enabled, c.far_field.mode));
    let skipping = enabled
        && match mode {
            FarFieldMode::SkipFar => true,
            FarFieldMode::SkipFarEye1 => crate::stereo::is_second_eye(),
            // The gated types are far-regime content: on a share frame they render only in the
            // far dispatch (the near dispatches composite them from the capture).
            FarFieldMode::Share => crate::stereo::share_frame() && !crate::stereo::far_phase(),
            FarFieldMode::Collect | FarFieldMode::SkipNear => false,
        };
    !skipping
}

/// Whether this frame should run the three-dispatch far-field share: the split is enabled in
/// `Share` mode (the Draw driver also requires stereo to be active).
pub fn share_configured() -> bool {
    Config::lock_query(|c| c.far_field.enabled && c.far_field.mode == FarFieldMode::Share)
}

/// The sentinel boundary appended after the threshold. The engine's bucket search
/// (`ComputeDepthBucket`) merges everything at or beyond the *last* table entry into the
/// second-to-last bucket, so the threshold only separates when a boundary lies beyond it; this is
/// far larger than any squared scene distance (the far plane is 38.4 km, ~1.5e9 m²).
const BUCKET_SENTINEL_SQ: f32 = 1.0e30;

/// Whether this pass carries our appended boundaries (threshold + sentinel). Self-describing, so
/// no registry is needed across enable/disable transitions.
fn has_our_buckets(pass: &RenderPass) -> bool {
    pass.m_NumDepthBuckets == 3 && pass.m_DepthSqTable[2] == BUCKET_SENTINEL_SQ
}

/// Extend a stock single-bucket table to `{0, threshold², sentinel}`, or refresh the threshold on
/// an already-extended pass. Returns whether the pass carries the split boundary.
///
/// The bucket count is written through the patcher (auto-reverted on uninject) and only after the
/// table entries, so a concurrently-running sort never reads a half-built table — at worst that
/// frame's keys quantize through the old table and the split scan finds no far run.
fn apply_buckets(pass: &mut RenderPass, threshold_m: f32) -> bool {
    let sq = threshold_m * threshold_m;
    if has_our_buckets(pass) {
        // A threshold change takes effect at the next rotation's re-sort.
        if pass.m_DepthSqTable[1] != sq {
            pass.m_DepthSqTable[1] = sq;
        }
        return true;
    }
    if pass.m_NumDepthBuckets != 1 {
        return false;
    }
    let Some(mut patcher) = crate::hooks::patcher() else {
        return false;
    };
    pass.m_DepthSqTable[1] = sq;
    pass.m_DepthSqTable[2] = BUCKET_SENTINEL_SQ;
    // SAFETY: the count field is live pass memory; the patcher records the original for the
    // uninject revert.
    unsafe {
        patcher.patch(
            (&raw const pass.m_NumDepthBuckets) as usize,
            &3u16.to_le_bytes(),
        );
    }
    true
}

/// Restore the stock single bucket if this pass carries our boundaries (leaving the inert table
/// residue beyond the count).
fn restore_buckets(pass: &mut RenderPass) {
    if !has_our_buckets(pass) {
        return;
    }
    // SAFETY: reverts the count patch applied in `apply_buckets` on the same live pass.
    if let Some(mut patcher) = crate::hooks::patcher() {
        unsafe { patcher.unpatch((&raw const pass.m_NumDepthBuckets) as usize) };
    }
}
