//! The terrain detail-tessellation budget fix (issue #40).
//!
//! The detail rock skin (`CTerrainRenderBlockDetail` — cliff walls, cave ceilings, near-field rock)
//! is built each frame by a GPU pipeline: compute stages admit quads by frustum and distance, then
//! allocate their vertices, indices, and texels from fixed-size buffers with *unbounded* atomic
//! cursors — there is no capacity check anywhere. Whatever overflows writes out of bounds, D3D11
//! silently drops it, and the losing tiles get empty indirect-draw arguments: they render as
//! world-locked black tiles that flip with head rotation (the admitted quad set shifts with view,
//! changing which tiles lose the allocation race). The shipped buffer sizes fit the flatscreen FOV;
//! VR's wide FOV admits roughly 2–3x the quads and oversubscribes them.
//!
//! The fix scales the budget: the five buffer-size immediates in the terrain setup types' `Create`
//! functions are patched to `shipped * scale` (through the patcher, so they auto-revert on
//! uninject), and the two setup types are destroyed and re-created so the larger buffers go live —
//! required because injection happens after the engine has already created them, and these types'
//! `Recreate` is a no-op in the release build. Applied automatically at the first frame start after
//! injection and re-appliable from the debug UI. See
//! [`StereoConfig::terrain_detail_budget_scale`](crate::config::StereoConfig).

use std::sync::atomic::{AtomicBool, Ordering};

use detours_macro::detour;
use jc3gi::graphics_engine::{
    graphics_engine::{GraphicsEngine, RenderContext},
    render_block::{RBIInfo, RenderBlockTerrainDetail},
    render_engine::{
        RenderBlockTypeRegistry, RenderEngine, TerrainDetailBudgetPatchSites as BudgetSites,
    },
};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&TERRAIN_DETAIL_DRAW_BINDER)
}

/// Detour on the terrain-detail render block's `Draw` for single-pass stereo. The detail rock skin is
/// GPU-indirect (`DrawIndexedInstancedIndirect`), so it cannot be instance-doubled like the model
/// geometry -- `SV_InstanceID` stays `0`, so nothing selects an eye. When the single-pass terrain path
/// is active, re-issue the draw once per eye with a per-eye `cb1` (the detail VS's `T_patch·OffsetVP`
/// transform reprojected by that eye's `M_eye`) and the eye's half-viewport; otherwise draw normally.
#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockTerrainDetail::Draw_ADDRESS)]
fn terrain_detail_draw(
    this: *const RenderBlockTerrainDetail,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let detour = TERRAIN_DETAIL_DRAW.get().unwrap();
    // SAFETY: `this`/`rc`/`info` are the live pointers the engine passed into `Draw`.
    let handled = unsafe {
        crate::stereo::single_pass::terrain_detail_per_eye(this, rc, || {
            detour.call(this, rc, info);
        })
    };
    if !handled {
        detour.call(this, rc, info);
    }
}

/// Set when a detail-budget apply is wanted; consumed by [`process_budget_request`] at the next
/// frame start on the game thread.
static BUDGET_APPLY_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Whether the automatic once-per-session apply has been requested yet.
static STARTUP_APPLY_DONE: AtomicBool = AtomicBool::new(false);

/// Request the detail-tessellation budget apply (from the debug UI): patch the setup types'
/// buffer-size immediates by the configured scale and re-create the two terrain setup types so the
/// larger buffers go live.
pub fn request_detail_budget_apply() {
    BUDGET_APPLY_REQUESTED.store(true, Ordering::Relaxed);
}

/// The five buffer-size immediates and their shipped values (see
/// [`TerrainDetailBudgetPatchSites`](jc3gi::graphics_engine::render_engine::TerrainDetailBudgetPatchSites)):
/// the detail vertex/index/texel budgets the GPU pipeline allocates from.
const BUDGET_SITES: [(u64, u32); 5] = [
    (BudgetSites::VERTEX_COUNT, 0x1_0000),
    (BudgetSites::DEBUG_VERTEX_COUNT, 0x1_0000),
    (BudgetSites::INDEX_BYTES, 0x4_0000),
    (BudgetSites::TEXEL_COUNT, 0x8000),
    (BudgetSites::INDEX_VIEW_COUNT, 0x8000),
];

/// If an apply was requested, patch the detail-budget buffer sizes to `shipped * scale` and
/// re-create the two terrain setup types so the new sizes take effect. Their release-build
/// `Recreate` is a no-op, so this goes through the same `Destroy` + `Create` pair `RegisterType`
/// runs at startup, with the render engine's own resource context. Call once per frame on the game
/// thread with no draw in flight; the immediates auto-revert on uninject.
pub fn process_budget_request() {
    let scale = Config::lock_query(|c| c.stereo.terrain_detail_budget_scale).clamp(1, 16);
    // The automatic once-per-session apply: the first frame-start after injection, when the engine
    // singletons are guaranteed live. Skipped at scale 1 (the shipped sizes).
    let startup = !STARTUP_APPLY_DONE.swap(true, Ordering::Relaxed) && scale > 1;
    if !BUDGET_APPLY_REQUESTED.swap(false, Ordering::Relaxed) && !startup {
        return;
    }
    {
        let Some(mut patcher) = crate::hooks::patcher() else {
            tracing::warn!("terrain detail budget: patcher unavailable; skipping");
            return;
        };
        for (site, shipped) in BUDGET_SITES {
            // SAFETY: each site is the imm32 operand of a `mov [mem], imm32` inside the setup
            // types' `Create` functions (release-IDB-verified); the patcher restores the shipped
            // bytes on uninject.
            unsafe {
                patcher.patch(site as usize, &(shipped * scale).to_le_bytes());
            }
        }
    }
    // SAFETY: runs at frame start on the game thread; the engine singletons are live. The draw is
    // drained before the types destroy and re-create their GPU buffers (the same discipline as the
    // shader-reload path).
    unsafe {
        let (Some(ge), Some(re), Some(reg)) = (
            GraphicsEngine::get(),
            RenderEngine::get(),
            RenderBlockTypeRegistry::get(),
        ) else {
            tracing::warn!(
                "terrain detail budget: engine singletons unavailable; sizes patched but not re-created"
            );
            return;
        };
        ge.WaitForCPUDrawToFinish();
        let ctx = &raw mut re.m_ResourceContext;
        let mut recreated = 0;
        for entry in reg.as_slice() {
            let Some(ty) = entry.m_Type.as_mut() else {
                continue;
            };
            if matches!(
                ty.get_type_name_str(),
                Some("VolumetricTerrainSetup" | "VolumetricTerrainPatchSetup")
            ) {
                ty.Destroy(ctx);
                ty.Create(ctx);
                recreated += 1;
            }
        }
        tracing::info!(
            scale,
            recreated,
            "terrain detail budget applied (#40): buffer sizes scaled and setup types re-created"
        );
    }
}
