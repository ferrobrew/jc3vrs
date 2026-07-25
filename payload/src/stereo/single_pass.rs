//! Single-pass stereo (experimental): render the G-buffer geometry once, emitting both eyes via
//! instancing + `SV_ViewportArrayIndex` routing into a double-wide render target, instead of the
//! double-draw (two full `game.Draw` walks, one per eye). See `docs/mod/single-pass-stereo.md` for
//! the design and phased plan.
//!
//! This module owns the mod-side state that the double-draw path does not need:
//! - the DXVK viewport-routing **capability probe** ([`probe`] / [`capability`]);
//! - the vertex-shader rewrite **census** ([`record_patch_outcome`] and the `*_count` getters), which
//!   the `CreateVertexProgram` hook feeds so the debug UI can report how the rewriter fared against
//!   the game's real shader set.
//!
//! The rest of the pipeline (cb13 dual-eye upload, the double-wide render-setup re-init, the
//! draw-doubling) runs under [`crate::config::StereoConfig::single_pass`] and the per-step flags
//! beside it. [`crate::config::StereoConfig::single_pass_patch_dryrun`] runs the census alone, with
//! no rendering change.

use std::{
    cell::Cell,
    collections::BTreeMap,
    ffi::c_void,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
};

use dxbc_stereo::DxbcError;
use jc3gi::{
    graphics_engine::{
        draw::SetVertexProgramConstants,
        graphics_engine::{GraphicsEngine, HContext_t, RenderContext},
        render_block::RenderBlockTerrainDetail,
        render_engine::RenderEngine,
    },
    types::math::{Matrix4, Vector4},
};
use parking_lot::Mutex;
use re_utilities::ThreadSuspender;
use retour::GenericDetour;
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::Direct3D11::{
            D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_WRITE,
            D3D11_FEATURE_D3D11_OPTIONS3, D3D11_FEATURE_DATA_D3D11_OPTIONS3,
            D3D11_MAP_WRITE_DISCARD, D3D11_MAPPED_SUBRESOURCE, D3D11_SUBRESOURCE_DATA,
            D3D11_USAGE_DYNAMIC, D3D11_VIEWPORT, ID3D11Buffer, ID3D11Device, ID3D11DeviceContext,
        },
        System::Threading::{EnterCriticalSection, LeaveCriticalSection},
    },
    core::Interface,
};

use crate::config::Config;

/// The result of the DXVK viewport-routing capability probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Not yet probed (no device seen, or the probe has not run this session).
    Unprobed,
    /// `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer` is supported: a vertex shader may write
    /// `SV_ViewportArrayIndex` directly, so single-pass routing is possible.
    Supported,
    /// The capability is absent; single-pass must fall back to double-draw.
    Unsupported,
}

/// Probe the D3D11 device for `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer` (the D3D11.3 feature
/// that lets a vertex shader write `SV_ViewportArrayIndex`), caching the result. Idempotent and
/// cheap; safe to call every frame. `CheckFeatureSupport` on the device is free-threaded, so no
/// context lock is needed.
pub fn probe(device: &ID3D11Device) -> Capability {
    let mut options = D3D11_FEATURE_DATA_D3D11_OPTIONS3::default();
    let ok = unsafe {
        device.CheckFeatureSupport(
            D3D11_FEATURE_D3D11_OPTIONS3,
            std::ptr::from_mut(&mut options).cast(),
            std::mem::size_of::<D3D11_FEATURE_DATA_D3D11_OPTIONS3>() as u32,
        )
    };
    let capability = if ok.is_ok()
        && options
            .VPAndRTArrayIndexFromAnyShaderFeedingRasterizer
            .as_bool()
    {
        Capability::Supported
    } else {
        Capability::Unsupported
    };
    CAPABILITY.store(capability as u8, Ordering::Relaxed);
    capability
}

/// Probe the capability using the live engine device, if one is available and the probe has not run
/// yet. Returns the (now cached) result. Called from the debug UI and the frame driver so the probe
/// happens as soon as a device exists.
pub fn probe_if_needed() -> Capability {
    let cached = capability();
    if cached != Capability::Unprobed {
        return cached;
    }
    // SAFETY: `GraphicsEngine::get` returns the live singleton or `None`; the device pointer is stable
    // once the engine has initialised.
    let Some(device) = (unsafe { GraphicsEngine::get() }) else {
        return Capability::Unprobed;
    };
    let Some(device) = (unsafe { device.m_Device.as_ref() }) else {
        return Capability::Unprobed;
    };
    probe(&device.m_Device)
}

/// The cached capability-probe result.
pub fn capability() -> Capability {
    match CAPABILITY.load(Ordering::Relaxed) {
        x if x == Capability::Supported as u8 => Capability::Supported,
        x if x == Capability::Unsupported as u8 => Capability::Unsupported,
        _ => Capability::Unprobed,
    }
}

/// Vertex-shader name prefixes whose no-`cb0` shaders are reprojected for single-pass: the scene
/// geometry that writes clip as `scene-VP · world` (skinned characters/NPCs, static and dynamic
/// models, roads). NDC writers -- sky, UI, post, particles, water -- are deliberately absent, so they
/// stay double-drawn; `M_eye` would corrupt them and the bytecode can't tell them apart. Terrain and
/// vegetation are absent too: the terrain VS writes no position (its clip is built in the domain
/// shader) and vegetation is GPU-indirect, both separate phases. Names come from
/// `CreateVertexProgramParams.m_Name`; matched by prefix to cover each family's permutations.
const REPROJECT_NAME_PREFIXES: &[&str] = &[
    // Skinned and rigid characters, creatures (the NPCs).
    "character",
    "creature",
    // Static and dynamic scene models.
    "prop",
    "general",
    "buildingjc3",
    "buildingrsm",
    "window",
    "materialtune",
    "open",
    "flag",
    "snow",
    "skidmarks",
    // Roads.
    "junctionroad",
    "splineroad",
    "dirtroad",
];

/// Whether a no-`cb0` vertex shader named `name` should be reprojected for single-pass: the
/// Vertex-shader name prefixes of the far-distance tree impostors (`CTreeImpostorRB`), gated by the
/// separate [`single_pass_tree_impostors`](crate::config::StereoConfig::single_pass_tree_impostors)
/// flag. The impostor VS writes `SV_Position` from the global billboard view-projection and draws a
/// single non-instanced `DrawIndexed` -- no GPU-indirect path shares it -- so the same reprojection
/// rewrite the scene families take covers it completely. The other vegetation families
/// (`vegetationfoliage*`, `vegetationbark*`, `grass`, `leaves`) are deliberately absent: their dominant
/// draw is GPU-indirect and shares the VS, so reprojecting it would break the indirect path -- they need
/// the coordinated indirect handling (see `docs/mod/single-pass-render-blocks.md`).
const VEGETATION_REPROJECT_NAME_PREFIXES: &[&str] = &["treeimpostor"];

/// Whether a no-`cb0` vertex shader named `name` should be reprojected for single-pass: either the
/// `single_pass_reproject` flag is on and the name is on [`REPROJECT_NAME_PREFIXES`], or the
/// `single_pass_tree_impostors` flag is on and the name is on [`VEGETATION_REPROJECT_NAME_PREFIXES`].
/// Called from the `CreateVertexProgram` hook when `patch_vertex_shader` reports no per-eye `cb0`
/// operands.
pub fn should_reproject(name: Option<&str>) -> bool {
    let Some(name) = name else {
        return false;
    };
    let (reproject, tree_impostors) = Config::lock_query(|c| {
        (
            c.stereo.single_pass_reproject,
            c.stereo.single_pass_tree_impostors,
        )
    });
    (reproject && REPROJECT_NAME_PREFIXES.iter().any(|p| name.starts_with(p)))
        || (tree_impostors
            && VEGETATION_REPROJECT_NAME_PREFIXES
                .iter()
                .any(|p| name.starts_with(p)))
}

/// Vertex-shader name prefixes of the tessellated base terrain, whose VS originates the single-pass
/// eye index on the free `TEXCOORD3.z` lane (it writes no `SV_Position`, so it takes neither the `cb0`
/// remap nor the reprojection). The hull and domain shaders that pair with these are transformed
/// structurally -- gated on the transform succeeding, not by name -- since they are created through
/// separate calls that do not carry a paired-VS identity. Names come from `CreateVertexProgramParams.m_Name`.
const TERRAIN_VS_NAME_PREFIXES: &[&str] = &[
    "volumetricterrain",
    "terrainscroller",
    "terrainshaderforest",
    "controlpoint",
    // `terraindetailrt*` is deliberately absent: the terrain-detail render block is GPU-indirect and is
    // reprojected per-eye by a render-block intercept (`terrain_detail_per_eye`) that rebuilds its `cb1`,
    // so its vertex shader must stay pristine -- reprojecting it here would double-transform.
];

/// Whether a no-`cb0` vertex shader named `name` should be eye-injected for the single-pass terrain
/// path: the `single_pass_terrain` config flag is on and the name is on [`TERRAIN_VS_NAME_PREFIXES`].
/// Called from the `CreateVertexProgram` hook when `patch_vertex_shader` reports no per-eye `cb0`
/// operands and the name is not a reprojection candidate.
pub fn should_eye_inject(name: Option<&str>) -> bool {
    let Some(name) = name else {
        return false;
    };
    Config::lock_query(|c| c.stereo.single_pass_terrain)
        && TERRAIN_VS_NAME_PREFIXES.iter().any(|p| name.starts_with(p))
        && !is_terrain_shadow_pass(Some(name))
}

/// Whether a terrain shader named `name` is a shadow-pass variant. Shadow passes render the terrain
/// from the light's view into the shadow atlas, not per eye, so the single-pass eye transforms
/// (reprojection and viewport routing) must skip them: eye-transforming a shadow-pass draw corrupts
/// the shadow map, dropping large areas into shadow and blacking out the geometry that samples it.
///
/// The substring is the engine's own naming convention, readable in the shader bundle's name table:
/// the terrain families that have a shadow permutation spell it out --
/// `volumetricterrain4shadow`, `volumetricterrain4shadowblend[instanced]`,
/// `volumetricterrain4notessellationshadow*`, `terrainshaderforestshadow`, `terrainshadowsimple` --
/// and no non-shadow terrain permutation contains it (`terrainshaderforest` is the near miss, and it
/// is "shader", not "shadow").
///
/// An **unnamed** shader counts as a shadow pass. The name is the only thing distinguishing the two,
/// and the hull and domain hooks are reached without a paired-VS identity, so failing closed costs a
/// terrain draw its single-pass treatment (it stays double-drawn -- correct, just slower) where
/// failing open would silently eye-transform a shadow-atlas draw.
pub fn is_terrain_shadow_pass(name: Option<&str>) -> bool {
    name.is_none_or(|n| n.contains("shadow"))
}

/// Whether the single-pass terrain path is live: single-pass is [`active`] and the `single_pass_terrain`
/// flag is on. Gates the hull-forward and domain-reproject substitutions in the shader-creation hooks.
pub fn terrain_active() -> bool {
    active() && Config::lock_query(|c| c.stereo.single_pass_terrain)
}

/// Record that a terrain hull shader's eye lane was forwarded (its `TEXCOORD3.z` widened), for the
/// debug UI's is-the-terrain-path-catching-anything readout.
pub fn record_hull_forwarded() {
    TERRAIN_HS_FORWARDED.fetch_add(1, Ordering::Relaxed);
}

/// Record that a terrain domain shader was reprojected, for the debug UI.
pub fn record_domain_reprojected() {
    TERRAIN_DS_REPROJECTED.fetch_add(1, Ordering::Relaxed);
}

/// The number of terrain hull shaders forwarded and domain shaders reprojected since injection (reset
/// on a shader reload alongside the vertex census).
pub fn terrain_counts() -> (usize, usize) {
    (
        TERRAIN_HS_FORWARDED.load(Ordering::Relaxed),
        TERRAIN_DS_REPROJECTED.load(Ordering::Relaxed),
    )
}

/// Record the outcome of running [`dxbc_stereo::patch_vertex_shader`] on one vertex shader, for the
/// census the debug UI reports. Classifies into four buckets: successfully patched; no per-eye
/// references (the baked-WVP / no-position families left double-drawn -- expected); the
/// `SV_InstanceID`-already-declared deferral (shaders that instance themselves, whose `>> 1` consumer
/// rewrite is a later phase -- also expected, left double-drawn); and genuinely errored (an
/// unexpected shape the rewriter could not handle -- worth investigating, should be zero).
pub fn record_patch_outcome(outcome: &Result<Vec<u8>, DxbcError>, name: Option<&str>) {
    let (counter, class) = match outcome {
        Ok(_) => (&PATCHED, PatchClass::Patched),
        Err(DxbcError::NoPerEyeReferences) => (&NO_REFS, PatchClass::NoRefs),
        Err(DxbcError::InstanceIdAlreadyDeclared) => (&DEFERRED, PatchClass::Deferred),
        Err(_) => (&ERRORED, PatchClass::Errored),
    };
    counter.fetch_add(1, Ordering::Relaxed);
    if DUMP_VS_NAME_CENSUS && let Some(name) = name {
        VS_NAME_CENSUS.lock().insert(name.to_string(), class);
    }
}

/// Whether to record every vertex shader's name against its rewrite class and dump the result to the
/// session directory on a shader reload (see [`dump_vs_name_census`]). Off: the reprojection allowlist
/// ([`REPROJECT_NAME_PREFIXES`]) is baked in and the census file lives in `docs/mod/single-pass-stereo.md`.
/// Flip to `true` and rebuild to re-census -- e.g. to catch scene shaders an area didn't load the first
/// time (the census only sees shaders created while it runs).
const DUMP_VS_NAME_CENSUS: bool = false;

/// The rewrite outcome class of a vertex shader, tracked per shader name in [`VS_NAME_CENSUS`] so the
/// name census can group the reprojection candidates (no-per-eye-refs) apart from the `cb0`-remap set.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PatchClass {
    Patched,
    NoRefs,
    Deferred,
    Errored,
}

/// Every censused vertex shader's name and its rewrite class (populated only while
/// [`DUMP_VS_NAME_CENSUS`] is on), dumped to `vs-name-census.txt` on a shader reload to build the
/// reprojection allowlist from real data.
static VS_NAME_CENSUS: Mutex<BTreeMap<String, PatchClass>> = Mutex::new(BTreeMap::new());

/// Write the vertex-shader name census (see [`VS_NAME_CENSUS`]) to the session directory, grouped by
/// rewrite class with the reprojection candidates first. A no-op unless [`DUMP_VS_NAME_CENSUS`] is on
/// (the census is empty otherwise). Called after a shader reload, once the bounce has re-created every
/// shader through the census hook.
pub fn dump_vs_name_census() {
    let census = VS_NAME_CENSUS.lock();
    if census.is_empty() {
        return;
    }
    let Some(dir) = crate::session::dir() else {
        return;
    };
    let mut out = String::new();
    for (label, class) in [
        (
            "no-per-eye-refs (reprojection candidates)",
            PatchClass::NoRefs,
        ),
        ("patched (cb0 remap)", PatchClass::Patched),
        ("instance-id deferred", PatchClass::Deferred),
        ("errored", PatchClass::Errored),
    ] {
        let names: Vec<&str> = census
            .iter()
            .filter(|(_, c)| **c == class)
            .map(|(n, _)| n.as_str())
            .collect();
        out.push_str(&format!("## {label} -- {}\n", names.len()));
        for name in names {
            out.push_str(name);
            out.push('\n');
        }
        out.push('\n');
    }
    let path = dir.join("vs-name-census.txt");
    match std::fs::write(&path, out) {
        Ok(()) => tracing::info!("vs name census -> {}", path.display()),
        Err(e) => tracing::warn!("vs name census: failed to write {}: {e}", path.display()),
    }
}

/// Vertex shaders successfully rewritten for single-pass since injection.
pub fn patched_count() -> usize {
    PATCHED.load(Ordering::Relaxed)
}

/// Vertex shaders with no per-eye `cb0` references -- the baked-WVP / no-position families left
/// double-drawn. Expected, not a failure.
pub fn no_refs_count() -> usize {
    NO_REFS.load(Ordering::Relaxed)
}

/// Vertex shaders left double-drawn because they already declare an `SV_InstanceID` input; their
/// `>> 1` consumer rewrite is a later phase. Expected, not a failure.
pub fn deferred_count() -> usize {
    DEFERRED.load(Ordering::Relaxed)
}

/// Vertex shaders the rewriter could not handle for an unexpected reason (a shape it does not yet
/// support). A non-zero count flags shaders to investigate -- the offline corpus reports zero.
pub fn errored_count() -> usize {
    ERRORED.load(Ordering::Relaxed)
}

/// Reset the census counters (on a shader reload, so the reported numbers reflect one clean pass over
/// the shader set rather than accumulating across reloads).
pub fn reset_census() {
    PATCHED.store(0, Ordering::Relaxed);
    NO_REFS.store(0, Ordering::Relaxed);
    DEFERRED.store(0, Ordering::Relaxed);
    ERRORED.store(0, Ordering::Relaxed);
    TERRAIN_HS_FORWARDED.store(0, Ordering::Relaxed);
    TERRAIN_DS_REPROJECTED.store(0, Ordering::Relaxed);
    VS_NAME_CENSUS.lock().clear();
}

/// Whether single-pass rendering should actually run this frame: the master switch is on, the
/// census-only dry-run is off, and the device supports viewport routing. The VS-substitution and
/// cb13 paths gate on this; when it is false the double-draw path is left untouched.
pub fn active() -> bool {
    // Go inert the instant eject begins: the render thread keeps running through the whole teardown,
    // so an ungated single-pass path would race the hook uninstall and the D3D-resource release (the
    // same crash-on-uninject class already fixed for `vr::update` -- see `crate::is_shutting_down`).
    if crate::is_shutting_down() {
        return false;
    }
    let (single_pass, dry_run) =
        Config::lock_query(|c| (c.stereo.single_pass, c.stereo.single_pass_patch_dryrun));
    single_pass && !dry_run && capability() == Capability::Supported
}

/// Whether the eyes are made to diverge (in addition to [`active`]): distinct per-eye `cb13`,
/// left/right-half viewport routing, and instance doubling of the G-buffer geometry. With it off the
/// patched shaders still run, but both `cb13` eye slots hold the same view, so the two eyes render
/// identically -- the shape the substitution was brought up in, and still the fallback whenever
/// [`compute_dual_eye_rows`] cannot produce per-eye data.
pub fn dual_eye_active() -> bool {
    active() && Config::lock_query(|c| c.stereo.single_pass_dual_eye)
}

/// Whether the per-eye double-draw has been collapsed to a single G-buffer walk: one `game.Draw`
/// produces both eyes (via [`dual_eye_active`]'s `cb13` + viewport routing + instance doubling), the
/// render camera stays centered (no per-eye offset -- both eyes come from `cb13`), and the capture
/// splits the one back buffer into the two eye textures. Requires [`dual_eye_active`]; independent of
/// `single_pass_double_wide`, which only upgrades each eye-half from squished to full resolution.
pub fn collapse_active() -> bool {
    dual_eye_active() && Config::lock_query(|c| c.stereo.single_pass_collapse)
}

/// Whether the scene render targets are re-created at 2x per-eye width so each eye-half is full
/// resolution (instead of a squished half of a per-eye-sized target). Requires [`collapse_active`] --
/// it only makes sense for the single walk whose capture split reads one full-width half per eye.
/// Drives the engine render resolution ([`crate::vr::engine_render_resolution`]) and the per-eye
/// capture-texture width (`ui::render`); the XR swapchain stays per-eye width.
pub fn double_wide_active() -> bool {
    collapse_active() && Config::lock_query(|c| c.stereo.single_pass_double_wide)
}

/// The eye whose viewport + view-projection the collapse UI overlays (HUD panel, egui panel) should
/// currently draw with, so a head/world-locked quad lands at the correct 3D spot in each eye instead
/// of being drawn once, stretched, across the double-wide target. Set around each eye's overlay draw
/// by `render_engine_post_draw`; [`NO_UI_EYE`] means "not drawing a collapse overlay".
static COLLAPSE_UI_EYE: AtomicUsize = AtomicUsize::new(NO_UI_EYE);
const NO_UI_EYE: usize = usize::MAX;

/// Select the eye for the collapse UI overlay draws (`Some(0)`/`Some(1)`), or clear it (`None`).
pub fn set_collapse_ui_eye(eye: Option<usize>) {
    COLLAPSE_UI_EYE.store(eye.unwrap_or(NO_UI_EYE), Ordering::Relaxed);
}

/// The eye-half viewport and per-eye **full** view-projection for the current collapse UI overlay
/// draw, or `None` when not drawing one (or not collapsed). The HUD/egui-panel quad renderer uses
/// this to draw each overlay into one eye's half with that eye's own VP.
pub fn collapse_ui_eye_override() -> Option<(D3D11_VIEWPORT, Matrix4)> {
    let eye = COLLAPSE_UI_EYE.load(Ordering::Relaxed);
    if eye == NO_UI_EYE || !collapse_active() {
        return None;
    }
    let full = (*COLLAPSE_FULL_VIEWPORT.lock())?;
    let half = full.Width / 2.0;
    let viewport = D3D11_VIEWPORT {
        TopLeftX: full.TopLeftX + eye as f32 * half,
        Width: half,
        ..full
    };
    Some((viewport, full_eye_view_projection(eye)?))
}

/// Set the immediate-context viewport via the original (un-detoured) `RSSetViewports`, so a mod
/// overlay can bind one eye's half of the double-wide target without the collapse viewport detour
/// dup'ing it to full width. `context` is the raw `ID3D11DeviceContext` pointer.
pub fn set_ui_viewport_raw(context: *mut c_void, viewport: &D3D11_VIEWPORT) {
    if let Some(detour) = RS_SET_VIEWPORTS.get() {
        // SAFETY: `context` is the live immediate context; the trampoline is the original function.
        unsafe { detour.call(context, 1, std::slice::from_ref(viewport).as_ptr()) };
    }
}

/// One eye's re-issue of a terrain-detail draw: the reprojected `cb1` (four float4 rows) to stage on
/// vertex slot 1, and the eye-half viewport to render into.
struct TerrainDetailEyePass {
    cb1: [f32; 16],
    viewport: D3D11_VIEWPORT,
}

/// The per-eye passes for a terrain-detail draw, or `None` when the single-pass terrain intercept
/// should not run -- the same gate every other per-eye re-issue takes
/// ([`baked_cb_intercept_ready`]: the collapse, the G-buffer range, and a published `M_eye` and
/// viewport), plus the terrain flag.
///
/// The detail draw is GPU-indirect, so it cannot be instance-doubled like the model geometry; instead
/// the render block's `Draw` is re-issued once per eye with a per-eye `cb1`. The detail VS builds clip
/// with a multiply-add chain over `cb1[0..3]` (`clip = Σ_i P_local[i] · cb1[i]`), so those four
/// registers are the *columns* of `T_patch · OffsetVP` (`T_patch` translating the patch origin relative
/// to the camera) and the per-eye buffer is the column-wise `cb1_eye[k] = M_eye · cb1_center[k]`.
/// `cb1[4]` (the LOD-fade position) is left untouched. `this` and `rc` are the
/// [`RenderBlockTerrainDetail`] and [`RenderContext`] the block's `Draw` received.
///
/// # Safety
///
/// `this` and `rc` must be the live pointers the detoured `Draw` received.
unsafe fn terrain_detail_eye_passes(
    this: *const RenderBlockTerrainDetail,
    rc: *const RenderContext,
) -> Option<(
    [TerrainDetailEyePass; 2],
    D3D11_VIEWPORT,
    ID3D11DeviceContext,
)> {
    if !terrain_active() {
        return None;
    }
    let (m_eye, full, d3d) = baked_cb_intercept_ready()?;

    // SAFETY: caller guarantees live pointers.
    let ovp = unsafe { (*rc).m_OffsetViewProjection.data };
    let cam = unsafe { (*rc).m_CameraPosition.data };
    let (patch_x, patch_z) = unsafe { ((*this).m_WorldPatchX, (*this).m_WorldPatchZ) };

    let row =
        |r: usize| glam::Vec4::new(ovp[r * 4], ovp[r * 4 + 1], ovp[r * 4 + 2], ovp[r * 4 + 3]);
    let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
    // The engine stores `Matrix4` row-major, so `ovp` row `k` is column `k` of the column-vector
    // `OffsetVP` -- exactly the entry the VS's mad chain wants at `cb1[k]`. The fourth column folds in
    // the patch-relative camera translation `T_patch`.
    let (tx, ty, tz) = (patch_x - cam[0], -cam[1], patch_z - cam[2]);
    let cb1_center = [r0, r1, r2, r3 + tx * r0 + ty * r1 + tz * r2];

    let passes = std::array::from_fn(|eye| {
        let mut cb1 = [0.0f32; 16];
        for (k, center) in cb1_center.iter().enumerate() {
            cb1[k * 4..k * 4 + 4].copy_from_slice(&m_eye[eye].mul_vec4(*center).to_array());
        }
        TerrainDetailEyePass {
            cb1,
            viewport: eye_half_viewport(full, eye),
        }
    });
    Some((passes, full, d3d))
}

/// The graphics context (`HContext_t*`) a render block's `Draw` stages constants into, read from its
/// [`RenderContext`]. Used by the terrain-detail intercept to call `SetVertexProgramConstants`.
///
/// # Safety
///
/// `rc` must be a live pointer.
unsafe fn render_context_graphics_context(rc: *const RenderContext) -> *mut HContext_t {
    unsafe { (*rc).m_Context }
}

/// The engine's immediate `ID3D11DeviceContext`, or `None` if the device/context is not live yet.
fn immediate_context() -> Option<ID3D11DeviceContext> {
    // SAFETY: read on the render thread, where the engine device/context pointers are stable.
    unsafe {
        let ge = GraphicsEngine::get()?;
        let device = ge.m_Device.as_ref()?;
        let context = device.m_Context.as_ref()?;
        Some(context.m_Context.clone())
    }
}

/// Bind `viewport` to both viewport slots of the immediate context. Binding two slots (rather than
/// one) passes the collapse viewport detour through untouched -- it only special-cases a single-slot
/// set -- and the terrain-detail VS has no `SV_ViewportArrayIndex`, so it rasterizes into slot 0.
fn bind_both_viewport_slots(d3d: &ID3D11DeviceContext, viewport: D3D11_VIEWPORT) {
    // SAFETY: `d3d` is the live immediate context; a two-element slice is a valid viewport array.
    unsafe { d3d.RSSetViewports(Some(&[viewport, viewport])) };
}

/// Re-issue a terrain-detail `Draw` once per eye with a per-eye `cb1` and the eye's half-viewport,
/// calling `draw` (the block's original `Draw` trampoline) each time. Returns `false` when the
/// single-pass terrain intercept should not run, in which case the caller draws normally once. The
/// detail draw is GPU-indirect and so cannot be instance-doubled; this drives per-eye rendering from
/// the CPU instead. See [`terrain_detail_eye_passes`].
///
/// # Safety
///
/// `this` and `rc` must be the live pointers the detoured `Draw` received, and `draw` must invoke the
/// original `Draw`.
pub unsafe fn terrain_detail_per_eye(
    this: *const RenderBlockTerrainDetail,
    rc: *const RenderContext,
    mut draw: impl FnMut(),
) -> bool {
    let Some((passes, full, d3d)) = (unsafe { terrain_detail_eye_passes(this, rc) }) else {
        return false;
    };
    // SAFETY: `rc` is live per the caller contract.
    let ctx = unsafe { render_context_graphics_context(rc) };
    for pass in &passes {
        // SAFETY: `ctx` is the render context's live graphics context; `cb1` is four float4 rows.
        unsafe { SetVertexProgramConstants(ctx, 1, 0, pass.cb1.as_ptr(), 4) };
        bind_both_viewport_slots(&d3d, pass.viewport);
        draw();
    }
    // Restore the collapse's full viewport for the draws that follow in this pass.
    bind_both_viewport_slots(&d3d, full);
    true
}

// ---- Baked-cb per-eye re-issue -----------------------------------------------------------------
//
// The generalization of the terrain-detail intercept above, for the render blocks (bark, foliage,
// occluder) that bake their view-projection into a constant buffer inside their own `Draw` -- across
// draw kinds that cannot be instance-doubled (CPU-instanced, GPU-indirect). Rather than replicate each
// block's bake, [`reproject_baked_cb_per_eye`] re-issues the block's whole `Draw` once per eye and, for
// the duration of each call, arms [`set_vertex_program_constants_detour`] to post-multiply the block's
// own constant upload by that eye's `M_eye`.

/// A pending reprojection of a render block's baked view-projection constant, armed around a per-eye
/// re-issue. While armed, the game's stage of the four `float4` entries at (`cb_index`, `reg_offset`)
/// -- the columns of the baked matrix -- is reprojected by `m_eye`.
#[derive(Clone, Copy)]
struct ReprojectUpload {
    cb_index: i32,
    reg_offset: u32,
    m_eye: glam::Mat4,
}

/// Fast-path guard for [`set_vertex_program_constants_detour`]: a relaxed load skips the mutex on every
/// un-armed stage (the common case -- the detour sees every VS constant upload in the frame).
static REPROJECT_ARMED: AtomicBool = AtomicBool::new(false);
static REPROJECT_UPLOAD: Mutex<Option<ReprojectUpload>> = Mutex::new(None);

fn arm_reproject(upload: ReprojectUpload) {
    *REPROJECT_UPLOAD.lock() = Some(upload);
    REPROJECT_ARMED.store(true, Ordering::Release);
}

fn disarm_reproject() {
    REPROJECT_ARMED.store(false, Ordering::Release);
    *REPROJECT_UPLOAD.lock() = None;
}

/// The eye-half of `full` for eye `e` (left = 0, right = 1).
fn eye_half_viewport(full: D3D11_VIEWPORT, eye: usize) -> D3D11_VIEWPORT {
    let half = full.Width / 2.0;
    D3D11_VIEWPORT {
        TopLeftX: full.TopLeftX + eye as f32 * half,
        Width: half,
        ..full
    }
}

/// The state a baked-cb per-eye re-issue needs, or `None` when it must not run: the two per-eye `M_eye`
/// matrices, the collapse full viewport, and the immediate context. Requires the collapse (a single
/// centered walk) and the G-buffer pass range -- outside the range the eye-half split does not apply
/// (the shadow-cascade and reflection passes reuse these blocks' `DrawZ`, and eye-splitting a
/// shadow-atlas draw would corrupt it), and outside the collapse re-issuing per eye is wrong.
fn baked_cb_intercept_ready() -> Option<([glam::Mat4; 2], D3D11_VIEWPORT, ID3D11DeviceContext)> {
    if !collapse_active() || !in_gbuffer_range() {
        return None;
    }
    let m_eye = (*CURRENT_M_EYE.lock())?;
    let full = (*COLLAPSE_FULL_VIEWPORT.lock())?;
    let d3d = immediate_context()?;
    Some((m_eye, full, d3d))
}

/// Re-issue a render block's `Draw` once per eye, reprojecting the four `float4` entries the block bakes
/// at (`cb_index`, `reg_offset`) by that eye's `M_eye` and binding the eye's half-viewport. Returns `false`
/// when the intercept must not run (collapse inactive, or the dual-eye state not yet published), in which
/// case the caller draws normally once.
///
/// The block writes its view-projection into a constant buffer inside its own `Draw`, so rather than
/// replicate that bake, this arms [`set_vertex_program_constants_detour`] to reproject the block's own
/// upload for the duration of a wrapped original-`Draw` call. It covers every draw kind (plain,
/// CPU-instanced, GPU-indirect) uniformly, since it re-drives the block's whole `Draw` -- the block's
/// vertex shader stays pristine (unpatched), so the draw-doubling detour leaves it single, and each of
/// the two re-issues renders into its eye's viewport half.
///
/// # Safety
///
/// `draw` must invoke the block's original `Draw` trampoline.
pub unsafe fn reproject_baked_cb_per_eye(
    cb_index: i32,
    reg_offset: u32,
    mut draw: impl FnMut(),
) -> bool {
    let Some((m_eye, full, d3d)) = baked_cb_intercept_ready() else {
        return false;
    };
    for (eye, &m) in m_eye.iter().enumerate() {
        arm_reproject(ReprojectUpload {
            cb_index,
            reg_offset,
            m_eye: m,
        });
        bind_both_viewport_slots(&d3d, eye_half_viewport(full, eye));
        draw();
        disarm_reproject();
    }
    bind_both_viewport_slots(&d3d, full);
    true
}

type SetVertexProgramConstantsFn =
    unsafe extern "system" fn(*mut c_void, i32, u32, *const f32, u32);
static SET_VERTEX_PROGRAM_CONSTANTS: OnceLock<GenericDetour<SetVertexProgramConstantsFn>> =
    OnceLock::new();

/// Detour on `Graphics::SetVertexProgramConstants`. While a baked-cb per-eye re-issue is armed (see
/// [`reproject_baked_cb_per_eye`]), reproject the four `float4` entries at the armed (`cb_index`,
/// `reg_offset`) by the armed `M_eye` before the engine stages them, so the block's own
/// view-projection upload becomes that eye's. Every other stage -- un-armed, a different slot, or a
/// range that does not contain the target entries -- passes through unchanged.
///
/// The transform is applied entry-wise (`M_eye · cb[k]`) because the vertex shaders that consume these
/// registers build clip with a multiply-add chain (`clip = Σ_i p_i · cb[k+i]`) rather than four `dp4`s:
/// each register is a *column* of the baked matrix, not a row. Confirmed against the bundle's Bark,
/// Foliage and Occluder vertex shaders; see `docs/mod/single-pass-render-blocks.md`. (`cb13`'s own
/// `M_eye` block is the opposite convention -- the rewriter's epilogue *is* a `dp4` chain -- so
/// [`write_meye`] stores rows there.)
unsafe extern "system" fn set_vertex_program_constants_detour(
    ctx: *mut c_void,
    cb_index: i32,
    start_offset: u32,
    data: *const f32,
    count: u32,
) {
    let detour = SET_VERTEX_PROGRAM_CONSTANTS
        .get()
        .expect("set before enable");
    if REPROJECT_ARMED.load(Ordering::Acquire)
        && !data.is_null()
        && let Some(up) = *REPROJECT_UPLOAD.lock()
        && cb_index == up.cb_index
        && start_offset <= up.reg_offset
        && up.reg_offset + 4 <= start_offset + count
    {
        let n = count as usize * 4;
        let mut buf = vec![0.0f32; n];
        // SAFETY: the caller stages `count` float4 rows = `n` floats from `data`.
        unsafe { std::ptr::copy_nonoverlapping(data, buf.as_mut_ptr(), n) };
        let base = (up.reg_offset - start_offset) as usize * 4;
        for k in 0..4 {
            let column = glam::Vec4::from_slice(&buf[base + k * 4..base + k * 4 + 4]);
            buf[base + k * 4..base + k * 4 + 4]
                .copy_from_slice(&up.m_eye.mul_vec4(column).to_array());
        }
        // SAFETY: `buf` holds `n` floats and outlives the call; `detour.call` is the trampoline.
        unsafe { detour.call(ctx, cb_index, start_offset, buf.as_ptr(), count) };
        return;
    }
    // SAFETY: forwards the original arguments to the trampoline.
    unsafe { detour.call(ctx, cb_index, start_offset, data, count) };
}

/// The per-eye **full** (translation-carrying) world→clip view-projection, matching the render
/// camera's `m_ViewProjectionF` for that eye -- for projecting the mod's world-space overlay quads
/// per eye in the collapse, where the render camera stays centered. `None` if the centre transform or
/// per-eye params are unavailable.
fn full_eye_view_projection(eye: usize) -> Option<Matrix4> {
    let center_transform = crate::stereo::STEREO_STATE.lock().center_transform?;
    let params = crate::vr::render_params(eye)?;
    let mut eye_world = glam::Mat4::from(center_transform);
    eye_world.w_axis += params.world_offset.extend(0.0);
    let eye_world = eye_world * glam::Mat4::from_quat(params.orientation_delta);
    let vp = glam::Mat4::from(params.projection_reverse_z) * eye_world.inverse();
    Some(Matrix4::from(vp))
}

/// Marks whether the render thread is currently inside the G-buffer geometry pass range
/// (`RP_Z_OCCLUDERS..RP_FIRST_SCENE`), set around that `DrawRenderPassRange` call. The dual-eye
/// viewport split and instance doubling apply only here -- so shadow/lighting/post passes, which
/// reuse the same patched shaders but are not double-wide, keep the identical-viewport behaviour.
pub fn set_gbuffer_range(inside: bool) {
    IN_GBUFFER_RANGE.store(inside, Ordering::Relaxed);
    if !inside {
        // The per-eye matrices belong to the range that just ended. Dropping them means a re-issue
        // that somehow runs outside a range -- or in a later frame where `compute_dual_eye_rows`
        // declined to publish -- reprojects with nothing rather than with last frame's head pose.
        *CURRENT_M_EYE.lock() = None;
    }
}

fn in_gbuffer_range() -> bool {
    IN_GBUFFER_RANGE.load(Ordering::Relaxed)
}

/// The stereo constant buffer's register slot (`b13`, free across the game's vertex shaders) and its
/// size in float4 rows (five per eye: four view-projection rows then the camera position, two eyes).
const STEREO_CB_REGISTER: u32 = 13;
/// The `cb0`-remap block: five rows per eye (`dxbc_stereo::STEREO_CB_ROWS`).
const STEREO_CB_ROWS: usize = 10;
/// The row where the reprojection `M_eye` block begins (`dxbc_stereo::MEYE_ROW_BASE`).
const MEYE_ROW_BASE: usize = 10;
/// The full `cb13` size: the remap block plus a four-rows-per-eye `M_eye` block for the reprojection
/// rewrite (`dxbc_stereo::STEREO_REPROJ_CB_ROWS`). Both idioms bind the same `b13` buffer.
const STEREO_CB_TOTAL_ROWS: usize = 18;

// Keep the payload's cb13 layout in lockstep with the rewriter that reads it.
const _: () = {
    assert!(STEREO_CB_ROWS == dxbc_stereo::STEREO_CB_ROWS as usize);
    assert!(MEYE_ROW_BASE == dxbc_stereo::MEYE_ROW_BASE as usize);
    assert!(STEREO_CB_TOTAL_ROWS == dxbc_stereo::STEREO_REPROJ_CB_ROWS as usize);
    assert!(STEREO_CB_REGISTER == dxbc_stereo::STEREO_CB_REGISTER);
};

/// The `cb0` (`m_VPGlobalConstData`) rows the patched shaders read per eye, in the order the rewrite
/// lays them out in `cb13`: the four translation-free view-projection rows (`cb0[29..32]`), then the
/// camera world position (`cb0[4]`). See `dxbc_stereo::PER_EYE_CB0_ROWS`.
const PER_EYE_SOURCE_ROWS: [usize; 5] = [29, 30, 31, 32, 4];

/// Mirror the current view's per-eye `cb0` rows into the mod-owned `cb13` and bind it at `b13`.
///
/// Outside the diverging case both eye slots get the **same** (current-view) rows, so a patched
/// vertex shader -- which reads its position from `cb13` instead of `cb0` -- renders exactly what it
/// would have from `cb0`, in *every* pass (the G-buffer, but also the shadow and reflection passes
/// that reuse the same model shaders under a different view). That shadow-safety is why `cb13` tracks
/// whatever view is current rather than being written once.
///
/// Called from the `SetAllGlobalShaderProgramConstants` detour, after the engine has refreshed
/// `m_VPGlobalConstData` and uploaded `cb0`, on the render thread.
pub fn mirror_and_bind_cb13(engine: &RenderEngine) {
    // Ensure the viewport-duplication detours are installed (once, on the first active frame).
    ensure_viewport_detours();

    // During the main-scene G-buffer range, fill the two eye slots with *distinct* per-eye
    // view-projections so the eyes diverge. Everywhere else (the shadow and reflection passes, and
    // whenever divergence is off) mirror the current view into both slots -- diverging those would be
    // wrong, since they render from the sun or reflection camera, not the eye camera.
    let rows = if dual_eye_active() && in_gbuffer_range() {
        compute_dual_eye_rows(engine).unwrap_or_else(|| mirror_rows(engine))
    } else {
        mirror_rows(engine)
    };

    // SAFETY: `GraphicsEngine::get` returns the live singleton or `None`; the device/context pointers
    // are stable once the engine has initialised, and the ops run under the engine's context mutex.
    unsafe {
        let Some(ge) = GraphicsEngine::get() else {
            return;
        };
        let Some(device) = ge.m_Device.as_ref() else {
            return;
        };
        let Some(context) = device.m_Context.as_ref() else {
            return;
        };
        EnterCriticalSection(context.m_Mutex);
        let result = CB13
            .lock()
            .upload_and_bind(&device.m_Device, &context.m_Context, &rows);
        LeaveCriticalSection(context.m_Mutex);
        if let Err(e) = result {
            tracing::warn!("single-pass cb13: {e}");
        }
    }
}

/// Mirror the current view's per-eye `cb0` rows into both `cb13` eye slots (the non-scene passes, and
/// any frame where divergence is off): a patched shader then renders exactly what it would from
/// `cb0`. The `M_eye` reprojection block is left at identity, so a reprojected shader is a no-op here
/// too.
fn mirror_rows(engine: &RenderEngine) -> [Vector4; STEREO_CB_TOTAL_ROWS] {
    let vp = &engine.m_VPGlobalConstData;
    let mut rows = [Vector4::default(); STEREO_CB_TOTAL_ROWS];
    for eye in 0..2 {
        for (k, &src) in PER_EYE_SOURCE_ROWS.iter().enumerate() {
            rows[eye * PER_EYE_SOURCE_ROWS.len() + k] = vp[src];
        }
        write_meye(&mut rows, eye, glam::Mat4::IDENTITY);
    }
    rows
}

/// Write eye `e`'s reprojection matrix into the `M_eye` block (`cb13[MEYE_ROW_BASE + 4*e ..]`), one
/// glam row per `cb13` row. The reprojection rewrite reads these with `dp4 o0.{xyzw}, cb13[row],
/// rClip`, so each `cb13` row must be a row of `M_eye` acting on the clip column vector.
fn write_meye(rows: &mut [Vector4; STEREO_CB_TOTAL_ROWS], eye: usize, m_eye: glam::Mat4) {
    for r in 0..4 {
        rows[MEYE_ROW_BASE + eye * 4 + r] = Vector4 {
            data: m_eye.row(r).to_array(),
        };
    }
}

/// Compute distinct per-eye `cb13` rows from the pristine center render-camera transform and the
/// per-eye [`EyeRenderParams`](crate::vr::frame::EyeRenderParams), replicating the double-draw's
/// per-eye camera math (`hooks/camera.rs`) purely in mod code -- so the single walk produces both
/// eyes. Returns `None` (falling back to the mirror) if the center transform or per-eye params are
/// not available this frame.
///
/// Per eye: offset the center world transform by the eye parallax + orientation delta, invert to a
/// view, zero its translation for the camera-relative OffsetVP, multiply by the reverse-Z eye
/// projection, and pair it with the eye's camera world position (`center campos + world_offset`).
/// The engine `Matrix4` <-> `glam::Mat4` bridge is a transpose, so the math is done in glam
/// column-vector form and converted back once (see the `Matrix4` doc-comment).
fn compute_dual_eye_rows(engine: &RenderEngine) -> Option<[Vector4; STEREO_CB_TOTAL_ROWS]> {
    let center_transform = crate::stereo::STEREO_STATE.lock().center_transform?;
    let center_world = glam::Mat4::from(center_transform);
    let center_campos = engine.m_VPGlobalConstData[4];

    // The reprojection `M_eye = VP_eye · VP_center⁻¹` needs the engine's *center* full view-projection
    // (world -> clip, column-vector) -- the one the baked-WVP shaders folded into their `cb1`. It is
    // `cb0[29..32]` (the translation-free OffsetVP, stored row-major so `glam::Mat4::from` -- which is
    // `from_cols_array` -- yields its transpose, the column-vector form) composed with the
    // camera-relative `−campos` translation. Inverted in f64: the reverse-Z VP is near-singular.
    let center_offset_vp = {
        let mut data = [0.0f32; 16];
        for r in 0..4 {
            data[r * 4..r * 4 + 4].copy_from_slice(&engine.m_VPGlobalConstData[29 + r].data);
        }
        glam::Mat4::from(Matrix4 { data })
    };
    let center_campos_v = glam::Vec3::new(
        center_campos.data[0],
        center_campos.data[1],
        center_campos.data[2],
    );
    let vp_center = center_offset_vp * glam::Mat4::from_translation(-center_campos_v);
    let vp_center_inv = vp_center.as_dmat4().inverse();

    let mut rows = [Vector4::default(); STEREO_CB_TOTAL_ROWS];
    let mut forwards = [glam::Vec3::ZERO; 2];
    let mut m_eyes = [glam::Mat4::IDENTITY; 2];
    for eye in 0..2 {
        let params = crate::vr::render_params(eye)?;

        let mut eye_world = center_world;
        eye_world.w_axis += params.world_offset.extend(0.0);
        let eye_world = eye_world * glam::Mat4::from_quat(params.orientation_delta);
        // Camera forward is -Z of the world transform; kept for the divergence diagnostic below.
        forwards[eye] = (-eye_world.z_axis.truncate()).normalize_or_zero();

        let mut offset_view = eye_world.inverse();
        offset_view.w_axis = glam::Vec4::new(0.0, 0.0, 0.0, 1.0);

        let offset_vp_glam = glam::Mat4::from(params.projection_reverse_z) * offset_view;
        let offset_vp = Matrix4::from(offset_vp_glam);

        for r in 0..4 {
            rows[eye * 5 + r] = Vector4 {
                data: [
                    offset_vp.data[r * 4],
                    offset_vp.data[r * 4 + 1],
                    offset_vp.data[r * 4 + 2],
                    offset_vp.data[r * 4 + 3],
                ],
            };
        }
        let eye_campos = glam::Vec3::new(
            center_campos.data[0] + params.world_offset.x,
            center_campos.data[1] + params.world_offset.y,
            center_campos.data[2] + params.world_offset.z,
        );
        rows[eye * 5 + 4] = Vector4 {
            data: [
                eye_campos.x,
                eye_campos.y,
                eye_campos.z,
                center_campos.data[3],
            ],
        };

        // M_eye maps this eye's own centre-clip to eye-clip: build the eye's full VP the same way as
        // the centre (offset VP composed with −campos) and post-compose the centre's inverse.
        let vp_eye = offset_vp_glam * glam::Mat4::from_translation(-eye_campos);
        let m_eye = (vp_eye.as_dmat4() * vp_center_inv).as_mat4();
        write_meye(&mut rows, eye, m_eye);
        m_eyes[eye] = m_eye;
    }
    // Publish the per-eye reprojection matrices for the render-block-level intercepts (terrain detail),
    // which apply `M_eye` on the CPU to a per-draw constant buffer rather than through a rewritten shader.
    *CURRENT_M_EYE.lock() = Some(m_eyes);

    // Diagnostic (rate-limited): the angle between the two eyes' forward vectors. A stereo pair should
    // diverge only by the display cant (a few degrees on the Index) -- a large value means a per-eye
    // matrix bug rather than the canted-runtime views that merely look divergent on a flat capture.
    if let (Some(p0), Some(p1)) = (crate::vr::render_params(0), crate::vr::render_params(1)) {
        let diverge = forwards[0]
            .dot(forwards[1])
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees();

        // Flatten one eye's four `cb13` view-projection rows into a 16-float matrix.
        let vp16 = |base: usize| {
            let mut m = [0.0f32; 16];
            for r in 0..4 {
                m[r * 4..r * 4 + 4].copy_from_slice(&rows[base + r].data);
            }
            m
        };
        let eye_diag = |p: &crate::vr::EyeRenderParams, i: usize| EyeDiagnostics {
            world_offset: p.world_offset.to_array(),
            orientation_delta_quat: p.orientation_delta.to_array(),
            orientation_delta_deg: p.orientation_delta.to_axis_angle().1.to_degrees(),
            forward: forwards[i].to_array(),
            projection_reverse_z: p.projection_reverse_z.data,
            cb13_view_projection: vp16(i * 5),
            cb13_camera_position: rows[i * 5 + 4].data,
            cb13_m_eye: vp16(MEYE_ROW_BASE + i * 4),
        };
        let full_viewport = COLLAPSE_FULL_VIEWPORT.lock().map(|v| {
            [
                v.TopLeftX, v.TopLeftY, v.Width, v.Height, v.MinDepth, v.MaxDepth,
            ]
        });
        *LAST_FRAME_DIAG.lock() = Some(FrameDiagnostics {
            single_pass: active(),
            dual_eye: dual_eye_active(),
            collapse: collapse_active(),
            double_wide: double_wide_active(),
            capability: match capability() {
                Capability::Supported => "supported",
                Capability::Unsupported => "unsupported",
                Capability::Unprobed => "unprobed",
            },
            full_viewport,
            center_transform: center_transform.data,
            center_camera_position: center_campos.data,
            forward_divergence_deg: diverge,
            substitution: substitution_stats(),
            eyes: [eye_diag(&p0, 0), eye_diag(&p1, 1)],
        });

        if CB13_DIVERGE_LOG
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(240)
        {
            // Max |M_eye - I|: the reprojection matrix is near-identity for a small IPD and cant, so a
            // large deviation flags a construction bug (a wrong VP convention or a bad inverse).
            let meye_dev = |base: usize| {
                let m = vp16(base);
                (0..16)
                    .map(|k| (m[k] - if k % 5 == 0 { 1.0 } else { 0.0 }).abs())
                    .fold(0.0f32, f32::max)
            };
            tracing::info!(
                target: "single_pass",
                "cb13 eyes: fwd divergence={diverge:.2}deg | eye0 delta={:.2}deg off={:.4?} | eye1 delta={:.2}deg off={:.4?} | M_eye dev eye0={:.4} eye1={:.4}",
                p0.orientation_delta.to_axis_angle().1.to_degrees(), p0.world_offset,
                p1.orientation_delta.to_axis_angle().1.to_degrees(), p1.world_offset,
                meye_dev(MEYE_ROW_BASE), meye_dev(MEYE_ROW_BASE + 4),
            );
        }
    }

    Some(rows)
}

static CB13_DIVERGE_LOG: AtomicUsize = AtomicUsize::new(0);

/// A serializable snapshot of one eye's single-pass matrices, dumped in the F12 screenshot's JSON
/// sidecar so the exact `cb13` state can be inspected offline.
#[derive(Clone, serde::Serialize)]
pub struct EyeDiagnostics {
    /// The per-eye world position offset from the head centre (the IPD parallax), engine world units.
    pub world_offset: [f32; 3],
    /// The per-eye orientation delta from the head centre, as a quaternion `[x, y, z, w]`.
    pub orientation_delta_quat: [f32; 4],
    /// The magnitude of [`orientation_delta_quat`](Self::orientation_delta_quat), in degrees.
    pub orientation_delta_deg: f32,
    /// This eye's world-space view forward direction (`-Z` of the eye world transform).
    pub forward: [f32; 3],
    /// The per-eye reverse-Z projection from the runtime, row-major (engine `Matrix4` order).
    pub projection_reverse_z: [f32; 16],
    /// The translation-free offset view-projection written into `cb13` for this eye, row-major.
    pub cb13_view_projection: [f32; 16],
    /// The eye's world camera position written into `cb13` (`cb0[4]` equivalent).
    pub cb13_camera_position: [f32; 4],
    /// The reprojection matrix `M_eye = VP_eye · VP_center⁻¹` written into `cb13`'s `M_eye` block, one
    /// row per `cb13` row. Near-identity for a small IPD and cant; a large deviation flags a bug.
    pub cb13_m_eye: [f32; 16],
}

/// A serializable snapshot of the whole frame's single-pass matrix state, refreshed each time
/// [`compute_dual_eye_rows`] runs and dumped alongside an F12 screenshot ([`last_frame_diagnostics`]).
#[derive(Clone, serde::Serialize)]
pub struct FrameDiagnostics {
    pub single_pass: bool,
    pub dual_eye: bool,
    pub collapse: bool,
    pub double_wide: bool,
    pub capability: &'static str,
    /// The recorded full (unsplit) viewport `[x, y, w, h, minDepth, maxDepth]`, if one is bound.
    pub full_viewport: Option<[f32; 6]>,
    /// The pristine head-centre world transform, row-major (engine `Matrix4` order).
    pub center_transform: [f32; 16],
    /// The head-centre camera world position (`cb0[4]`).
    pub center_camera_position: [f32; 4],
    /// The angle between the two eyes' view forwards, in degrees (a stereo pair should be a few).
    pub forward_divergence_deg: f32,
    /// The cumulative shader-substitution tallies at capture time. See [`SubstitutionStats`].
    pub substitution: SubstitutionStats,
    pub eyes: [EyeDiagnostics; 2],
}

static LAST_FRAME_DIAG: Mutex<Option<FrameDiagnostics>> = Mutex::new(None);

/// The most recent frame's single-pass matrix diagnostics, for the F12 screenshot JSON sidecar.
/// `None` until the dual-eye path has run at least once this session.
pub fn last_frame_diagnostics() -> Option<FrameDiagnostics> {
    LAST_FRAME_DIAG.lock().clone()
}

/// The mod-owned `cb13` constant buffer, lazily created and updated per view.
struct Cb13Buffer {
    buffer: Option<ID3D11Buffer>,
}

impl Cb13Buffer {
    /// Ensure the dynamic `cb13` buffer exists, write `rows` into it, and bind it at `b13`.
    unsafe fn upload_and_bind(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        rows: &[Vector4; STEREO_CB_TOTAL_ROWS],
    ) -> Result<(), windows::core::Error> {
        let byte_width = std::mem::size_of_val(rows) as u32;
        let buffer = match &self.buffer {
            Some(buffer) => buffer,
            None => {
                let mut created = None;
                unsafe {
                    device.CreateBuffer(
                        &D3D11_BUFFER_DESC {
                            ByteWidth: byte_width,
                            Usage: D3D11_USAGE_DYNAMIC,
                            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                            ..Default::default()
                        },
                        Some(&D3D11_SUBRESOURCE_DATA {
                            pSysMem: rows.as_ptr().cast(),
                            ..Default::default()
                        }),
                        Some(&mut created),
                    )?;
                }
                self.buffer
                    .insert(created.expect("CreateBuffer returned Ok with no buffer"))
            }
        };

        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
            std::ptr::copy_nonoverlapping(rows.as_ptr(), mapped.pData.cast(), STEREO_CB_TOTAL_ROWS);
            context.Unmap(buffer, 0);
            context.VSSetConstantBuffers(STEREO_CB_REGISTER, Some(&[Some(buffer.clone())]));
            // The terrain domain shader also reads `cb13` (its per-eye `M_eye` reprojection block), so
            // bind the same buffer at `b13` on the domain stage. The hull shader only forwards the eye
            // lane and reads nothing from `cb13`, so it needs no binding.
            context.DSSetConstantBuffers(STEREO_CB_REGISTER, Some(&[Some(buffer.clone())]));
        }
        Ok(())
    }
}

/// If single-pass is active, duplicate the immediate context's current viewport (and scissor) into
/// slot 1. Called right after the engine binds a render setup ([`SetRenderSetup`]), which is where
/// the viewport is (re)set -- including per-cascade in the shadow passes, so slot 1 tracks whatever
/// region is currently bound rather than going stale between binds.
pub fn duplicate_current_viewport() {
    if !active() {
        return;
    }
    // SAFETY: runs on the render thread after a render-setup bind; the device/context pointers are
    // stable and the ops run under the engine's context mutex.
    unsafe {
        let Some(ge) = GraphicsEngine::get() else {
            return;
        };
        let Some(device) = ge.m_Device.as_ref() else {
            return;
        };
        let Some(context) = device.m_Context.as_ref() else {
            return;
        };
        EnterCriticalSection(context.m_Mutex);
        duplicate_viewport(&context.m_Context);
        LeaveCriticalSection(context.m_Mutex);
    }
}

/// Re-apply the eye-half split to the currently-bound viewport at the start of the G-buffer range.
///
/// The main G-buffer render setup is bound (setting its viewport) *before* `DrawRenderPassRange`
/// raises [`in_gbuffer_range`], so the [`rs_set_viewports_detour`] identical-dups it instead of
/// splitting -- and that dup'd viewport covers the bulk of the geometry, so both instances of a
/// patched draw land in the same half. Called right after the range flag goes up (dual-eye only),
/// this reads that bound viewport and re-sets it as left/right halves.
pub fn apply_eye_split_viewport() {
    // Collapse routes per draw (`ensure_collapse_viewport`), so the pass-level pre-split is off there.
    if collapse_active() || !(dual_eye_active() && in_gbuffer_range()) {
        return;
    }
    // SAFETY: runs on the render thread at the G-buffer range boundary; the device/context pointers
    // are stable and the ops run under the engine's context mutex.
    unsafe {
        let Some(ge) = GraphicsEngine::get() else {
            return;
        };
        let Some(device) = ge.m_Device.as_ref() else {
            return;
        };
        let Some(context) = device.m_Context.as_ref() else {
            return;
        };
        let ctx = &context.m_Context;
        EnterCriticalSection(context.m_Mutex);
        let mut count = 1u32;
        let mut viewports = [D3D11_VIEWPORT::default(); 1];
        ctx.RSGetViewports(&mut count, Some(viewports.as_mut_ptr()));
        let vp = viewports[0];
        if vp.Width > 0.0 {
            let half = vp.Width / 2.0;
            let mut left = vp;
            left.Width = half;
            let mut right = vp;
            right.Width = half;
            right.TopLeftX = vp.TopLeftX + half;
            // count == 2 passes straight through the detour to the raw RSSetViewports.
            ctx.RSSetViewports(Some(&[left, right]));
        }
        LeaveCriticalSection(context.m_Mutex);
    }
}

/// Duplicate the current (single) viewport into viewport slots 0 **and** 1, both covering the same
/// region.
///
/// A patched shader writes `SV_ViewportArrayIndex = SV_InstanceID & 1`. With divergence off nothing
/// doubles instances or sets up per-eye viewports, so an instanced draw's odd-`SV_InstanceID`
/// primitives would route to viewport 1 -- which the engine never bound -- and be discarded, dropping
/// half of every instanced object (the flicker, since VR head-motion re-sorts which instance ids are
/// odd). Binding a second, identical viewport makes index 1 valid and render the same as index 0.
/// When the eyes diverge, the two identical viewports become the left/right halves of the double-wide
/// target instead.
unsafe fn duplicate_viewport(context: &ID3D11DeviceContext) {
    unsafe {
        let mut count = 1u32;
        let mut viewports = [D3D11_VIEWPORT::default(); 1];
        context.RSGetViewports(&mut count, Some(viewports.as_mut_ptr()));
        // Only duplicate a real viewport; a zero-width one (no viewport bound yet) would clip
        // everything to nothing.
        if viewports[0].Width > 0.0 {
            context.RSSetViewports(Some(&[viewports[0], viewports[0]]));
        }

        // If scissor testing is on, viewport 1 pairs with scissor rect 1; duplicate the engine's
        // rect into slot 1 too, else index-1 primitives clip to an empty (unset) rect.
        let mut scissor_count = 1u32;
        let mut scissors = [RECT::default(); 1];
        context.RSGetScissorRects(&mut scissor_count, Some(scissors.as_mut_ptr()));
        if scissors[0].right > scissors[0].left && scissors[0].bottom > scissors[0].top {
            context.RSSetScissorRects(Some(&[scissors[0], scissors[0]]));
        }
    }
}

// The mirror at `SetRenderSetup` (above) covers the scene passes, but the shadow cascades set their
// viewport through a raw `RSSetViewports` between binds, which that hook does not see -- so slot 1
// goes stale there and odd-instance shadow casters route to the wrong region (flickering shadows).
// Detouring `RSSetViewports`/`RSSetScissorRects` on the immediate-context vtable catches *every*
// viewport set, wherever it comes from, and mirrors a single-viewport set into two identical slots.

/// `ID3D11DeviceContext` vtable slots (7 base `IUnknown`/`ID3D11DeviceChild` slots + the method's
/// index), verified against `windows`'s `ID3D11DeviceContext_Vtbl`.
const RS_SET_VIEWPORTS_SLOT: usize = 44;
const RS_SET_SCISSOR_RECTS_SLOT: usize = 45;

type RsSetViewportsFn = unsafe extern "system" fn(*mut c_void, u32, *const D3D11_VIEWPORT);
type RsSetScissorRectsFn = unsafe extern "system" fn(*mut c_void, u32, *const RECT);

static RS_SET_VIEWPORTS: OnceLock<GenericDetour<RsSetViewportsFn>> = OnceLock::new();
static RS_SET_SCISSOR_RECTS: OnceLock<GenericDetour<RsSetScissorRectsFn>> = OnceLock::new();

unsafe extern "system" fn rs_set_viewports_detour(
    context: *mut c_void,
    count: u32,
    viewports: *const D3D11_VIEWPORT,
) {
    let detour = RS_SET_VIEWPORTS.get().expect("set before enable");
    if active() && count == 1 && !viewports.is_null() {
        let vp = unsafe { *viewports };
        if collapse_active() {
            // Collapse: record the full viewport and bind both slots to it unsplit. The eye-split is
            // applied per-draw in `draw_indexed_detour` via `ensure_collapse_viewport`, so the
            // interleaved fullscreen lighting/post passes (which do not route to an eye) keep the full
            // width while patched geometry gets the L/R halves. Binding both slots keeps a patched
            // shader that writes `SV_ViewportArrayIndex` valid before the first split of a pass.
            *COLLAPSE_FULL_VIEWPORT.lock() = Some(vp);
            VIEWPORT_DUP.fetch_add(1, Ordering::Relaxed);
            unsafe { detour.call(context, 2, [vp, vp].as_ptr()) };
            return;
        }
        let (slot0, slot1) = if dual_eye_active() && in_gbuffer_range() {
            // Route the two eyes to the left/right halves of the (double-wide) target.
            VIEWPORT_SPLIT.fetch_add(1, Ordering::Relaxed);
            let half = vp.Width / 2.0;
            let mut left = vp;
            left.Width = half;
            let mut right = vp;
            right.Width = half;
            right.TopLeftX = vp.TopLeftX + half;
            (left, right)
        } else {
            // Not diverging: both slots identical, so a patched shader routes anywhere validly.
            VIEWPORT_DUP.fetch_add(1, Ordering::Relaxed);
            (vp, vp)
        };
        unsafe { detour.call(context, 2, [slot0, slot1].as_ptr()) };
    } else {
        unsafe { detour.call(context, count, viewports) };
    }
}

/// In the collapsed single walk, bind the immediate-context viewport as the L/R eye halves (for a
/// patched geometry draw) or the full width (for a fullscreen/unpatched draw), re-binding only on a
/// transition. Derives the halves from the full viewport recorded by [`rs_set_viewports_detour`]; a
/// no-op until the scene's first viewport bind records it.
fn ensure_collapse_viewport(context: *mut c_void, split: bool) {
    let Some(full) = *COLLAPSE_FULL_VIEWPORT.lock() else {
        return;
    };
    let Some(detour) = RS_SET_VIEWPORTS.get() else {
        return;
    };
    let viewports = if split {
        VIEWPORT_SPLIT.fetch_add(1, Ordering::Relaxed);
        let half = full.Width / 2.0;
        let mut left = full;
        left.Width = half;
        let mut right = full;
        right.Width = half;
        right.TopLeftX = full.TopLeftX + half;
        [left, right]
    } else {
        [full, full]
    };
    // SAFETY: `context` is the live immediate context; `detour.call` invokes the original
    // RSSetViewports (the trampoline), so this does not re-enter the detour. Bound unconditionally
    // (no split-state skip): the engine can change the viewport underneath us via a path we do not
    // observe (a `count != 1` set), so a cached "already split" flag would go stale and let both
    // instances land in one half -- the doubled/"same geometry twice" artifact. Re-binding per draw
    // is cheap (a few hundred geometry draws per frame, far below the draw budget we are cutting).
    unsafe { detour.call(context, 2, viewports.as_ptr()) };
}

unsafe extern "system" fn rs_set_scissor_rects_detour(
    context: *mut c_void,
    count: u32,
    rects: *const RECT,
) {
    let detour = RS_SET_SCISSOR_RECTS.get().expect("set before enable");
    if active() && count == 1 && !rects.is_null() {
        let rect = unsafe { *rects };
        unsafe { detour.call(context, 2, [rect, rect].as_ptr()) };
    } else {
        unsafe { detour.call(context, count, rects) };
    }
}

/// `ID3D11DeviceContext` vtable slots for the two indexed-draw entry points (verified against
/// `windows`'s `ID3D11DeviceContext_Vtbl`: field 6 → slot 12, field 14 → slot 20).
const DRAW_INDEXED_SLOT: usize = 12;
const DRAW_SLOT: usize = 13;
const DRAW_INDEXED_INSTANCED_SLOT: usize = 20;

type DrawIndexedFn = unsafe extern "system" fn(*mut c_void, u32, u32, i32);
type DrawFn = unsafe extern "system" fn(*mut c_void, u32, u32);
type DrawIndexedInstancedFn = unsafe extern "system" fn(*mut c_void, u32, u32, u32, i32, u32);

static DRAW_INDEXED: OnceLock<GenericDetour<DrawIndexedFn>> = OnceLock::new();
static DRAW: OnceLock<GenericDetour<DrawFn>> = OnceLock::new();
/// The raw `DrawIndexedInstanced` entry (not detoured), used to re-issue a promoted draw.
static DRAW_INDEXED_INSTANCED_RAW: OnceLock<DrawIndexedInstancedFn> = OnceLock::new();

/// Handle a `DrawIndexed` while the dual-eye G-buffer geometry is drawing. A **patched** shader is
/// promoted to a 2-instance `DrawIndexedInstanced` -- its `SV_InstanceID & 1` selects the eye and
/// `SV_ViewportArrayIndex` routes it to that eye's viewport half (one draw, both eyes). An
/// **unpatched** shader is left single, so it rasterises to viewport slot 0 -- which under collapse
/// is the left eye's half, meaning unpatched indexed geometry is absent from the right eye. The
/// patched/unpatched split is counted for the diagnostic log.
unsafe extern "system" fn draw_indexed_detour(
    context: *mut c_void,
    index_count: u32,
    start_index: u32,
    base_vertex: i32,
) {
    let detour = DRAW_INDEXED.get().expect("set before enable");
    if dual_eye_active() && in_gbuffer_range() {
        let patched = BOUND_VS_PATCHED.load(Ordering::Relaxed);
        // Collapse: split the viewport into L/R eye halves for a patched geometry draw, re-binding
        // only on a transition (a pass boundary, not every draw). Unpatched `DrawIndexed` geometry is
        // left on whatever viewport is bound -- the fullscreen reset-to-full lives in `draw_detour`
        // (non-indexed `Draw`), so the deferred-lighting/post passes restore the full width without
        // dragging unpatched *geometry* to full width and ghosting it across both eyes.
        if collapse_active() && patched {
            ensure_collapse_viewport(context, true);
        }
        if patched {
            PATCHED_DRAWS.fetch_add(1, Ordering::Relaxed);
            if let Some(instanced) = DRAW_INDEXED_INSTANCED_RAW.get() {
                unsafe { instanced(context, index_count, 2, start_index, base_vertex, 0) };
                return;
            }
        } else {
            UNPATCHED_DRAWS.fetch_add(1, Ordering::Relaxed);
        }
    }
    unsafe { detour.call(context, index_count, start_index, base_vertex) };
}

/// Handle a non-indexed `DrawIndexed` sibling -- `Draw` (vtable slot 13). The fullscreen passes
/// (deferred lighting, screen-space effects, post) draw a fullscreen triangle this way, and under
/// collapse they must cover the **full** target, not the eye-half the previous patched geometry draw
/// left the viewport split to. Reset the viewport to full before the draw. Outside collapse (or the
/// camera scene) this is a straight pass-through.
unsafe extern "system" fn draw_detour(context: *mut c_void, vertex_count: u32, start_vertex: u32) {
    if collapse_active() && in_gbuffer_range() {
        ensure_collapse_viewport(context, false);
    }
    let detour = DRAW.get().expect("set before enable");
    unsafe { detour.call(context, vertex_count, start_vertex) };
}

/// `ID3D11Device::CreateVertexShader` (device vtable slot 12) and `ID3D11DeviceContext::VSSetShader`
/// (context vtable slot 11), verified against the `windows` vtable structs.
const CREATE_VERTEX_SHADER_SLOT: usize = 12;
const VS_SET_SHADER_SLOT: usize = 11;

type CreateVertexShaderFn = unsafe extern "system" fn(
    *mut c_void,
    *const c_void,
    usize,
    *mut c_void,
    *mut *mut c_void,
) -> i32;
type VsSetShaderFn = unsafe extern "system" fn(*mut c_void, *mut c_void, *const *mut c_void, u32);

static CREATE_VERTEX_SHADER: OnceLock<GenericDetour<CreateVertexShaderFn>> = OnceLock::new();
static VS_SET_SHADER: OnceLock<GenericDetour<VsSetShaderFn>> = OnceLock::new();

/// Substitute and record the `ID3D11VertexShader` for a stereo-patched blob, covering both the fresh
/// and the re-created shader-creation paths.
///
/// The `CreateVertexProgram` hook substitutes the blob for a *fresh* shader create and sets
/// [`PATCH_PENDING`] right before the engine calls `CreateVertexShader`, so the shader created under
/// that flag is the patched one and goes straight into [`PATCHED_VS`]. But a bundle reload re-creates
/// an already-loaded shader through `ResourceCacheReCreateResource`, which calls `CreateVertexShader`
/// directly *without* re-running `CreateVertexProgram` -- so a shader first loaded before single-pass
/// (e.g. a character shader from level start, whose resource still holds the original bytecode) would
/// arrive here unsubstituted and render mono/skewed. Catch that path by running the rewrite on the
/// incoming bytecode: an unpatched-but-patchable blob is substituted in place for the create call, and
/// an already-patched blob (`Cb13AlreadyDeclared` -- a reload of a shader whose resource already holds
/// the patched blob) is recorded as-is. `PATCH_PENDING` short-circuits the re-analysis for the fresh
/// path, whose blob `CreateVertexProgram` already substituted.
unsafe extern "system" fn create_vertex_shader_detour(
    device: *mut c_void,
    bytecode: *const c_void,
    length: usize,
    linkage: *mut c_void,
    out: *mut *mut c_void,
) -> i32 {
    let detour = CREATE_VERTEX_SHADER.get().expect("set before enable");
    let pending = PATCH_PENDING.with(Cell::take);
    let reacquired = (!pending && active() && !bytecode.is_null() && length >= 4).then(|| {
        let code = unsafe { std::slice::from_raw_parts(bytecode.cast::<u8>(), length) };
        dxbc_stereo::patch_vertex_shader(code)
    });
    let (record, blob, len) = match &reacquired {
        Some(Ok(patched)) => {
            CVS_REACQ_PATCHED.fetch_add(1, Ordering::Relaxed);
            (true, patched.as_ptr().cast::<c_void>(), patched.len())
        }
        Some(Err(DxbcError::Cb13AlreadyDeclared)) => {
            CVS_REACQ_CB13.fetch_add(1, Ordering::Relaxed);
            (true, bytecode, length)
        }
        Some(Err(DxbcError::NoPerEyeReferences)) => {
            CVS_REACQ_NOREFS.fetch_add(1, Ordering::Relaxed);
            (false, bytecode, length)
        }
        Some(Err(_)) => {
            CVS_REACQ_ERR.fetch_add(1, Ordering::Relaxed);
            (false, bytecode, length)
        }
        None => {
            if pending {
                CVS_PENDING.fetch_add(1, Ordering::Relaxed);
            }
            (pending, bytecode, length)
        }
    };
    let hr = unsafe { detour.call(device, blob, len, linkage, out) };
    if record
        && hr == 0
        && !out.is_null()
        && let shader = unsafe { *out }
        && !shader.is_null()
    {
        PATCHED_VS.lock().push(shader as usize);
    }
    hr
}

/// Cache whether the vertex shader now bound is a patched one, so [`draw_indexed_detour`] can gate
/// without a per-draw set lookup.
unsafe extern "system" fn vs_set_shader_detour(
    context: *mut c_void,
    shader: *mut c_void,
    instances: *const *mut c_void,
    num_instances: u32,
) {
    let patched = !shader.is_null() && PATCHED_VS.lock().contains(&(shader as usize));
    BOUND_VS_PATCHED.store(patched, Ordering::Relaxed);
    let detour = VS_SET_SHADER.get().expect("set before enable");
    unsafe { detour.call(context, shader, instances, num_instances) };
}

/// Reset the patched-shader set (on a shader reload, since the old `ID3D11VertexShader` pointers are
/// released and could be reused by later allocations). Also zeroes the `CreateVertexShader`-path
/// tallies so [`substitution_stats`] reflects one clean pass over the reloaded shader set.
pub fn reset_patched_vs() {
    PATCHED_VS.lock().clear();
    CVS_PENDING.store(0, Ordering::Relaxed);
    CVS_REACQ_PATCHED.store(0, Ordering::Relaxed);
    CVS_REACQ_CB13.store(0, Ordering::Relaxed);
    CVS_REACQ_NOREFS.store(0, Ordering::Relaxed);
    CVS_REACQ_ERR.store(0, Ordering::Relaxed);
}

/// Whether single-pass has substituted any patched vertex shaders this session (they are still held
/// by the game). If so, eject must re-create the originals, else the game keeps rendering with the
/// mod's `cb13`-reading shaders after the mod is gone.
pub fn has_patched_shaders() -> bool {
    !PATCHED_VS.lock().is_empty()
}

/// A snapshot of the shader-substitution tallies, for the bring-up log and the screenshot JSON sidecar.
/// `recorded_vs` is the live size of [`PATCHED_VS`] (patched shaders the draw gating will double); the
/// `cvs_*` fields are the [`create_vertex_shader_detour`] outcome buckets; the `census_*` fields are the
/// [`record_patch_outcome`] buckets from the `CreateVertexProgram` hook. Comparing the two paths shows
/// whether the re-create path (which skips `CreateVertexProgram`) is reaching the D3D-level substitution.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct SubstitutionStats {
    pub recorded_vs: usize,
    pub cvs_pending: usize,
    pub cvs_reacq_patched: usize,
    pub cvs_reacq_cb13: usize,
    pub cvs_reacq_no_refs: usize,
    pub cvs_reacq_err: usize,
    pub census_patched: usize,
    pub census_no_refs: usize,
    pub census_deferred: usize,
    pub census_errored: usize,
}

/// Snapshot the current shader-substitution tallies. See [`SubstitutionStats`].
pub fn substitution_stats() -> SubstitutionStats {
    SubstitutionStats {
        recorded_vs: PATCHED_VS.lock().len(),
        cvs_pending: CVS_PENDING.load(Ordering::Relaxed),
        cvs_reacq_patched: CVS_REACQ_PATCHED.load(Ordering::Relaxed),
        cvs_reacq_cb13: CVS_REACQ_CB13.load(Ordering::Relaxed),
        cvs_reacq_no_refs: CVS_REACQ_NOREFS.load(Ordering::Relaxed),
        cvs_reacq_err: CVS_REACQ_ERR.load(Ordering::Relaxed),
        census_patched: patched_count(),
        census_no_refs: no_refs_count(),
        census_deferred: deferred_count(),
        census_errored: errored_count(),
    }
}

/// Log and reset the per-window patched/unpatched G-buffer draw counts -- called once per frame so
/// the bring-up log shows how the draw gating is splitting the geometry.
pub fn log_draw_split() {
    let patched = PATCHED_DRAWS.swap(0, Ordering::Relaxed);
    let unpatched = UNPATCHED_DRAWS.swap(0, Ordering::Relaxed);
    let split = VIEWPORT_SPLIT.swap(0, Ordering::Relaxed);
    let dup = VIEWPORT_DUP.swap(0, Ordering::Relaxed);
    if patched + unpatched > 0 {
        let n = DRAW_SPLIT_LOG.fetch_add(1, Ordering::Relaxed);
        if n.is_multiple_of(120) {
            let s = substitution_stats();
            tracing::info!(
                target: "single_pass",
                "gbuffer draws: {patched} patched, {unpatched} unpatched | viewports: {split} split, {dup} identical-dup | \
                 recorded VS={} | CreateVertexShader: pending={} reacq[patched={} cb13={} no-refs={} err={}] | \
                 census[patched={} no-refs={} deferred={} errored={}]",
                s.recorded_vs,
                s.cvs_pending,
                s.cvs_reacq_patched,
                s.cvs_reacq_cb13,
                s.cvs_reacq_no_refs,
                s.cvs_reacq_err,
                s.census_patched,
                s.census_no_refs,
                s.census_deferred,
                s.census_errored,
            );
        }
    }
}

/// Disable all the single-pass COM-vtable detours, restoring the original D3D functions. Must run on
/// eject **before** the payload unloads: the detours inline-patch the DXVK functions to jump into
/// payload code, so leaving them enabled while the DLL unmaps dangles those jumps and the next D3D
/// call crashes. Runs under a thread suspender and the install lock, like install -- with the same
/// caveat that suspension narrows rather than closes the in-prologue window.
pub fn uninstall_com_detours() {
    let _install = DETOUR_INSTALL.lock();
    if RS_SET_VIEWPORTS.get().is_none() {
        return; // never installed (single-pass never activated this session)
    }
    let _ = ThreadSuspender::for_block(|| {
        // A detour left enabled here is a relay still pointing into the about-to-be-freed payload
        // image, so a swallowed failure would be an undiagnosable crash -- log it instead.
        macro_rules! disable_detour {
            ($lock:expr, $name:literal) => {
                if let Some(detour) = $lock.get() {
                    // SAFETY: patching the function back runs with all other threads suspended.
                    match unsafe { detour.disable() } {
                        Err(e) => tracing::error!("single-pass: {} disable failed: {e}", $name),
                        Ok(()) if detour.is_enabled() => tracing::error!(
                            "single-pass: {} still enabled after disable (dangling into freed payload)",
                            $name
                        ),
                        Ok(()) => {}
                    }
                }
            };
        }
        disable_detour!(RS_SET_VIEWPORTS, "RSSetViewports");
        disable_detour!(RS_SET_SCISSOR_RECTS, "RSSetScissorRects");
        disable_detour!(DRAW_INDEXED, "DrawIndexed");
        disable_detour!(DRAW, "Draw");
        disable_detour!(VS_SET_SHADER, "VSSetShader");
        disable_detour!(CREATE_VERTEX_SHADER, "CreateVertexShader");
        disable_detour!(SET_VERTEX_PROGRAM_CONSTANTS, "SetVertexProgramConstants");
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });
    tracing::info!("single-pass: COM detours uninstalled");
}

/// Install the single-pass COM-vtable detours on the immediate-context (and device) vtables, once.
/// Patching runs under a thread suspender, which narrows the window in which another thread can be
/// executing a target's prologue while it is rewritten -- it does not close it: `SuspendThread` is
/// asynchronous, and no instruction pointer is inspected, so a thread already inside the bytes being
/// overwritten stays there. Called from the active render path and from the
/// `CreateVertexProgram` hook -- the latter so the `CreateVertexShader` detour that records a patched
/// shader into [`PATCHED_VS`] exists *before* the shader is created, not lazily on the first rendered
/// frame (a shader created in between, e.g. a character shader loaded at level start, would otherwise
/// be patched at the blob level but never recorded, so `BOUND_VS_PATCHED` stays false and its draw is
/// never doubled). A normal (single-pass-off) session never installs it.
pub(crate) fn ensure_viewport_detours() {
    // The whole body is serialized, not just the published-yet check: the callers are on different
    // threads (the render thread's cb13 mirror and the shader-creation thread's `CreateVertexProgram`),
    // and the publish happens only after seven `GenericDetour::new` calls. Two threads could otherwise
    // both pass an unpublished check and both reach `ThreadSuspender::for_block`, each suspending the
    // other -- a silent, permanent hang. `uninstall_com_detours` takes the same lock so an eject cannot
    // interleave with an install.
    let _install = DETOUR_INSTALL.lock();
    if RS_SET_VIEWPORTS.get().is_some() || crate::is_shutting_down() {
        return; // already installed, or tearing down -- never (re)install during eject
    }
    // SAFETY: reads the live immediate-context vtable; the two slots are the standard D3D11 layout.
    unsafe {
        let Some(ge) = GraphicsEngine::get() else {
            return;
        };
        let Some(device) = ge.m_Device.as_ref() else {
            return;
        };
        let Some(context) = device.m_Context.as_ref() else {
            return;
        };
        let vtable = *(context.m_Context.as_raw() as *const *const usize);
        let device_vtable = *(device.m_Device.as_raw() as *const *const usize);
        let viewports_target: RsSetViewportsFn =
            std::mem::transmute(*vtable.add(RS_SET_VIEWPORTS_SLOT));
        let scissors_target: RsSetScissorRectsFn =
            std::mem::transmute(*vtable.add(RS_SET_SCISSOR_RECTS_SLOT));
        let draw_indexed_target: DrawIndexedFn =
            std::mem::transmute(*vtable.add(DRAW_INDEXED_SLOT));
        let draw_target: DrawFn = std::mem::transmute(*vtable.add(DRAW_SLOT));
        let draw_indexed_instanced: DrawIndexedInstancedFn =
            std::mem::transmute(*vtable.add(DRAW_INDEXED_INSTANCED_SLOT));
        let vs_set_shader_target: VsSetShaderFn =
            std::mem::transmute(*vtable.add(VS_SET_SHADER_SLOT));
        let create_vertex_shader_target: CreateVertexShaderFn =
            std::mem::transmute(*device_vtable.add(CREATE_VERTEX_SHADER_SLOT));
        // Unlike the rest, this one is a static engine function (not a COM vtable slot): the leaf
        // vertex-constant stager, detoured so the baked-cb per-eye re-issue can reproject a block's own
        // constant upload.
        let set_vs_consts_target: SetVertexProgramConstantsFn =
            std::mem::transmute(jc3gi::graphics_engine::draw::SetVertexProgramConstants_ADDRESS);

        let (
            Ok(viewports_detour),
            Ok(scissors_detour),
            Ok(draw_indexed_detour_handle),
            Ok(draw_detour_handle),
            Ok(vs_set_shader_detour_handle),
            Ok(create_vertex_shader_detour_handle),
            Ok(set_vs_consts_detour_handle),
        ) = (
            GenericDetour::new(viewports_target, rs_set_viewports_detour),
            GenericDetour::new(scissors_target, rs_set_scissor_rects_detour),
            GenericDetour::new(draw_indexed_target, draw_indexed_detour),
            GenericDetour::new(draw_target, draw_detour),
            GenericDetour::new(vs_set_shader_target, vs_set_shader_detour),
            GenericDetour::new(create_vertex_shader_target, create_vertex_shader_detour),
            GenericDetour::new(set_vs_consts_target, set_vertex_program_constants_detour),
        )
        else {
            tracing::warn!("single-pass: COM detour construction failed");
            return;
        };

        // Publish into the statics before enabling, so a detour that fires mid-enable finds its
        // trampoline. Enabling itself runs with other threads suspended.
        let _ = DRAW_INDEXED_INSTANCED_RAW.set(draw_indexed_instanced);
        let _ = RS_SET_VIEWPORTS.set(viewports_detour);
        let _ = RS_SET_SCISSOR_RECTS.set(scissors_detour);
        let _ = DRAW_INDEXED.set(draw_indexed_detour_handle);
        let _ = DRAW.set(draw_detour_handle);
        let _ = VS_SET_SHADER.set(vs_set_shader_detour_handle);
        let _ = CREATE_VERTEX_SHADER.set(create_vertex_shader_detour_handle);
        let _ = SET_VERTEX_PROGRAM_CONSTANTS.set(set_vs_consts_detour_handle);
        let _ = ThreadSuspender::for_block(|| {
            RS_SET_VIEWPORTS.get().expect("just set").enable().ok();
            RS_SET_SCISSOR_RECTS.get().expect("just set").enable().ok();
            DRAW_INDEXED.get().expect("just set").enable().ok();
            DRAW.get().expect("just set").enable().ok();
            VS_SET_SHADER.get().expect("just set").enable().ok();
            CREATE_VERTEX_SHADER.get().expect("just set").enable().ok();
            SET_VERTEX_PROGRAM_CONSTANTS
                .get()
                .expect("just set")
                .enable()
                .ok();
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        });
        tracing::info!("single-pass: viewport + draw + shader-tracking COM detours installed");
    }
}

/// Serializes [`ensure_viewport_detours`] against itself and against [`uninstall_com_detours`]. Both
/// suspend every other thread while they patch, so two of them running concurrently would suspend each
/// other and hang the process.
static DETOUR_INSTALL: Mutex<()> = Mutex::new(());

static CB13: Mutex<Cb13Buffer> = Mutex::new(Cb13Buffer { buffer: None });
static IN_GBUFFER_RANGE: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Set by the `CreateVertexProgram` hook right before the engine creates the D3D shader from a
    /// substituted (patched) blob, so [`create_vertex_shader_detour`] knows the next shader is patched.
    ///
    /// Thread-local, not a global: `ID3D11Device` is free-threaded and JC3 streams resources off the
    /// render thread, so a loader thread's flag would otherwise tag whatever shader a *different*
    /// thread happened to create next -- instance-doubling and eye-splitting an unrelated shader while
    /// the genuinely patched one went unrecorded. The hook sets it and the device detour consumes it
    /// within the same synchronous call, so the flag never needs to cross a thread.
    static PATCH_PENDING: Cell<bool> = const { Cell::new(false) };
}

/// Set (or clear) this thread's [`PATCH_PENDING`] flag. Called by the `CreateVertexProgram` hook
/// around the engine's shader creation.
pub fn set_patch_pending(pending: bool) {
    PATCH_PENDING.with(|flag| flag.set(pending));
}
/// The `ID3D11VertexShader` pointers created from patched blobs (as `usize`). Written at creation,
/// read when a shader is bound.
static PATCHED_VS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
/// Whether the currently-bound vertex shader is a patched one (updated on `VSSetShader`).
static BOUND_VS_PATCHED: AtomicBool = AtomicBool::new(false);
static PATCHED_DRAWS: AtomicUsize = AtomicUsize::new(0);
static UNPATCHED_DRAWS: AtomicUsize = AtomicUsize::new(0);
static DRAW_SPLIT_LOG: AtomicUsize = AtomicUsize::new(0);
static VIEWPORT_SPLIT: AtomicUsize = AtomicUsize::new(0);
static VIEWPORT_DUP: AtomicUsize = AtomicUsize::new(0);

/// `CreateVertexShader`-detour outcome tallies (cumulative since injection), to diagnose what the
/// shader re-create path -- which bypasses `CreateVertexProgram` -- feeds through the D3D-level
/// substitution: `pending` came pre-substituted from `CreateVertexProgram`; the four `reacq_*` buckets
/// are what the detour's own rewrite of the incoming bytecode found.
static CVS_PENDING: AtomicUsize = AtomicUsize::new(0);
static CVS_REACQ_PATCHED: AtomicUsize = AtomicUsize::new(0);
static CVS_REACQ_CB13: AtomicUsize = AtomicUsize::new(0);
static CVS_REACQ_NOREFS: AtomicUsize = AtomicUsize::new(0);
static CVS_REACQ_ERR: AtomicUsize = AtomicUsize::new(0);

/// The last full (unsplit) viewport bound during a collapsed camera scene, recorded by
/// [`rs_set_viewports_detour`] so [`ensure_collapse_viewport`] can derive the L/R eye halves at draw
/// time. `None` until the scene's first viewport bind.
///
/// Deliberately *not* cleared at the end of the G-buffer range, unlike [`CURRENT_M_EYE`]: the
/// post-draw UI overlay ([`collapse_ui_eye_override`]) reads it to place each eye's HUD quad, and that
/// runs long after the range. Every consumer that must not see a stale value gates on
/// [`in_gbuffer_range`] instead.
static COLLAPSE_FULL_VIEWPORT: Mutex<Option<D3D11_VIEWPORT>> = Mutex::new(None);
/// The two per-eye reprojection matrices `M_eye` (`clip_eye = M_eye · clip_center`), published each
/// view by [`compute_dual_eye_rows`]. The terrain-detail render-block intercept reads them to build a
/// per-eye `cb1` on the CPU (the detail draw is GPU-indirect, so it cannot be instance-doubled).
static CURRENT_M_EYE: Mutex<Option<[glam::Mat4; 2]>> = Mutex::new(None);

static CAPABILITY: AtomicU8 = AtomicU8::new(Capability::Unprobed as u8);
static PATCHED: AtomicUsize = AtomicUsize::new(0);
static NO_REFS: AtomicUsize = AtomicUsize::new(0);
static DEFERRED: AtomicUsize = AtomicUsize::new(0);
static ERRORED: AtomicUsize = AtomicUsize::new(0);
/// Terrain tessellation shaders substituted for single-pass since injection: hull shaders whose eye
/// lane was forwarded, and domain shaders reprojected. Surfaced in the debug UI so it is clear whether
/// the terrain path is catching anything.
static TERRAIN_HS_FORWARDED: AtomicUsize = AtomicUsize::new(0);
static TERRAIN_DS_REPROJECTED: AtomicUsize = AtomicUsize::new(0);
