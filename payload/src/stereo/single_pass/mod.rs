//! Single-pass stereo (experimental): render the G-buffer geometry once, emitting both eyes via
//! instancing + `SV_ViewportArrayIndex` routing into a double-wide render target, instead of the
//! double-draw (two full `game.Draw` walks, one per eye). See `docs/mod/single-pass-stereo.md` for
//! the design.
//!
//! This module owns the mod-side state that the double-draw path does not need:
//! - the DXVK viewport-routing **capability probe** ([`probe`] / [`capability`]);
//! - the vertex-shader rewrite **census** ([`record_patch_outcome`] and the `*_count` getters), which
//!   the `CreateVertexProgram` hook feeds so the debug UI can report how the rewriter fared against
//!   the game's real shader set;
//! - which **transform** each vertex shader gets ([`decide_vs_transform`]), decided from its engine
//!   name at `CreateVertexProgram` time and remembered against its bytecode ([`remember_vs_transform`])
//!   so the D3D-level re-create path, which sees no name, applies the same decision instead of
//!   guessing a different one.
//!
//! The rest of the pipeline (cb13 dual-eye upload, the double-wide render-setup re-init, the
//! draw-doubling) runs under [`crate::config::StereoConfig::single_pass`] and the per-step flags
//! beside it. [`crate::config::StereoConfig::single_pass_patch_dryrun`] runs the census alone, with
//! no rendering change.

use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicU32, AtomicUsize, Ordering},
};

use dxbc_stereo::DxbcError;
use jc3gi::{
    graphics_engine::{
        draw::SetVertexProgramConstants,
        graphics_engine::{GraphicsEngine, HContext_t, RenderContext},
        render_block::RenderBlockTerrainDetail,
        render_engine::{RenderEngine, RenderPassId},
    },
    types::math::{Matrix4, Vector4},
};
use parking_lot::Mutex;
use re_utilities::ThreadSuspender;
use retour::{Function, GenericDetour};
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
    core::{IUnknown, Interface},
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
mod cb13;
mod config_snapshot;
mod frame_diagnostics;
mod instanced_exposure;

pub use cb13::*;
pub use config_snapshot::*;
pub use frame_diagnostics::*;
pub use instanced_exposure::*;

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
    // The `landmark` and `layered` model families, structurally identical to `generaljc3`: clip is
    // `cb1[0..3] · objectPosition` (a baked world-view-projection), and the only `cb0` row they touch
    // is `cb0[4]`, differenced against the world position to drive a distance fade. `layered` also
    // covers `layeredblend` and `layeredrsm`; `layeredroad` reads the real `cb0[29..32]` and so keeps
    // the remap, which already gives it a per-eye clip.
    "landmark",
    "layered",
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
    let flags = config_flags();
    (flags.has(Flag::Reproject) && REPROJECT_NAME_PREFIXES.iter().any(|p| name.starts_with(p)))
        || (flags.has(Flag::TreeImpostors)
            && VEGETATION_REPROJECT_NAME_PREFIXES
                .iter()
                .any(|p| name.starts_with(p)))
}

/// Whether a scene family the `cb0` remap *claims* should be reprojected instead, because the only
/// `cb0` row it reads is the camera position and its clip position therefore comes from a baked
/// matrix like the no-`cb0` families'.
///
/// The remap's candidacy test is "references one of `cb0[{4, 29..32}]`", but those rows are not one
/// thing. `cb0[29..32]` can only be a clip transform; `cb0[4]` is a camera *position*, which a shader
/// may read for a view vector or a distance fade while taking its clip from a constant buffer of its
/// own. `generaljc3` does exactly that -- `add r1.xyz, r0.xyzx, -cb0[4].xyzx` feeds a `dp3` whose
/// result is a LOD fade, and clip comes from a baked `cb1[0..3]`. Claimed by the remap, such a shader
/// gets viewport routing and instance doubling but no per-eye clip: under the collapse the render
/// camera is centred, so *both* eye halves are drawn from the centre viewpoint, and the family sits at
/// a rigid half-IPD offset from everything around it. That is visible in a single eye, not only in
/// stereo.
///
/// Reprojection is the fix and is already built; it was simply unreachable, because the remap claimed
/// the shader before [`should_reproject`] was ever consulted. Routing it there costs nothing extra --
/// the shader still resolves its eye from `SV_InstanceID & 1` and the draw is still the one
/// instance-doubled submission.
///
/// Gated on the family also being on [`REPROJECT_NAME_PREFIXES`], so this only ever moves a shader
/// between two per-eye transforms, never off one: an unrecognised family keeps the remap rather than
/// falling through to being drawn once. The `cb0[4]` reference is deliberately left un-remapped, which
/// leaves the fade distance measured from the centre camera -- the same in both eyes, so a LOD does
/// not pop between them.
///
/// The families this reaches in the shipped bundle are `generaljc3`, `landmark`, `layered` and
/// `layeredblend`, which share one body: `clip = cb1[0..3] · objectPosition`, a distance fade off
/// `cb0[4]`, and a depth bias applied *after* the projection (`o0.z += cb2[0].x · o0.w`). The
/// reprojection folds that bias into the clip position it then transforms by `M_eye`; `M_eye` is near
/// identity so it survives approximately, and it is the first thing to suspect if z-fighting shows up
/// on any of them.
pub fn should_reproject_camera_only(name: Option<&str>, code: &[u8]) -> bool {
    config_flags().has(Flag::ReprojectCameraOnly)
        && should_reproject(name)
        && dxbc_stereo::reads_global_view_projection(code).is_ok_and(|reads| !reads)
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
    config_flags().has(Flag::Terrain)
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
    active() && config_flags().has(Flag::Terrain)
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

/// Which single-pass transform a vertex shader is to be given, resolved at `CreateVertexProgram` time
/// (see [`decide_vs_transform`]) and applied by [`apply_vs_transform`].
///
/// The decision needs the shader's engine name, which only the `CreateVertexProgram` hook has: the
/// D3D layer sees a bare blob. So the decision is made once, where the name is, and carried to the
/// D3D layer through [`remember_vs_transform`] rather than guessed at again from the bytecode --
/// guessing can only ever produce the `cb0` remap, which for the reprojection families is the wrong
/// transform and for the intercept-owned families is one transform too many.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VsTransform {
    /// The `cb0` per-eye remap ([`dxbc_stereo::patch_vertex_shader`]).
    Remap,
    /// The `M_eye` reprojection, keeping the remap if the reprojection bows out. For a family the
    /// remap claims on a `cb0[4]` camera-position reference alone (see
    /// [`should_reproject_camera_only`]) -- reprojection is the transform it should have had, and the
    /// remap is still better than no per-eye transform at all.
    ReprojectOrRemap,
    /// The `M_eye` reprojection alone, for an allowlisted family the remap found no per-eye operand in.
    Reproject,
    /// The `M_eye` reprojection, falling back to originating the eye index on the free `TEXCOORD3.z`
    /// lane. The terrain families split by whether they tessellate: the non-tessellated variants write
    /// `SV_Position` and reproject like the model families, and only the tessellated ones (whose clip
    /// is built in the domain shader) need the eye-lane inject.
    ReprojectOrEyeInject,
    /// Leave the shader pristine. Either no transform applies to it, or a render-block intercept owns
    /// its draws ([`baked_cb_block_owns_vs`]) and the two mechanisms must not both claim the same
    /// geometry.
    None,
}

/// Resolve which single-pass transform a vertex shader is to be given, from its engine name and the
/// outcome of the `cb0` remap on its bytecode. Callers that a block intercept owns must pass
/// [`VsTransform::None`] to [`remember_vs_transform`] instead of consulting this.
///
/// Decided independently of [`active`], so the decision is on record for the D3D layer even in the
/// census-only dry-run: enabling single-pass afterwards creates no new shader *programs*, so the
/// dry-run pass is the only chance to record a name-derived decision for the shaders already loaded.
pub fn decide_vs_transform(
    name: Option<&str>,
    code: &[u8],
    remap: &Result<Vec<u8>, DxbcError>,
) -> VsTransform {
    match remap {
        Ok(_) if should_reproject_camera_only(name, code) => VsTransform::ReprojectOrRemap,
        Ok(_) => VsTransform::Remap,
        Err(DxbcError::NoPerEyeReferences) if should_reproject(name) => VsTransform::Reproject,
        Err(DxbcError::NoPerEyeReferences) if should_eye_inject(name) => {
            VsTransform::ReprojectOrEyeInject
        }
        Err(_) => VsTransform::None,
    }
}

/// Rewrite `code` per `transform`, or `None` when the shader is to be left pristine (either the
/// decision was [`VsTransform::None`] or every rewrite the decision allows bowed out). `remapped` is
/// the `cb0` remap's output where the caller already has it, so the creation path does not run the
/// rewrite twice; pass `None` to have it run on demand. `name` is used only for the terrain log line.
pub fn apply_vs_transform(
    transform: VsTransform,
    code: &[u8],
    remapped: Option<Vec<u8>>,
    name: Option<&str>,
) -> Option<Vec<u8>> {
    let remap = || remapped.or_else(|| dxbc_stereo::patch_vertex_shader(code).ok());
    match transform {
        VsTransform::Remap => remap(),
        VsTransform::ReprojectOrRemap => dxbc_stereo::reproject_vertex_shader(code)
            .ok()
            .or_else(remap),
        VsTransform::Reproject => dxbc_stereo::reproject_vertex_shader(code).ok(),
        VsTransform::ReprojectOrEyeInject => {
            let (out, path) = match dxbc_stereo::reproject_vertex_shader(code) {
                Ok(blob) => (Some(blob), "reproject"),
                Err(_) => (
                    dxbc_stereo::inject_eye_forward_vertex_shader(code).ok(),
                    "eye-inject",
                ),
            };
            tracing::info!(
                target: "single_pass", stage = "vertex", name = ?name, path,
                transformed = out.is_some(), "terrain: VS transform",
            );
            out
        }
        VsTransform::None => None,
    }
}

/// Record the transform decided for the pristine blob `code`, so [`create_vertex_shader_detour`] can
/// apply the same decision to a shader that reaches D3D without passing the `CreateVertexProgram`
/// hook. Overwrites any previous decision for the same blob, so a re-created program re-decides under
/// the current config rather than being pinned by the first pass.
pub fn remember_vs_transform(code: &[u8], transform: VsTransform) {
    VS_TRANSFORM_CACHE
        .lock()
        .insert(vs_blob_key(code), transform);
}

/// The transform decided at `CreateVertexProgram` time for each pristine vertex-shader blob, keyed by
/// the blob's content.
///
/// **Keyed by content, not by pointer.** The engine owns the bytecode and frees and reuses those
/// allocations, so a blob address is not a stable identity for a decision that has to survive until
/// some later `ResourceCacheReCreateResource` hands the same bytecode to D3D. Hashing every blob costs
/// one pass over a few hundred kilobytes per bundle load -- next to nothing beside the DXBC parse the
/// rewrite does on the same blob, and on the re-create path it *replaces* a parse.
///
/// The bytes the two layers see are the same bytes: `CreateVertexShader` is handed
/// `CreateVertexProgramParams.m_Code` and `m_Size` verbatim, which is the whole reason repointing them
/// substitutes a shader at all. So a shader that passes the hook and is then created unsubstituted --
/// the declined families -- hits its own entry on the very next call, on the same thread.
///
/// **Shared across threads, unlike [`PATCH_PENDING`].** That flag has to be thread-local because it
/// carries no identity: a free-threaded `ID3D11Device` and a streaming loader mean a process-global
/// flag can be consumed by whatever shader another thread creates next. This map is the opposite --
/// the key *is* the identity, so a concurrent creation on another thread can only ever match its own
/// entry, and it must be shared precisely because the deciding thread and the re-creating thread need
/// not be the same one. Both sides are shader-creation paths (load time), never a draw path, so the
/// lock is uncontended in practice.
///
/// Cleared with the patched-shader set in [`reset_patched_vs`], which is also where a bundle bounce
/// re-creates every shader through the hook and so repopulates it; that bounds the map at one entry
/// per distinct vertex shader in a bundle (a few hundred).
static VS_TRANSFORM_CACHE: Mutex<BTreeMap<(usize, u64), VsTransform>> = Mutex::new(BTreeMap::new());

/// The transform [`remember_vs_transform`] recorded for this blob, or `None` if the
/// `CreateVertexProgram` hook never saw it. A recorded [`VsTransform::None`] is a *decision* to leave
/// the shader pristine and is deliberately distinct from never having seen it.
fn cached_vs_transform(code: &[u8]) -> Option<VsTransform> {
    VS_TRANSFORM_CACHE.lock().get(&vs_blob_key(code)).copied()
}

/// A pristine shader blob's identity for [`VS_TRANSFORM_CACHE`]: its length and an FNV-1a hash of its
/// bytes. The length is part of the key rather than folded into the hash so that the cheap half of the
/// comparison is exact.
fn vs_blob_key(code: &[u8]) -> (usize, u64) {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for byte in code {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    (code.len(), hash)
}

/// Record the outcome of running [`dxbc_stereo::patch_vertex_shader`] on one vertex shader, for the
/// census the debug UI reports. Classifies into four buckets: successfully patched; no per-eye
/// references (the baked-WVP / no-position families -- expected; this also covers the families
/// declined in favour of a block intercept, see [`baked_cb_block_owns_vs`]); the
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
    if vs_name_census_enabled()
        && let Some(name) = name
    {
        VS_NAME_CENSUS.lock().insert(name.to_string(), class);
    }
}

/// Whether to record every vertex shader's name against its rewrite class, for
/// [`dump_vs_name_census`] to write out on the next shader reload.
///
/// Read live rather than pinned into the frame snapshot: this runs at shader creation, not on a draw
/// path, and the census is armed by turning it on and reloading the shaders, which is the same action.
fn vs_name_census_enabled() -> bool {
    Config::lock_query(|c| c.stereo.single_pass_dump_vs_name_census)
}

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
/// [`vs_name_census_enabled`] is on), dumped to `vs-name-census.txt` on a shader reload to build the
/// reprojection allowlist from real data.
static VS_NAME_CENSUS: Mutex<BTreeMap<String, PatchClass>> = Mutex::new(BTreeMap::new());

/// Write the vertex-shader name census (see [`VS_NAME_CENSUS`]) to the session directory, grouped by
/// rewrite class with the reprojection candidates first. A no-op unless [`vs_name_census_enabled`] was on
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
    let flags = config_flags();
    flags.has(Flag::SinglePass) && !flags.has(Flag::DryRun) && capability() == Capability::Supported
}

/// Whether the eyes are made to diverge (in addition to [`active`]): distinct per-eye `cb13`,
/// left/right-half viewport routing, and instance doubling of the G-buffer geometry. With it off the
/// patched shaders still run, but both `cb13` eye slots hold the same view, so the two eyes render
/// identically -- the shape the substitution was brought up in, and still the fallback whenever
/// [`compute_dual_eye_rows`] cannot produce per-eye data.
pub fn dual_eye_active() -> bool {
    active() && config_flags().has(Flag::DualEye)
}

/// Whether the per-eye double-draw has been collapsed to a single G-buffer walk: one `game.Draw`
/// produces both eyes (via [`dual_eye_active`]'s `cb13` + viewport routing + instance doubling), the
/// render camera stays centered (no per-eye offset -- both eyes come from `cb13`), and the capture
/// splits the one back buffer into the two eye textures. Requires [`dual_eye_active`]; independent of
/// `single_pass_double_wide`, which only upgrades each eye-half from squished to full resolution.
pub fn collapse_active() -> bool {
    dual_eye_active() && config_flags().has(Flag::Collapse)
}

/// Whether the scene render targets are re-created at 2x per-eye width so each eye-half is full
/// resolution (instead of a squished half of a per-eye-sized target). Requires [`collapse_active`] --
/// it only makes sense for the single walk whose capture split reads one full-width half per eye.
/// Drives the engine render resolution ([`crate::vr::engine_render_resolution`]) and the per-eye
/// capture-texture width (`ui::render`); the XR swapchain stays per-eye width.
pub fn double_wide_active() -> bool {
    collapse_active() && config_flags().has(Flag::DoubleWide)
}

/// Whether the per-eye re-issue intercept for one of the baked-view-projection render blocks is
/// enabled. Read from the frame's config snapshot, so the block `Draw` detours -- which fire for
/// every draw of their type whether or not single-pass is on -- cost a relaxed load rather than a
/// mutex acquisition.
pub fn block_intercept_enabled(block: BlockIntercept) -> bool {
    config_flags().has(match block {
        BlockIntercept::Bark => Flag::Bark,
        BlockIntercept::Foliage => Flag::Foliage,
        BlockIntercept::Occluder => Flag::Occluder,
    })
}

/// Vertex-shader name prefixes whose draws a baked-view-projection block intercept owns end to end,
/// with the intercept's own flag. Names come from `CreateVertexProgramParams.m_Name`; matched by
/// prefix to cover each family's permutations, all of which are issued by the block's `Draw`/`DrawZ`.
///
/// The occluder is deliberately absent: its shader has no `cb0[4]` reference, so the remap never
/// claims it and there is nothing to decline.
const BAKED_CB_VS_NAME_PREFIXES: &[(&str, BlockIntercept)] = &[
    ("vegetationbark", BlockIntercept::Bark),
    ("vegetationfoliage", BlockIntercept::Foliage),
];

/// Whether a baked-view-projection block intercept owns this vertex shader, so the `cb0` remap must
/// leave it pristine.
///
/// Every vegetation vertex shader that reads `cb0` reads **only** `cb0[4]`, the camera world position
/// -- the foliage family as the world-space origin of its wind-noise lookup, the bark family as the
/// offset paired with a view-projection baked into `cb1`. Neither takes its clip position from `cb0`
/// (`dcl_constantbuffer CB0[5]` cannot even address the view-projection rows), so the remap gives them
/// no per-eye clip: both eyes keep the collapsed centre view, and near geometry rendered at zero
/// disparity reads as swimming against the parallaxed world around it. Being remapped also costs them
/// the fix: the intercept that *does* reproject their baked matrix stands down for a patched shader
/// ([`reproject_baked_cb_per_eye`]), because a patched shader is supposed to be producing both eyes
/// already.
///
/// So the two go together, exactly as they are gated: while the block's flag is on, its `Draw`/`DrawZ`
/// is re-issued per eye with the baked matrix reprojected, and its shaders are declined here. With the
/// flag off both halves revert and the family is remapped as before.
///
/// The bytecode is the final say, not the name: a permutation that really does read the global
/// view-projection is left to the remap, so a future bundle that moves one of these families onto
/// `cb0` does not silently lose its position path.
///
/// The decline is recorded as [`VsTransform::None`] against the blob, so the D3D-level re-create path
/// honours it too -- that path has no name to decline by, and left to itself it would remap the very
/// shaders this declined.
pub fn baked_cb_block_owns_vs(name: Option<&str>, code: &[u8]) -> bool {
    let Some(name) = name else {
        return false;
    };
    BAKED_CB_VS_NAME_PREFIXES
        .iter()
        .any(|(prefix, block)| name.starts_with(prefix) && block_intercept_enabled(*block))
        && dxbc_stereo::reads_global_view_projection(code).is_ok_and(|reads| !reads)
}

/// A render block with a baked-view-projection per-eye intercept.
#[derive(Clone, Copy)]
pub enum BlockIntercept {
    Bark,
    Foliage,
    Occluder,
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
        // One slot: slot 1 is left unbound, so the slots are no longer uniform.
        set_viewport_slots_uniform(false);
        // SAFETY: `context` is the live immediate context; the trampoline is the original function.
        unsafe { detour.call(context, 1, std::slice::from_ref(viewport).as_ptr()) };
    }
}

/// One eye's re-issue of a terrain-detail draw: the reprojected `cb1` (four float4 rows) to stage on
/// vertex slot 1, and the eye-half viewport to render into.
struct TerrainDetailEyePass {
    cb1: [f32; 16],
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
    [f32; 16],
    D3D11_VIEWPORT,
    EngineContext,
)> {
    if !terrain_active() {
        return None;
    }
    let (m_eye, full, d3d) = baked_cb_intercept_ready("terrain-detail", BoundVsGate::Checked)?;

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
        TerrainDetailEyePass { cb1 }
    });
    let mut cb1_center_rows = [0.0f32; 16];
    for (k, center) in cb1_center.iter().enumerate() {
        cb1_center_rows[k * 4..k * 4 + 4].copy_from_slice(&center.to_array());
    }
    Some((passes, cb1_center_rows, full, d3d))
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

/// The engine's immediate context, borrowed for the duration of a render-thread operation.
///
/// Borrowed rather than cloned: the per-eye re-issues take this per draw, and cloning the
/// `ID3D11DeviceContext` would be an `AddRef`/`Release` pair each time on a path whose whole purpose
/// is cutting per-draw cost. The engine owns the context for the process's life, so a `'static`
/// borrow is sound for as long as the engine is up -- which the render thread guarantees.
#[derive(Clone, Copy)]
struct EngineContext(&'static jc3gi::graphics_engine::device::Context);

impl EngineContext {
    /// The engine's immediate context, or `None` if the device/context is not live yet.
    fn get() -> Option<Self> {
        // SAFETY: read on the render thread, where the engine device/context pointers are stable.
        unsafe {
            let ge = GraphicsEngine::get()?;
            let device = ge.m_Device.as_ref()?;
            Some(Self(device.m_Context.as_ref()?))
        }
    }

    /// Run `f` on the D3D immediate context under the engine's own context mutex, which every other
    /// path in the mod that touches the context also takes.
    fn with_lock<R>(self, f: impl FnOnce(&ID3D11DeviceContext) -> R) -> R {
        // SAFETY: `m_Mutex` is the engine's live critical section for this context.
        unsafe {
            EnterCriticalSection(self.0.m_Mutex);
            let result = f(&self.0.m_Context);
            LeaveCriticalSection(self.0.m_Mutex);
            result
        }
    }
}

/// Bind `viewport` to both viewport slots of the immediate context. Binding two slots (rather than
/// one) passes the collapse viewport detour through untouched -- it only special-cases a single-slot
/// set -- and the terrain-detail VS has no `SV_ViewportArrayIndex`, so it rasterizes into slot 0.
fn bind_both_viewport_slots(d3d: EngineContext, viewport: D3D11_VIEWPORT) {
    // SAFETY: a two-element slice is a valid viewport array.
    d3d.with_lock(|ctx| unsafe { ctx.RSSetViewports(Some(&[viewport, viewport])) });
}

/// The immediate context's currently-bound viewport slots, captured so a per-eye re-issue can put back
/// exactly what the surrounding pass had rather than assume it was the collapse's full viewport. Only
/// the two slots single-pass uses are captured.
#[derive(Clone, Copy)]
struct ViewportSlots {
    slots: [D3D11_VIEWPORT; 2],
    count: u32,
}

fn capture_viewport_slots(d3d: EngineContext) -> ViewportSlots {
    let mut count = 2u32;
    let mut slots = [D3D11_VIEWPORT::default(); 2];
    // SAFETY: `count` is the length of `slots`, as `RSGetViewports` requires.
    d3d.with_lock(|ctx| unsafe { ctx.RSGetViewports(&mut count, Some(slots.as_mut_ptr())) });
    // Trailing zero-width slots are what a runtime writes for the elements it had nothing bound for,
    // and whether it also writes the count back is implementation-defined -- so take the width, not the
    // count, as the authority on how many slots there really are. Restoring a zero-width slot 1 would
    // clip every later right-eye primitive to nothing.
    let count = slots
        .iter()
        .take(count.min(2) as usize)
        .take_while(|v| v.Width > 0.0)
        .count() as u32;
    ViewportSlots { slots, count }
}

/// Re-bind the slots [`capture_viewport_slots`] recorded. Goes through the trampoline rather than the
/// vtable entry: a restored single-slot set would otherwise be re-recorded by
/// [`rs_set_viewports_detour`] as the collapse's full viewport.
fn restore_viewport_slots(d3d: EngineContext, saved: ViewportSlots) {
    if saved.count == 0 {
        return;
    }
    let Some(detour) = RS_SET_VIEWPORTS.get() else {
        return;
    };
    // A single restored slot leaves slot 1 unbound, which is exactly the state a patched shader cannot
    // survive; flag it so the next out-of-range patched draw repairs it.
    set_viewport_slots_uniform(saved.count == 2 && saved.slots[0] == saved.slots[1]);
    // SAFETY: the context is the live immediate context and `slots` holds `count` viewports.
    d3d.with_lock(|ctx| unsafe {
        detour.call(ctx.as_raw(), saved.count, saved.slots.as_ptr());
    });
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
    let Some((passes, cb1_center, full, d3d)) = (unsafe { terrain_detail_eye_passes(this, rc) })
    else {
        return false;
    };
    // SAFETY: `rc` is live per the caller contract.
    let ctx = unsafe { render_context_graphics_context(rc) };
    per_eye_halves(full, d3d, &mut |eye| {
        // SAFETY: `ctx` is the render context's live graphics context; `cb1` is four float4 rows.
        unsafe { SetVertexProgramConstants(ctx, 1, 0, passes[eye].cb1.as_ptr(), 4) };
        draw();
    });
    // Put the centre transform back, as `screen_uv_cb_per_eye` does with the rows it biases. The
    // block type stages this constant once per pass rather than per draw, so leaving eye 1's
    // reprojection behind hands it to anything later in the pass that reads vertex `cb1[0..3]` --
    // including a terrain-detail draw this intercept declines, which it can do per draw, since
    // `BoundVsGate::Checked` reads the live bound-shader flag.
    // SAFETY: as above; `cb1_center` is the same four float4 rows, un-reprojected.
    unsafe { SetVertexProgramConstants(ctx, 1, 0, cb1_center.as_ptr(), 4) };
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
    /// The graphics context (`HContext_t*`) the re-issued block stages into. The detour sees *every*
    /// vertex-constant stage in the process, and `(slot, offset)` alone does not identify the block --
    /// `RenderBlockTerrainPatch` also stages four rows at vertex `cb1` offset 0 -- so a stage from any
    /// other context in the armed window would otherwise be reprojected by this eye's matrix. Held as
    /// an address rather than a pointer so the static stays `Sync`; it is only ever compared.
    ctx: usize,
    cb_index: i32,
    reg_offset: u32,
    m_eye: glam::Mat4,
}

/// `eye + 1` while a render-block per-eye re-issue is in flight, `0` otherwise.
///
/// A re-issue re-drives the block's whole `Draw`, so every draw and viewport call the block makes
/// passes back through this module's own detours. Without a marker they would compound the split the
/// re-issue just set up: a patched `DrawIndexed` inside would be instance-doubled and re-split
/// (drawing the geometry twice in each eye), and any `Draw` or single-slot `RSSetViewports` would
/// restore the full width, un-splitting the eye half mid-re-issue.
static PER_EYE_REISSUE: AtomicUsize = AtomicUsize::new(0);

/// The eye whose per-eye re-issue is currently in flight, or `None` outside one.
fn per_eye_reissue_eye() -> Option<usize> {
    match PER_EYE_REISSUE.load(Ordering::Acquire) {
        0 => None,
        marker => Some(marker - 1),
    }
}

/// Raises [`PER_EYE_REISSUE`] for one eye for as long as it lives, carrying the previous marker so a
/// nested re-issue restores rather than clears it.
struct PerEyeReissue(usize);

impl PerEyeReissue {
    /// Saves and restores the previous marker rather than clearing it, the same shape
    /// [`set_current_pass`] uses. Nothing today re-issues inside a re-issue -- no intercepted block's
    /// `Draw` reaches another intercepted block's `Draw` -- but nothing states or enforces that
    /// either, and clearing would un-guard the remainder of the outer loop the moment it stopped
    /// holding: this module's own draw detours would start splitting geometry the outer re-issue had
    /// already split, which is the doubled-geometry artifact the marker exists to prevent.
    fn enter(eye: usize) -> Self {
        let previous = PER_EYE_REISSUE.swap(eye + 1, Ordering::Release);
        Self(previous)
    }
}

impl Drop for PerEyeReissue {
    fn drop(&mut self) {
        PER_EYE_REISSUE.store(self.0, Ordering::Release);
    }
}

/// Fast-path guard for [`set_vertex_program_constants_detour`]: a relaxed load skips the mutex on every
/// un-armed stage (the common case -- the detour sees every VS constant upload in the frame).
static REPROJECT_ARMED: AtomicBool = AtomicBool::new(false);
static REPROJECT_UPLOAD: Mutex<Option<ReprojectUpload>> = Mutex::new(None);

fn arm_reproject(upload: ReprojectUpload) {
    *REPROJECT_UPLOAD.lock() = Some(upload);
    REPROJECT_FIRED.store(false, Ordering::Relaxed);
    REPROJECT_ARMED.store(true, Ordering::Release);
}

/// Disarm, reporting whether the block actually staged the constants the arm was waiting for.
fn disarm_reproject() -> bool {
    REPROJECT_ARMED.store(false, Ordering::Release);
    *REPROJECT_UPLOAD.lock() = None;
    REPROJECT_FIRED.load(Ordering::Relaxed)
}

/// Whether the armed reprojection matched a stage before it was disarmed.
///
/// A block that takes a different internal path than the intercept expects -- the occluder's
/// instanced path, selected by the `gfx.occluders.use_instancing` cvar, reads `cb0`'s
/// `m_VPGlobals` with per-instance world rows instead of baking a `cb1` matrix -- stages nothing the
/// arm can reproject, so the re-issue would draw the same mono geometry into both eye halves. This
/// makes that condition observable rather than silent; nothing in the mod forces the cvar, so it is
/// the only thing standing between the intercept and its precondition.
static REPROJECT_FIRED: AtomicBool = AtomicBool::new(false);

/// Whether `viewport` covers the scene render target, as opposed to one of the reduced-resolution
/// post-effect targets. Compared against [`crate::stereo::render_size`] -- the engine's
/// `m_BackBufferLinear`, which under the collapse's double-wide is the full two-eye width -- with a
/// pixel of slack for the engine's own rounding.
fn is_scene_sized(viewport: D3D11_VIEWPORT) -> bool {
    let Some((width, height)) = crate::stereo::render_size() else {
        // Without a render size to compare against, take the viewport: the alternative is never
        // recording one and losing the eye split entirely.
        return true;
    };
    (viewport.Width - width as f32).abs() <= 1.0 && (viewport.Height - height as f32).abs() <= 1.0
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

/// Whether a baked-cb per-eye re-issue has to satisfy itself that the block's vertex shader was not
/// claimed by the `cb0` remap.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BoundVsGate {
    /// Stand down if a patched vertex shader is bound. For a block whose shaders the remap may claim
    /// (patching is by operand detection, not by name), a patched shader already renders both eyes from
    /// one draw: re-issuing would draw the geometry twice per eye, from `cb13`'s eye slot rather than
    /// the reprojected constant.
    Checked,
    /// Run regardless. For a block whose shaders are declined at creation while its own flag is on
    /// ([`baked_cb_block_owns_vs`], a decline both creation paths honour), so none of them can be
    /// patched — and where the check would be
    /// wrong anyway: it reads the shader bound by the *previous* draw, since the block binds its own
    /// inside the `Draw` being wrapped. A bark block drawn between two patched model draws would stand
    /// down on the neighbour's shader and lose its parallax.
    Owned,
}

/// The state a baked-cb per-eye re-issue needs, or `None` when it must not run: the two per-eye `M_eye`
/// matrices, the collapse full viewport, and the immediate context. Requires the collapse (a single
/// centered walk) and the G-buffer pass range -- outside the range the eye-half split does not apply
/// (the shadow-cascade and reflection passes reuse these blocks' `DrawZ`, and eye-splitting a
/// shadow-atlas draw would corrupt it), and outside the collapse re-issuing per eye is wrong.
fn baked_cb_intercept_ready(
    site: &'static str,
    gate: BoundVsGate,
) -> Option<([glam::Mat4; 2], D3D11_VIEWPORT, EngineContext)> {
    if gate == BoundVsGate::Checked && BOUND_VS_PATCHED.load(Ordering::Relaxed) {
        warn_intercept_declined_on_patched_vs(site);
        return None;
    }
    eye_split_state()
}

/// Warn, at most once per `site`, that a per-eye intercept stood itself down because a patched vertex
/// shader was bound.
///
/// The gate exists so an intercept and the shader rewrite cannot both claim the same geometry. But a
/// family whose *own* vertex shaders the rewrite claims declines on every draw, for the whole session,
/// silently -- the intercept is then dead code that reports success, and its flag documents a fix that
/// never runs. Which families those are is not decidable from the bytecode alone: it depends on which
/// permutations a session loads and on what happens to be bound when the block draws. So record it
/// from the one place that knows.
///
/// A single line per site over a whole session is the signal that the intercept never ran at all.
fn warn_intercept_declined_on_patched_vs(site: &'static str) {
    if INTERCEPT_DECLINED_ON_PATCHED_VS.lock().insert(site) {
        tracing::warn!(
            target: "single_pass",
            "{site} per-eye intercept declined: a patched vertex shader was bound, so the shader \
             rewrite already owns this draw. If this is the only line for {site} all session, the \
             intercept never ran and its flag is documenting a fix that is not happening.",
        );
    }
}

static INTERCEPT_DECLINED_ON_PATCHED_VS: Mutex<BTreeSet<&'static str>> =
    Mutex::new(BTreeSet::new());

/// The state every per-eye re-issue needs, without the bound-shader gate: the two per-eye `M_eye`
/// matrices, the collapse full viewport, and the immediate context. Requires the collapse (a single
/// centered walk) and the G-buffer pass range -- outside the range the eye-half split does not apply
/// (the shadow-cascade and reflection passes reuse these blocks' `DrawZ`, and eye-splitting a
/// shadow-atlas draw would corrupt it), and outside the collapse re-issuing per eye is wrong.
fn eye_split_state() -> Option<([glam::Mat4; 2], D3D11_VIEWPORT, EngineContext)> {
    if !collapse_active() || !in_gbuffer_range() {
        return None;
    }
    let m_eye = (*CURRENT_M_EYE.lock())?;
    let full = (*COLLAPSE_FULL_VIEWPORT.lock())?;
    let d3d = EngineContext::get()?;
    Some((m_eye, full, d3d))
}

/// Re-issue a render block's `Draw` once per eye, reprojecting the four `float4` entries the block bakes
/// on `rc`'s graphics context at (`cb_index`, `reg_offset`) by that eye's `M_eye` and binding the eye's
/// half-viewport. Returns `false`
/// when the intercept must not run (collapse inactive, or the dual-eye state not yet published), in which
/// case the caller draws normally once.
///
/// The block writes its view-projection into a constant buffer inside its own `Draw`, so rather than
/// replicate that bake, this arms [`set_vertex_program_constants_detour`] to reproject the block's own
/// upload for the duration of a wrapped original-`Draw` call. It covers every draw kind (plain,
/// CPU-instanced, GPU-indirect) uniformly, since it re-drives the block's whole `Draw`, and each of the
/// two re-issues renders into its eye's viewport half. The re-issue raises [`PER_EYE_REISSUE`] so the
/// draw and viewport detours leave the block's own calls alone rather than compounding the split, and
/// `gate` decides whether it also stands down for a patched bound vertex shader (see [`BoundVsGate`]).
///
/// # Safety
///
/// `rc` must be the live [`RenderContext`] the detoured `Draw` received, and `draw` must invoke the
/// block's original `Draw` trampoline.
pub unsafe fn reproject_baked_cb_per_eye(
    rc: *const RenderContext,
    cb_index: i32,
    reg_offset: u32,
    gate: BoundVsGate,
    mut draw: impl FnMut(),
) -> bool {
    // SAFETY: forwarded unchanged from this function's own contract.
    unsafe { reproject_baked_cb_per_eye_staged(rc, cb_index, reg_offset, gate, |_| draw()) }
}

/// [`reproject_baked_cb_per_eye`] for a block that *also* has per-eye state of its own to stage:
/// `render` receives the eye and does that staging before invoking the block's `Draw`, all while the
/// vertex reprojection is armed.
///
/// The two halves act on different uploads and must not be conflated. The armed reprojection rewrites
/// the block's own **vertex** constant stage at (`cb_index`, `reg_offset`) as it passes through
/// [`set_vertex_program_constants_detour`], which is what gives the geometry its parallax; anything
/// `render` stages is its own upload, on whatever stage and slot it chooses, and is left alone.
///
/// # Safety
///
/// `rc` must be the live [`RenderContext`] the detoured `Draw` received, and `render` must invoke the
/// block's original `Draw` trampoline.
pub unsafe fn reproject_baked_cb_per_eye_staged(
    rc: *const RenderContext,
    cb_index: i32,
    reg_offset: u32,
    gate: BoundVsGate,
    mut render: impl FnMut(usize),
) -> bool {
    let Some((m_eye, full, d3d)) = baked_cb_intercept_ready("baked-cb", gate) else {
        return false;
    };
    // SAFETY: `rc` is the live render context the detoured `Draw` received.
    let ctx = unsafe { render_context_graphics_context(rc) } as usize;
    per_eye_halves(full, d3d, &mut |eye| {
        arm_reproject(ReprojectUpload {
            ctx,
            cb_index,
            reg_offset,
            m_eye: m_eye[eye],
        });
        render(eye);
        if !disarm_reproject() {
            warn_reproject_never_fired(cb_index, reg_offset);
        }
    });
    bind_both_viewport_slots(d3d, full);
    true
}

/// Re-issue a render block's `Draw` once per eye with a *projective screen-UV* constant biased into
/// that eye's half of the double-wide target, restoring the staged rows afterwards. Returns `false`
/// when the intercept must not run (the same gate every other per-eye re-issue takes), in which case
/// the caller draws normally once.
///
/// `base` is the four `float4` rows the block's *type* staged at (`cb_index`, `reg_offset`): a
/// world→screen-UV transform with the NDC→UV `x·0.5 + w·0.5` already folded in, which the vertex
/// shader applies with a multiply-add chain over the four registers (so they are the matrix's rows in
/// the row-vector convention) and hands on as a projective `TEXCOORD1` for the pixel shader to divide
/// by `w`. The resulting UV is normalized over the *viewport*, i.e. over one eye's half, while the
/// buffers it indexes are the whole double-wide target -- so each eye reads the entire two-eye image
/// across its surface, and the mismatch slides as the camera moves. Composing one more bias per eye,
/// `u' = (u + eye) · 0.5`, maps it back into that eye's half; row-wise, and because `u` and `w` are
/// both per-row sums, that is `row.x ← row.x · 0.5 + row.w · 0.5 · eye`.
///
/// Unlike [`reproject_baked_cb_per_eye`] this restages rather than intercepts, because the constant is
/// staged by the block *type*'s per-pass setup rather than inside the `Draw` being re-issued (the same
/// reason [`terrain_detail_per_eye`] stages its own rows). It is also deliberately *not* reprojected
/// by `M_eye`: the geometry these blocks rasterize still comes from the collapsed centre view, so the
/// UV must describe where that geometry actually landed, not where the eye's own projection would have
/// put it.
///
/// # Safety
///
/// `rc` must be the live [`RenderContext`] the detoured `Draw` received, and `draw` must invoke the
/// block's original `Draw` trampoline.
pub unsafe fn screen_uv_cb_per_eye(
    rc: *const RenderContext,
    cb_index: i32,
    reg_offset: u32,
    base: [f32; 16],
    mut draw: impl FnMut(),
) -> bool {
    let Some((_, full, d3d)) =
        baked_cb_intercept_ready("legacy-water screen UV", BoundVsGate::Checked)
    else {
        return false;
    };
    // SAFETY: `rc` is live per the caller contract.
    let ctx = unsafe { render_context_graphics_context(rc) };
    per_eye_halves(full, d3d, &mut |eye| {
        let mut rows = base;
        for k in 0..4 {
            rows[k * 4] = base[k * 4].mul_add(0.5, base[k * 4 + 3] * 0.5 * eye as f32);
        }
        // SAFETY: `ctx` is the render context's live graphics context; `rows` is four float4 rows.
        unsafe { SetVertexProgramConstants(ctx, cb_index, reg_offset, rows.as_ptr(), 4) };
        draw();
    });
    // Put the type's own rows back. It stages them once per pass, ahead of every block it covers, so
    // leaving the second eye's bias behind would hand it to any later draw this intercept declines.
    // SAFETY: as above; `base` is the four rows the type staged.
    unsafe { SetVertexProgramConstants(ctx, cb_index, reg_offset, base.as_ptr(), 4) };
    true
}

/// Run `render` once per eye with that eye's half-viewport pinned on both slots, restoring the
/// collapse's full viewport afterwards. Returns `false` when the intercept must not run -- the same
/// gate every other per-eye re-issue takes ([`baked_cb_intercept_ready`]) -- in which case the caller
/// must do its work itself, exactly once.
///
/// The bare form of [`reproject_baked_cb_per_eye`] and [`screen_uv_cb_per_eye`], for a block whose
/// per-eye state is not a vertex constant this module knows how to transform: `render` receives the
/// eye and does whatever staging that block needs before invoking its own `Draw`. Each call is
/// bracketed by [`PER_EYE_REISSUE`], so the draw and viewport detours leave the block's own
/// submissions alone instead of splitting them a second time.
/// `site` names the calling block for the decline diagnostic (see
/// [`warn_intercept_declined_on_patched_vs`]); it is the caller's identity rather than this helper's,
/// because a bound-shader decline is a fact about that block's own shaders.
pub fn draw_per_eye_half(site: &'static str, mut render: impl FnMut(usize)) -> bool {
    let Some((_, full, d3d)) = baked_cb_intercept_ready(site, BoundVsGate::Checked) else {
        return false;
    };
    per_eye_halves(full, d3d, &mut render);
    true
}

/// [`draw_per_eye_half`] for a block that binds its own vertex programs *inside* the `Draw` being
/// re-issued, so the shader bound when the gate runs is the previous draw's and says nothing about
/// this one.
///
/// [`baked_cb_intercept_ready`]'s bound-shader gate exists to leave already-patched geometry to the
/// patched path, and it reads the shader that `VSSetShader` last saw. For a block whose type binds
/// its programs in a per-pass setup that is a fair proxy; for one that binds them per draw it is a
/// coin flip on whatever drew before it, which would make the re-issue fire intermittently. Callers
/// take this variant only when the block's own vertex programs are provably outside the rewrite.
///
/// "Builds clip from a constant buffer of its own" is *not* on its own enough to establish that: the
/// rewrite claims any shader referencing the per-eye `cb0` entries, and `cb0[4]` is a camera position
/// that such a family may still read for shading. Where a family reads it, the shader has to be
/// declined at creation instead ([`baked_cb_block_owns_vs`]).
pub fn draw_per_eye_half_ignoring_bound_vs(mut render: impl FnMut(usize)) -> bool {
    let Some((_, full, d3d)) = eye_split_state() else {
        return false;
    };
    per_eye_halves(full, d3d, &mut render);
    true
}

/// Run `render` once per eye with that eye's half of `full` pinned on both viewport slots, then put
/// `full` back for the draws that follow.
fn per_eye_halves(full: D3D11_VIEWPORT, d3d: EngineContext, render: &mut impl FnMut(usize)) {
    for eye in 0..2 {
        let _reissue = PerEyeReissue::enter(eye);
        bind_both_viewport_slots(d3d, eye_half_viewport(full, eye));
        render(eye);
    }
    bind_both_viewport_slots(d3d, full);
}

/// Warn, at most once per (`cb_index`, `reg_offset`), that a per-eye re-issue found nothing to
/// reproject -- see [`REPROJECT_FIRED`].
fn warn_reproject_never_fired(cb_index: i32, reg_offset: u32) {
    let key = (cb_index as i64) << 32 | i64::from(reg_offset);
    if REPROJECT_NEVER_FIRED.lock().insert(key) {
        tracing::warn!(
            target: "single_pass",
            "baked-cb per-eye re-issue at cb{cb_index}[{reg_offset}..{}] staged nothing to \
             reproject: the block took a path that does not bake this constant, so both eyes get \
             the same view",
            reg_offset + 4,
        );
    }
}

static REPROJECT_NEVER_FIRED: Mutex<BTreeSet<i64>> = Mutex::new(BTreeSet::new());

type SetVertexProgramConstantsFn =
    unsafe extern "system" fn(*mut c_void, i32, u32, *const f32, u32);
static SET_VERTEX_PROGRAM_CONSTANTS: DetourSlot<SetVertexProgramConstantsFn> = DetourSlot::new();

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
    // The clustered light-assignment view matrix, when the per-eye froxel split is assigning this
    // eye's lights from this eye's position. Checked first because it is scoped to a single call
    // inside `DrawClustered` and cannot overlap a baked-cb re-issue, which only arms in the G-buffer
    // range.
    if let Some(rows) =
        crate::hooks::graphics_engine::clustered_lighting::substitute_assignment_view(
            ctx as usize,
            cb_index,
            start_offset,
            data,
            count,
        )
    {
        // SAFETY: `rows` holds the 4 float4 rows the call stages and outlives it; `detour.call` is the
        // trampoline.
        unsafe { detour.call(ctx, cb_index, start_offset, rows.as_ptr(), count) };
        return;
    }
    if REPROJECT_ARMED.load(Ordering::Acquire)
        && !data.is_null()
        && let Some(up) = *REPROJECT_UPLOAD.lock()
        && ctx as usize == up.ctx
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
        REPROJECT_FIRED.store(true, Ordering::Relaxed);
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

/// Marks the render thread as inside the G-buffer geometry pass range
/// (`RP_Z_OCCLUDERS..RP_FIRST_SCENE`) until the returned guard drops. The dual-eye viewport split and
/// instance doubling apply only here -- so shadow/lighting/post passes, which reuse the same patched
/// shaders but are not double-wide, keep the identical-viewport behaviour.
///
/// A guard rather than a matching pair of calls: the range wraps a re-entrant engine call, and any
/// non-local exit from it (a panic unwinding through the detour, an early return added later) that
/// skipped the clear would leave the flag raised for the rest of the session -- after which *every*
/// shadow and reflection draw is instance-doubled and eye-split.
#[must_use = "the G-buffer range ends when the guard is dropped"]
pub fn enter_gbuffer_range() -> GBufferRange {
    IN_GBUFFER_RANGE.store(true, Ordering::Relaxed);
    GBufferRange(())
}

/// Holds [`in_gbuffer_range`] true; see [`enter_gbuffer_range`].
pub struct GBufferRange(());

impl Drop for GBufferRange {
    fn drop(&mut self) {
        // The guard is the only writer that should ever lower the flag. Finding it already down means
        // something else cleared it while the range was still running -- every draw between that point
        // and here was treated as out-of-range -- so count it rather than losing it in the clear.
        if !IN_GBUFFER_RANGE.swap(false, Ordering::Relaxed) {
            RANGE_TORN.fetch_add(1, Ordering::Relaxed);
        }
        // The per-eye matrices belong to the range that just ended; see [`clear_gbuffer_range`].
        *CURRENT_M_EYE.lock() = None;
        // The collapse's per-draw split ([`ensure_collapse_viewport`]) leaves the two slots holding
        // different eye halves, and that state outlives the range: everything drawn between here and
        // the next engine viewport bind would route its odd-parity instances into the other eye's half.
        // Put the slots back to a single region now, which also covers the draws nothing detours (the
        // GPU-indirect ones) -- the per-draw repair cannot see those.
        if collapse_active() {
            unify_viewport_slots();
        }
    }
}

/// Force the range closed, whether or not a guard is live, so a range left open by a torn-down or
/// interrupted dispatch cannot bleed into the next one. See [`begin_frame`] for the caller.
fn clear_gbuffer_range() {
    IN_GBUFFER_RANGE.store(false, Ordering::Relaxed);
    // The per-eye matrices belong to the range that just ended. Dropping them means a re-issue that
    // somehow runs outside a range -- or in a later frame where `compute_dual_eye_rows` declined to
    // publish -- reprojects with nothing rather than with a stale head pose.
    *CURRENT_M_EYE.lock() = None;
}

fn in_gbuffer_range() -> bool {
    IN_GBUFFER_RANGE.load(Ordering::Relaxed)
}

/// The render pass currently being walked, published by the `RenderPass::DoDraw` detour so the draw
/// detours -- which see only a D3D context -- can tell which pass a draw belongs to. [`NO_PASS`]
/// stands for "outside any pass"; no real id collides with it (`m_Index` is a byte and the engine's
/// highest pass is `0x96`).
static CURRENT_PASS: AtomicU8 = AtomicU8::new(NO_PASS);

const NO_PASS: u8 = 0xFF;

/// Publish the pass being drawn for the duration of one `DoDraw`. Returns the previous value so the
/// caller restores it rather than clearing, since a block-level re-issue can nest one pass inside
/// another.
///
/// Takes the engine's `m_Index` in its own `i16` form; an id outside the byte range is not a pass this
/// module can classify, so it reads as [`NO_PASS`] rather than truncating into a real pass's slot.
pub fn set_current_pass(pass: i16) -> u8 {
    let pass = u8::try_from(pass).unwrap_or(NO_PASS);
    CURRENT_PASS.swap(pass, Ordering::Relaxed)
}

/// Restore a pass id previously returned by [`set_current_pass`].
pub fn restore_current_pass(pass: u8) {
    CURRENT_PASS.store(pass, Ordering::Relaxed);
}

fn current_pass_id() -> u8 {
    CURRENT_PASS.load(Ordering::Relaxed)
}

/// Per-pass tally of non-indexed (`Draw`, slot 13) submissions seen inside a range while collapsed.
///
/// This exists to replace inference with measurement. Which passes actually reach slot 13 is the fact
/// that decides whether a family is being rasterised across the whole double-wide target instead of
/// one eye's half, and it is otherwise only obtainable from a frame capture. Indexed by pass id.
static SLOT13_BY_PASS: [AtomicU32; 256] = [const { AtomicU32::new(0) }; 256];

/// Drain the per-pass slot-13 census as `(pass_id, count)` for the passes that saw any, most frequent
/// first. Reported in the per-range diagnostic line.
fn drain_slot13_census() -> Vec<(u8, u32)> {
    let mut seen: Vec<(u8, u32)> = SLOT13_BY_PASS
        .iter()
        .enumerate()
        .filter_map(|(pass, count)| {
            let count = count.swap(0, Ordering::Relaxed);
            (count > 0).then_some((pass as u8, count))
        })
        .collect();
    seen.sort_unstable_by_key(|&(_, count)| std::cmp::Reverse(count));
    seen
}

/// Open a new real frame: advance the frame ordinal the diagnostics are keyed to, and fold the
/// previous frame's already-instanced exposure into the history.
///
/// The exposure counters are per *frame*, but the G-buffer range is entered once per
/// `DrawRenderPassRange` call -- three times per dispatch under the collapse (`DrawGBuffer`
/// `0x2F..0x55`, `Draw` `0x56..0x96`, `DrawPosteffects` `0x96..0x97`; see
/// `docs/mod/single-pass-stereo.md`). Folding them at a range boundary therefore cut the frame into
/// three unequal windows and reported each as if it were a frame; the fold belongs here, where a
/// frame actually begins.
pub fn begin_frame() {
    // The leaked-range clear deliberately does *not* happen here: it belongs to the thread that
    // raises and lowers the flag, and [`begin_dispatch`] does it there. With the frame tail deferred
    // this thread runs concurrently with the previous frame's still-walking dispatch, so clearing
    // from here tears a live range out from under it.
    //
    // The frame that just ended owns both the exposure fold and, if it was a diagnostic frame, the
    // trailing exposure line -- so it carries the same ordinal as its own range lines, which were
    // emitted before the ordinal advanced.
    let logged = diagnostic_frame();
    let exposure = accumulate_instanced_exposure();
    if logged {
        log_instanced_exposure(exposure);
    }
    FRAME_ORDINAL.fetch_add(1, Ordering::Relaxed);
}

/// Open a dispatch on the draw thread: pin this dispatch's config flags, and close a G-buffer range
/// left open by an interrupted dispatch, before this one's first range is entered.
///
/// This is the only place the range flag may be forced down from outside its guard. The flag is
/// written and read exclusively on the draw thread, and the dispatch prologue is the one point in
/// that thread's sequence where no range can be live -- so a clear here cannot interleave with a range
/// in progress, as the former frame-start clear on the game thread could once the frame tail was
/// deferred.
///
/// The config flags are pinned here for the same reason and at the same point. They gate state this
/// thread arms and restores in pairs -- the eye-half viewport, the armed constant reprojection, the
/// per-eye re-issue loops, the range guard's own viewport repair -- and a flag that moved between an
/// arm and its restore would leave that state raised for the rest of the frame. Sampled per dispatch
/// rather than per frame because a dispatch is the unit the draw thread actually walks: under the
/// collapse the game thread is already a frame ahead of it.
pub fn begin_dispatch() {
    pin_dispatch_config_flags();
    clear_gbuffer_range();
}

/// Whether this frame is one the per-frame single-pass diagnostics log on.
fn diagnostic_frame() -> bool {
    FRAME_ORDINAL
        .load(Ordering::Relaxed)
        .is_multiple_of(DIAGNOSTIC_FRAME_CADENCE)
}

/// How often the single-pass bring-up diagnostics log, in real frames. Every range of a logged frame
/// reports, so the frame's whole pass-range sequence appears together and can be read as one frame
/// rather than as unrelated samples.
const DIAGNOSTIC_FRAME_CADENCE: usize = 120;

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

/// Whether both viewport slots are known to be bound to the **same** region, so a patched shader's
/// `SV_ViewportArrayIndex = SV_InstanceID & 1` rasterises identically whichever parity it computes.
///
/// A patched vertex shader writes the viewport index unconditionally -- the bytecode has no idea which
/// pass it is in -- but the eye-half pair is only ever bound for the G-buffer geometry. Everywhere else
/// (the shadow cascades, the reflection prepass, the post and UI passes) slot 1 must be a duplicate of
/// slot 0, or the odd-parity instances of an already-instanced draw rasterise into a region that pass
/// never meant to write. [`rs_set_viewports_detour`] keeps that true for every viewport the engine
/// binds, but the collapse's own per-draw split ([`ensure_collapse_viewport`]) leaves the two slots
/// holding *different* halves, and that state outlives the G-buffer range until the next engine bind.
/// This flag tracks which of the two the slots are in, so [`unify_viewport_slots`] can repair the
/// split state without reading the device on every draw.
///
/// Conservative: anything that leaves the slots in an unknown state clears it, costing at most one
/// redundant repair.
static VIEWPORT_SLOTS_UNIFORM: AtomicBool = AtomicBool::new(false);

/// Record what the two viewport slots now hold. Called by every path that binds them.
fn set_viewport_slots_uniform(uniform: bool) {
    VIEWPORT_SLOTS_UNIFORM.store(uniform, Ordering::Relaxed);
}

/// Re-bind viewport slot 0's region to **both** slots, if they are not already known to hold the same
/// one. Restores the invariant [`VIEWPORT_SLOTS_UNIFORM`] describes outside the G-buffer range.
///
/// Slot 0 is left exactly as it was found, so this cannot change where an even-parity primitive lands
/// -- the only difference it makes is that the odd-parity ones stop being routed somewhere else. The
/// device read is behind the flag, so the common (already uniform) case costs a relaxed load.
fn unify_viewport_slots() {
    if VIEWPORT_SLOTS_UNIFORM.load(Ordering::Relaxed)
        || !config_flags().has(Flag::UniformViewportSlots)
    {
        return;
    }
    let (Some(d3d), Some(detour)) = (EngineContext::get(), RS_SET_VIEWPORTS.get()) else {
        return;
    };
    d3d.with_lock(|ctx| {
        // SAFETY: `count` is the length of `slots`; `detour.call` is the original RSSetViewports, so
        // the re-bind does not re-enter the detour and re-record a full viewport.
        unsafe {
            let mut count = 1u32;
            let mut slots = [D3D11_VIEWPORT::default(); 1];
            ctx.RSGetViewports(&mut count, Some(slots.as_mut_ptr()));
            // A zero-width slot 0 means no viewport is bound at all; duplicating it would clip
            // everything to nothing.
            if slots[0].Width > 0.0 {
                detour.call(ctx.as_raw(), 2, [slots[0], slots[0]].as_ptr());
                set_viewport_slots_uniform(true);
                VIEWPORT_UNIFIED.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
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

/// A process-global slot for one installed detour: lock-free to read, and reclaimable at teardown.
///
/// A `OnceLock` cannot give its contents back, so a `OnceLock<GenericDetour<_>>` static can only leak
/// -- Rust statics are not dropped, and a detour's trampoline lives in a `VirtualAlloc` region that
/// outlives the unmapped payload, so every inject/eject cycle strands one page per detour. An
/// `AtomicPtr` keeps the read on the hot path down to a single load while still allowing
/// [`take`](Self::take) to hand ownership back on eject.
struct DetourSlot<T: Function>(AtomicPtr<GenericDetour<T>>);

impl<T: Function> DetourSlot<T> {
    const fn new() -> Self {
        Self(AtomicPtr::new(std::ptr::null_mut()))
    }

    /// The installed detour, or `None` before install and after teardown.
    fn get(&self) -> Option<&GenericDetour<T>> {
        // SAFETY: the pointer is null or a `Box` this slot owns. It is published with `Release`
        // before the detour it belongs to can be entered, and reclaimed only with every other thread
        // suspended, so a borrow taken here cannot outlive the allocation.
        unsafe { self.0.load(Ordering::Acquire).as_ref() }
    }

    /// Install `detour` into an empty slot. A second call leaves the slot alone and drops `detour`.
    fn set(&self, detour: GenericDetour<T>) {
        let raw = Box::into_raw(Box::new(detour));
        if self
            .0
            .compare_exchange(
                std::ptr::null_mut(),
                raw,
                Ordering::Release,
                Ordering::Relaxed,
            )
            .is_err()
        {
            // SAFETY: the slot was already occupied, so nothing else can have seen `raw`.
            drop(unsafe { Box::from_raw(raw) });
        }
    }

    /// Empty the slot, returning the detour so dropping it frees the trampoline.
    fn take(&self) -> Option<Box<GenericDetour<T>>> {
        let raw = self.0.swap(std::ptr::null_mut(), Ordering::AcqRel);
        // SAFETY: a non-null pointer here is the `Box` this slot owned; the swap makes the take
        // exclusive.
        (!raw.is_null()).then(|| unsafe { Box::from_raw(raw) })
    }
}

/// `ID3D11DeviceContext` vtable slots (7 base `IUnknown`/`ID3D11DeviceChild` slots + the method's
/// index), verified against `windows`'s `ID3D11DeviceContext_Vtbl`.
const RS_SET_VIEWPORTS_SLOT: usize = 44;
const RS_SET_SCISSOR_RECTS_SLOT: usize = 45;

type RsSetViewportsFn = unsafe extern "system" fn(*mut c_void, u32, *const D3D11_VIEWPORT);
type RsSetScissorRectsFn = unsafe extern "system" fn(*mut c_void, u32, *const RECT);

static RS_SET_VIEWPORTS: DetourSlot<RsSetViewportsFn> = DetourSlot::new();
static RS_SET_SCISSOR_RECTS: DetourSlot<RsSetScissorRectsFn> = DetourSlot::new();

unsafe extern "system" fn rs_set_viewports_detour(
    context: *mut c_void,
    count: u32,
    viewports: *const D3D11_VIEWPORT,
) {
    let detour = RS_SET_VIEWPORTS.get().expect("set before enable");
    if active() && count == 1 && !viewports.is_null() {
        let vp = unsafe { *viewports };
        if let Some(eye) = per_eye_reissue_eye() {
            // Inside a per-eye re-issue the eye half must survive whatever the block binds. Honour the
            // requested region but keep it pinned to this eye's half, and leave `COLLAPSE_FULL_VIEWPORT`
            // alone so the re-issue cannot redefine what "full" means for the draws that follow it.
            let half = eye_half_viewport(vp, eye);
            set_viewport_slots_uniform(true);
            unsafe { detour.call(context, 2, [half, half].as_ptr()) };
            return;
        }
        if collapse_active() {
            // Collapse: record the full viewport and bind both slots to it unsplit. The eye-split is
            // applied per-draw in `draw_indexed_detour` via `ensure_collapse_viewport`, so the
            // interleaved fullscreen lighting/post passes (which do not route to an eye) keep the full
            // width while patched geometry gets the L/R halves. Binding both slots keeps a patched
            // shader that writes `SV_ViewportArrayIndex` valid before the first split of a pass.
            //
            // Only a scene-sized viewport is recorded. The detour sees every viewport bind in the
            // frame, including the half-resolution SSAO/SSR/bloom targets, and the eye halves are
            // derived from this record -- so without the size check the eye split would follow
            // whichever post pass happened to bind last.
            if is_scene_sized(vp) {
                *COLLAPSE_FULL_VIEWPORT.lock() = Some(vp);
            }
            // Unconditional, unlike the scene record above: this one has to follow the engine onto the
            // reduced-resolution off-screen targets, which is the whole point of it.
            *CURRENT_ENGINE_VIEWPORT.lock() = Some(vp);
            VIEWPORT_DUP.fetch_add(1, Ordering::Relaxed);
            set_viewport_slots_uniform(true);
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
        set_viewport_slots_uniform(slot0 == slot1);
        unsafe { detour.call(context, 2, [slot0, slot1].as_ptr()) };
    } else {
        // A multi-slot set passes straight through, so the slots become whatever the caller asked for:
        // uniform only for the mod's own two-identical-slot binds. Anything else (including a set that
        // leaves slot 1 unbound) is taken as non-uniform, so the next patched draw outside the range
        // repairs it.
        let uniform =
            count == 2 && !viewports.is_null() && unsafe { *viewports == *viewports.add(1) };
        set_viewport_slots_uniform(uniform);
        unsafe { detour.call(context, count, viewports) };
    }
}

/// What a collapse draw wants bound to the two viewport slots.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CollapseViewport {
    /// Both slots span the whole double-wide target: the fullscreen lighting and post passes.
    Full,
    /// Slot 0 is the left eye's half and slot 1 the right: a patched shader's
    /// `SV_ViewportArrayIndex` picks its own.
    Split,
    /// Both slots are the same eye's half, so a shader that writes no viewport index -- or writes
    /// either one -- still lands in that eye. Used by the per-eye re-issue of unpatched geometry.
    Eye(usize),
}

/// In the collapsed single walk, bind the immediate-context viewport for the draw about to be
/// submitted. Derives the halves from the full viewport recorded by [`rs_set_viewports_detour`]; a
/// no-op until the scene's first viewport bind records it.
fn ensure_collapse_viewport(context: *mut c_void, target: CollapseViewport) {
    // Split the viewport of whatever target is actually bound, not the scene's. They are the same
    // everywhere except the reduced-resolution off-screen passes -- see [`CURRENT_ENGINE_VIEWPORT`].
    // From the dispatch snapshot, not live: this function both pins an eye half and puts the full
    // viewport back, and the two calls have to agree on which "full" they mean.
    let base = if config_flags().has(Flag::ViewportFollowsTarget) {
        (*CURRENT_ENGINE_VIEWPORT.lock()).or(*COLLAPSE_FULL_VIEWPORT.lock())
    } else {
        *COLLAPSE_FULL_VIEWPORT.lock()
    };
    let Some(full) = base else {
        return;
    };
    let Some(detour) = RS_SET_VIEWPORTS.get() else {
        return;
    };
    let viewports = match target {
        CollapseViewport::Split => {
            VIEWPORT_SPLIT.fetch_add(1, Ordering::Relaxed);
            [eye_half_viewport(full, 0), eye_half_viewport(full, 1)]
        }
        CollapseViewport::Eye(eye) => {
            VIEWPORT_SPLIT.fetch_add(1, Ordering::Relaxed);
            let half = eye_half_viewport(full, eye);
            [half, half]
        }
        CollapseViewport::Full => [full, full],
    };
    set_viewport_slots_uniform(viewports[0] == viewports[1]);
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

static DRAW_INDEXED: DetourSlot<DrawIndexedFn> = DetourSlot::new();
static DRAW: DetourSlot<DrawFn> = DetourSlot::new();
/// `DrawIndexedInstanced` (see [`draw_indexed_instanced_detour`]); its trampoline also serves as the
/// raw entry a promoted [`draw_indexed_detour`] draw is re-issued through.
///
/// An already-instanced draw whose patched vertex shader takes its per-instance data through a
/// vertex-buffer slot (rather than through `SV_InstanceID`, which would have deferred the shader with
/// `InstanceIdAlreadyDeclared`) has instance ids that are the game's own, so `SV_InstanceID & 1` sends
/// instance `i` to eye `i & 1` -- half the instances in each eye, and a 1-instance draw to the left eye
/// only. Promoting the instance count cannot fix it: the per-instance vertex-buffer stepping is indexed
/// by the instance id, so doubling the count reads past the instance data. Instead [`instanced_per_eye`]
/// makes the parity irrelevant, re-issuing the draw once per eye with both `cb13` eye slots and both
/// viewport slots pinned to that eye -- the bucket-(d) mechanism in
/// `docs/mod/single-pass-render-blocks.md`.
static DRAW_INDEXED_INSTANCED: DetourSlot<DrawIndexedInstancedFn> = DetourSlot::new();

/// Handle a `DrawIndexed` while the dual-eye G-buffer geometry is drawing. A **patched** shader is
/// promoted to a 2-instance `DrawIndexedInstanced` -- its `SV_InstanceID & 1` selects the eye and
/// `SV_ViewportArrayIndex` routes it to that eye's viewport half (one draw, both eyes). An
/// **unpatched** shader writes no viewport index and so would rasterise to slot 0 only; under collapse
/// it is instead re-issued once per eye with both slots pinned to that eye's half, which costs a
/// second submission but is the only way it reaches both eyes. The patched/unpatched split is counted
/// for the diagnostic log.
unsafe extern "system" fn draw_indexed_detour(
    context: *mut c_void,
    index_count: u32,
    start_index: u32,
    base_vertex: i32,
) {
    let detour = DRAW_INDEXED.get().expect("set before enable");
    if dual_eye_active() && in_gbuffer_range() && per_eye_reissue_eye().is_none() {
        let patched = BOUND_VS_PATCHED.load(Ordering::Relaxed);
        if patched {
            if collapse_active() {
                ensure_collapse_viewport(context, CollapseViewport::Split);
            }
            PATCHED_DRAWS.fetch_add(1, Ordering::Relaxed);
            if let Some(instanced) = DRAW_INDEXED_INSTANCED.get() {
                // Through the trampoline, not the detoured entry: the promotion is the mod's own
                // draw, so it must not be counted as exposure by [`draw_indexed_instanced_detour`].
                unsafe { instanced.call(context, index_count, 2, start_index, base_vertex, 0) };
                return;
            }
        } else {
            UNPATCHED_DRAWS.fetch_add(1, Ordering::Relaxed);
            if collapse_active() {
                // The shader was not rewritten, so it writes no `SV_ViewportArrayIndex` and always
                // rasterises to slot 0. Left to the split binding it would appear in the left eye
                // only; left to the full binding it would stretch across both. Re-issue it once per
                // eye with both slots pinned to that eye's half. It still reads the centred `cb0`, so
                // there is no parallax on it -- but it is present, in both eyes, at the right size.
                for eye in 0..2 {
                    ensure_collapse_viewport(context, CollapseViewport::Eye(eye));
                    unsafe { detour.call(context, index_count, start_index, base_vertex) };
                }
                ensure_collapse_viewport(context, CollapseViewport::Full);
                return;
            }
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
    let detour = DRAW.get().expect("set before enable");
    // SAFETY: forwards the caller's arguments unchanged to the trampoline.
    let submit = || unsafe { detour.call(context, vertex_count, start_vertex) };

    if collapse_active() && in_gbuffer_range() && per_eye_reissue_eye().is_none() {
        let pass = current_pass_id();
        SLOT13_BY_PASS[usize::from(pass)].fetch_add(1, Ordering::Relaxed);
        // A geometry pass on this entry point is not the fullscreen triangle the `Full` reset below
        // exists for, so give it the same per-eye treatment the indexed path gives its draws.
        if is_geometry_slot13_pass(pass)
            && config_flags().has(Flag::Slot13PerEye)
            && instanced_per_eye(&submit)
        {
            return;
        }
        ensure_collapse_viewport(context, CollapseViewport::Full);
    }
    submit();
}

/// Render passes whose non-indexed draws are **geometry**, not a viewport-covering triangle.
///
/// `Draw` (slot 13) is predominantly the fullscreen-pass entry point, which is why the default is to
/// reset the viewport to full for it. But a number of render blocks submit ordinary world geometry
/// non-indexed, and pinning `Full` for those rasterises them across the whole double-wide target,
/// stretched 2x horizontally about its centre. A 2x horizontal stretch is also a 2x horizontal
/// *motion* gain, which is what makes decals appear to slide across the world at twice the camera's
/// rate.
///
/// # How this list was derived
///
/// Statically, from the release binary rather than from a capture. The engine funnels every
/// non-indexed submission through one wrapper (`Graphics::Draw`), so its call sites are the complete
/// set of blocks that can reach slot 13 -- 60-odd of them. Each was classified by reading its `Draw`:
/// a block that binds a vertex stream and submits a world transform is geometry, one that submits a
/// screen-covering primitive is not. Their passes come from the engine's pass-creation sites
/// (`CRenderEngine::CreateRenderPass`, whose pass id is a literal at every call), which name the
/// owning system for each id, plus that system's enqueue site.
///
/// The two groups below are the result. Every fullscreen slot-13 block turned out to sit on a pass of
/// its own that no geometry block is enqueued onto -- including three that fall *inside* the GBuffer
/// index range and so are live under the collapse ([`RenderPassId::RP_DOWNSAMPLE_DEPTH`],
/// [`RenderPassId::RP_UNDERWATER_FOG_GRADIENT`], and the fog-volume trio) -- so no pass carries both
/// kinds and the pass id is a sound discriminator. It stays an allowlist, not a heuristic: the cost of
/// misclassifying a fullscreen pass as geometry is a visibly wrong frame, while the cost of missing a
/// geometry pass is only the status quo.
///
/// Deliberately absent: [`RenderPassId::RP_MODELS_REFLECTION`], which is model geometry but renders
/// the reflection camera's view into the reflection target, not into the double-wide scene target --
/// splitting it per eye would be wrong. The blocks whose passes lie outside the collapsed range
/// (UI, the reflection prepasses, `POST_RP_FULLSCREEN_VIDEO`) need no entry at all.
const GEOMETRY_SLOT13_PASSES: &[u8] = &[
    // Passes whose entire draw list is a block that *always* submits non-indexed.
    // `CRoadMeshManager` owns both road passes; a road is enqueued onto the stencil one as well when
    // its visual type has shader type 0.
    RenderPassId::RP_ROAD_STENCIL as u8,
    RenderPassId::RP_ROAD_LAYERS as u8,
    // `CCreatureManager`'s pass: `CCreatureRenderBlock::Draw` has no indexed path at all.
    RenderPassId::RP_CREATURES as u8,
    // `CDecalManager`'s two passes, drawing the four `CRenderBlockDecal*` types.
    RenderPassId::RP_DECALS as u8,
    RenderPassId::RP_WINDOW_DECALS as u8,
    // `CSkidmarkManager`'s pass, drawing `CRenderBlockSkidmarks`.
    RenderPassId::RP_SKIDMARKS as u8,
    // The model-family passes. General / GeneralJC3 / GeneralMaskedJC3 / BuildingJC3 / Prop /
    // MaterialTune / Open / FXMeshFire / MeshParticle all end their `Draw` and `DrawZ` with the same
    // branch -- `DrawIndexed` when the block has an index buffer, plain `Draw` when it does not -- so
    // any index-buffer-less model in the shipped data lands on slot 13 in whichever of these it was
    // enqueued onto. The engine owns the two Z passes; `CModelInstanceManager` owns the rest, and
    // enqueues nothing but model instances onto them.
    RenderPassId::RP_Z_PASS as u8,
    RenderPassId::RP_Z_AND_VELOCITY_PASS as u8,
    RenderPassId::RP_MODELS_DYNAMIC as u8,
    RenderPassId::RP_MODELS_DYNAMIC_MASK_DAMAGE_POST_EFFECT as u8,
    RenderPassId::RP_MODELS_STATIC as u8,
    RenderPassId::RP_MODELS_TRANSPARENT as u8,
    RenderPassId::RP_MODELS_GLINT as u8,
    RenderPassId::RP_MODEL_HALO_POST as u8,
    RenderPassId::RP_MODELS_REFRACT as u8,
    RenderPassId::RP_Z_FINAL_TRANSPARENT as u8,
    RenderPassId::RP_GHOST_EFFECT as u8,
    RenderPassId::RP_OUTLINE_MASK as u8,
    RenderPassId::RP_FINAL_TRANSPARENT as u8,
];

fn is_geometry_slot13_pass(pass: u8) -> bool {
    GEOMETRY_SLOT13_PASSES.contains(&pass)
}

// ---- GPU-indirect draws --------------------------------------------------------------------------

/// `ID3D11DeviceContext` vtable slots for the two GPU-indirect draw entry points (field 33 → slot 39,
/// field 34 → slot 40, verified against `windows`'s `ID3D11DeviceContext_Vtbl`). The engine reaches
/// them through `Graphics::DrawInstanced` (slot 39) and `Graphics::DrawIndexedInstancedIndirectNoMutex`
/// (slot 40); the terrain-patch block's near tessellating passes and the foliage block's dominant path
/// are the volume users.
const DRAW_INDEXED_INSTANCED_INDIRECT_SLOT: usize = 39;
const DRAW_INSTANCED_INDIRECT_SLOT: usize = 40;

/// Both indirect entry points take `(ID3D11Buffer* pBufferForArgs, UINT AlignedByteOffsetForArgs)`.
type DrawIndirectFn = unsafe extern "system" fn(*mut c_void, *mut c_void, u32);

static DRAW_INDEXED_INSTANCED_INDIRECT: DetourSlot<DrawIndirectFn> = DetourSlot::new();
static DRAW_INSTANCED_INDIRECT: DetourSlot<DrawIndirectFn> = DetourSlot::new();

unsafe extern "system" fn draw_indexed_instanced_indirect_detour(
    context: *mut c_void,
    buffer_for_args: *mut c_void,
    aligned_byte_offset: u32,
) {
    let detour = DRAW_INDEXED_INSTANCED_INDIRECT
        .get()
        .expect("set before enable");
    // SAFETY: forwards the caller's arguments unchanged to the trampoline.
    let submit = || unsafe { detour.call(context, buffer_for_args, aligned_byte_offset) };
    if !indirect_per_eye(context, &submit) {
        submit();
    }
}

unsafe extern "system" fn draw_instanced_indirect_detour(
    context: *mut c_void,
    buffer_for_args: *mut c_void,
    aligned_byte_offset: u32,
) {
    let detour = DRAW_INSTANCED_INDIRECT.get().expect("set before enable");
    // SAFETY: forwards the caller's arguments unchanged to the trampoline.
    let submit = || unsafe { detour.call(context, buffer_for_args, aligned_byte_offset) };
    if !indirect_per_eye(context, &submit) {
        submit();
    }
}

/// Re-issue a GPU-indirect draw once per eye with both viewport slots pinned to that eye's half,
/// reporting whether it did; `false` means the caller must submit the draw itself, once, unchanged.
///
/// An indirect draw carries its vertex/instance counts in a GPU buffer, so it can be neither
/// instance-doubled (the promotion [`draw_indexed_detour`] applies to a patched shader) nor rewritten
/// -- the arguments are submitted verbatim, twice. What the mod controls is the viewport, and that is
/// the whole defect: nothing detoured these entry points, so the draw inherited whatever the previous
/// draw left bound, which under the collapse is dominated by [`CollapseViewport::Full`]. Rasterising
/// `cb0`'s per-eye off-axis projection into the full double-wide viewport stretches the geometry 2x
/// horizontally about the target centre, and a 2x horizontal stretch is a 2x horizontal *motion* gain
/// -- the near terrain patches and foliage sliding across the screen as the camera turns. Pinning each
/// submission to its eye's half is the same deal the unpatched `DrawIndexed` path already takes:
/// present and correctly sized in both eyes, with no parallax, since `cb0` stays eye 0's.
///
/// Declines outside the collapse and the G-buffer range (elsewhere there is no eye split to inherit),
/// inside a block-level per-eye re-issue (which has already pinned the viewport for this eye
/// deliberately -- the [`PER_EYE_REISSUE`] marker), and when the bound vertex shader is patched (it
/// writes `SV_ViewportArrayIndex` itself and would have to be routed, not pinned; that includes the
/// tessellated terrain families once `single_pass_terrain` transforms them). `cb13` needs no pinning
/// here for the same reason: no pristine game shader declares it, so an unpatched draw cannot read it.
///
/// The re-issue does not raise [`PER_EYE_REISSUE`] itself -- it drives no engine code, only the
/// trampolines, so nothing it calls can re-enter this module's detours.
fn indirect_per_eye(context: *mut c_void, submit: &dyn Fn()) -> bool {
    if !collapse_active() || !in_gbuffer_range() || per_eye_reissue_eye().is_some() {
        return false;
    }
    if !config_flags().has(Flag::IndirectPerEye) || BOUND_VS_PATCHED.load(Ordering::Relaxed) {
        INDIRECT_FORWARDED.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    // Without a recorded full viewport `ensure_collapse_viewport` is a no-op, so the loop would submit
    // the draw twice into one region -- doubled geometry rather than a split.
    if COLLAPSE_FULL_VIEWPORT.lock().is_none() {
        INDIRECT_FORWARDED.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    for eye in 0..2 {
        ensure_collapse_viewport(context, CollapseViewport::Eye(eye));
        submit();
    }
    // Unconditional, as on every other path that pins a viewport: a leaked eye-half would clip the
    // rest of the frame -- including the fullscreen passes -- to half the target.
    ensure_collapse_viewport(context, CollapseViewport::Full);
    INDIRECT_REISSUED.fetch_add(1, Ordering::Relaxed);
    true
}

/// GPU-indirect draws [`indirect_per_eye`] re-issued, and those it saw in a collapsed G-buffer range
/// but forwarded once (the flag off, or a patched shader bound). Reset per pass range by
/// [`log_draw_split`], which reports them.
static INDIRECT_REISSUED: AtomicUsize = AtomicUsize::new(0);
static INDIRECT_FORWARDED: AtomicUsize = AtomicUsize::new(0);

// ---- Already-instanced draws ---------------------------------------------------------------------

/// Detour on `DrawIndexedInstanced`, handling the already-instanced case (see
/// [`DRAW_INDEXED_INSTANCED`]) with a per-eye re-issue and measuring how much of the frame it covers.
///
/// A call is in that case when a patched vertex shader is bound (so the shader reads `SV_InstanceID` as
/// an eye parity it did not ask for), the render thread is inside the G-buffer range, and the collapse
/// is on -- minus the mod's own draws. Promoted `DrawIndexed`es come through the trampoline and never
/// reach here; draws a per-eye re-issue re-drives are excluded by [`per_eye_reissue_eye`], since those
/// already land in one eye deliberately. Everything else is forwarded verbatim, once.
unsafe extern "system" fn draw_indexed_instanced_detour(
    context: *mut c_void,
    index_count_per_instance: u32,
    instance_count: u32,
    start_index: u32,
    base_vertex: i32,
    start_instance: u32,
) {
    let detour = DRAW_INDEXED_INSTANCED.get().expect("set before enable");
    let submit = || {
        // SAFETY: forwards the caller's arguments unchanged to the trampoline.
        unsafe {
            detour.call(
                context,
                index_count_per_instance,
                instance_count,
                start_index,
                base_vertex,
                start_instance,
            );
        }
    };
    if active() && per_eye_reissue_eye().is_none() {
        INSTANCED_TOTAL.fetch_add(1, Ordering::Relaxed);
        let patched = BOUND_VS_PATCHED.load(Ordering::Relaxed);
        let in_range = in_gbuffer_range();
        if patched {
            let bucket = if in_range {
                &INSTANCED_RANGE_PATCHED
            } else {
                &INSTANCED_RANGE_OUT_PATCHED
            };
            bucket.fetch_add(1, Ordering::Relaxed);
        }
        if patched && in_range && collapse_active() {
            let handled = config_flags().has(Flag::InstancedPerEye) && instanced_per_eye(&submit);
            record_instanced_case(instance_count, handled);
            if handled {
                return;
            }
        } else {
            record_instanced_bystander(patched, in_range, instance_count);
            if patched && !in_range && collapse_active() {
                // The shader writes `SV_ViewportArrayIndex` from instance parity whatever pass it is
                // in, but this one binds no eye-half pair -- so the odd instances need slot 1 to hold
                // the same region as slot 0 to appear at all.
                unify_viewport_slots();
            }
        }
    }
    submit();
}

/// Re-issue an already-instanced draw once per eye, so the game's own instance ids stop being read as
/// an eye parity. Returns `false` when the re-issue cannot run (no full viewport recorded yet, no live
/// context, or no `cb13` contents to pin and restore), in which case the caller submits once and the
/// draw is counted as exposure.
///
/// For each eye, **both** `cb13` eye slots are filled with that eye's transforms and **both** viewport
/// slots with that eye's half of the double-wide target, so whichever parity `SV_InstanceID & 1`
/// produces -- and therefore whichever `SV_ViewportArrayIndex` the shader writes -- resolves to the
/// same eye. The instance count is left alone: per-instance vertex-buffer stepping is indexed by the
/// instance id, so doubling it would read past the batch's per-instance data.
///
/// `cb13` is shared by every patched shader in the pass, so its restore is unconditional: leaving it
/// pinned would collapse the rest of the frame's geometry to one eye. The viewport is put back exactly
/// as it was found rather than to the recorded full width, so a following draw that inherits the
/// viewport (an indirect draw, which nothing detours) sees no trace of the pin.
fn instanced_per_eye(submit: &dyn Fn()) -> bool {
    let Some(full) = *COLLAPSE_FULL_VIEWPORT.lock() else {
        return false;
    };
    let Some(d3d) = EngineContext::get() else {
        return false;
    };
    let Some(rows) = CB13.lock().live_rows() else {
        return false;
    };
    let saved = capture_viewport_slots(d3d);

    let mut submitted = 0;
    for eye in 0..2 {
        // The same marker the baked-cb re-issues raise: it keeps this module's own detours -- and any
        // block-level re-issue reached from here -- from compounding the split this loop sets up.
        let _reissue = PerEyeReissue::enter(eye);
        if !write_cb13_rows(d3d, &pin_rows_to_eye(&rows, eye)) {
            break;
        }
        bind_both_viewport_slots(d3d, eye_half_viewport(full, eye));
        submit();
        submitted += 1;
    }

    write_cb13_rows(d3d, &rows);
    restore_viewport_slots(d3d, saved);
    // A map failure before the first submission leaves the draw undrawn, so the caller must still
    // submit it; one after leaves it partially drawn, where a further submission would only duplicate
    // geometry in an eye that already has it.
    submitted > 0
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

static CREATE_VERTEX_SHADER: DetourSlot<CreateVertexShaderFn> = DetourSlot::new();
static VS_SET_SHADER: DetourSlot<VsSetShaderFn> = DetourSlot::new();

/// What [`create_vertex_shader_detour`] made of a blob that did not arrive pre-substituted from the
/// `CreateVertexProgram` hook.
enum Reacquired {
    /// The hook did see this blob and decided its transform by name ([`remember_vs_transform`]); that
    /// decision was re-applied here. `None` when the decision was to leave the shader pristine, or
    /// when the rewrites it allows all bowed out.
    Decided(Option<Vec<u8>>),
    /// The hook never saw this blob, so there is no name to decide with. Fall back to the `cb0` remap,
    /// the only transform decidable from the bytecode alone.
    Rederived(Result<Vec<u8>, DxbcError>),
}

/// Substitute and record the `ID3D11VertexShader` for a stereo-patched blob, covering both the fresh
/// and the re-created shader-creation paths.
///
/// The `CreateVertexProgram` hook substitutes the blob for a *fresh* shader create and sets
/// [`PATCH_PENDING`] right before the engine calls `CreateVertexShader`, so the shader created under
/// that flag is the patched one and goes straight into [`PATCHED_VS`]. But a bundle reload re-creates
/// an already-loaded shader through `ResourceCacheReCreateResource`, which calls `CreateVertexShader`
/// directly *without* re-running `CreateVertexProgram` -- so a shader first loaded before single-pass
/// (e.g. a character shader from level start, whose resource still holds the original bytecode) would
/// arrive here unsubstituted and render mono/skewed.
///
/// Catch that path with the decision the hook already made for that blob ([`cached_vs_transform`]),
/// which is the only way to get it right: the transform is chosen by shader *name* and the D3D layer
/// has no name, so re-deriving one here can only ever produce the `cb0` remap -- the wrong transform
/// for the reprojection families, and one transform too many for the families a render-block intercept
/// owns and the hook deliberately left pristine. Only a blob the hook never saw is re-derived, and
/// then the remap is the best that can be done: an unpatched-but-patchable blob is substituted in
/// place for the create call, and an already-patched blob (`Cb13AlreadyDeclared` -- a reload of a
/// shader whose resource already holds the patched blob) is recorded as-is. `PATCH_PENDING`
/// short-circuits all of it for the fresh path, whose blob `CreateVertexProgram` already substituted.
unsafe extern "system" fn create_vertex_shader_detour(
    device: *mut c_void,
    bytecode: *const c_void,
    length: usize,
    linkage: *mut c_void,
    out: *mut *mut c_void,
) -> i32 {
    let detour = CREATE_VERTEX_SHADER.get().expect("set before enable");
    let pending = PATCH_PENDING.with(Cell::take);
    // Taken unconditionally, so a name left behind by a create that did not reach the record below
    // cannot be attributed to a later, unrelated shader.
    let pending_name = PATCH_PENDING_NAME.with(Cell::take);
    let reacquired = (!pending && active() && !bytecode.is_null() && length >= 4).then(|| {
        let code = unsafe { std::slice::from_raw_parts(bytecode.cast::<u8>(), length) };
        match cached_vs_transform(code) {
            // No remap output to hand on (this path did not run one) and no name (the whole reason the
            // decision had to be made elsewhere), so the terrain log line here is unnamed.
            Some(transform) => Reacquired::Decided(apply_vs_transform(transform, code, None, None)),
            None => Reacquired::Rederived(dxbc_stereo::patch_vertex_shader(code)),
        }
    });
    let (record, blob, len) = match &reacquired {
        Some(Reacquired::Decided(Some(transformed))) => {
            CVS_DECIDED_TRANSFORMED.fetch_add(1, Ordering::Relaxed);
            (
                true,
                transformed.as_ptr().cast::<c_void>(),
                transformed.len(),
            )
        }
        Some(Reacquired::Decided(None)) => {
            CVS_DECIDED_PRISTINE.fetch_add(1, Ordering::Relaxed);
            (false, bytecode, length)
        }
        Some(Reacquired::Rederived(Ok(patched))) => {
            CVS_REACQ_PATCHED.fetch_add(1, Ordering::Relaxed);
            (true, patched.as_ptr().cast::<c_void>(), patched.len())
        }
        Some(Reacquired::Rederived(Err(DxbcError::Cb13AlreadyDeclared))) => {
            CVS_REACQ_CB13.fetch_add(1, Ordering::Relaxed);
            (true, bytecode, length)
        }
        Some(Reacquired::Rederived(Err(DxbcError::NoPerEyeReferences))) => {
            CVS_REACQ_NOREFS.fetch_add(1, Ordering::Relaxed);
            (false, bytecode, length)
        }
        Some(Reacquired::Rederived(Err(_))) => {
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
        && PATCHED_VS.lock().insert(shader as usize)
    {
        // SAFETY: on success `*out` is the newly-created, live COM object. Only the thread that won the
        // `insert` takes the reference, so records and releases stay balanced.
        unsafe { com_add_ref(shader) };
        if let Some(name) = pending_name {
            PATCHED_VS_NAMES.lock().insert(shader as usize, name);
        }
    }
    hr
}

/// Cache whether the vertex shader now bound is a patched one, so [`draw_indexed_detour`] can gate
/// without a per-draw set lookup, and its pointer, so the already-instanced exposure measurement can
/// attribute a draw to the shader that issued it.
unsafe extern "system" fn vs_set_shader_detour(
    context: *mut c_void,
    shader: *mut c_void,
    instances: *const *mut c_void,
    num_instances: u32,
) {
    let patched = !shader.is_null() && PATCHED_VS.lock().contains(&(shader as usize));
    BOUND_VS_PATCHED.store(patched, Ordering::Relaxed);
    BOUND_VS.store(shader as usize, Ordering::Relaxed);
    let detour = VS_SET_SHADER.get().expect("set before enable");
    unsafe { detour.call(context, shader, instances, num_instances) };
}

/// Reset the patched-shader set, releasing this module's reference to each recorded shader (on a
/// shader reload, where the game drops the old shaders and their addresses become reusable). Also
/// zeroes the `CreateVertexShader`-path tallies so [`substitution_stats`] reflects one clean pass over
/// the reloaded shader set.
pub fn reset_patched_vs() {
    for shader in std::mem::take(&mut *PATCHED_VS.lock()) {
        // SAFETY: every entry was `com_add_ref`'d exactly once when it was recorded.
        unsafe { com_release(shader as *mut c_void) };
    }
    // Both are keyed by shader pointer, and those addresses become reusable the moment the references
    // above are dropped, so an entry that survived the reset would be attributed to a different shader.
    PATCHED_VS_NAMES.lock().clear();
    // The transform decisions go with them: this runs mid-bounce, so the pass that follows re-decides
    // every shader under the current config and repopulates the map. Keeping the old entries would let
    // a decision made under config that has since changed outlive the shader it was made for, and
    // would accumulate the previous bundle's blobs across every reload.
    VS_TRANSFORM_CACHE.lock().clear();
    reset_instanced_exposure();
    CVS_PENDING.store(0, Ordering::Relaxed);
    CVS_DECIDED_TRANSFORMED.store(0, Ordering::Relaxed);
    CVS_DECIDED_PRISTINE.store(0, Ordering::Relaxed);
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

/// Warn if any shader was created from bytecode that already carried the mod's rewrite (see
/// [`CVS_REACQ_CB13`]). The eject bounce cannot restore those: it re-creates from the resource's own
/// bytecode, which in this case is the patched blob, and the rewriter has no inverse. Called from the
/// eject restore so the condition is on the record instead of only in a live counter.
pub fn warn_if_shaders_hold_patched_bytecode() {
    let count = CVS_REACQ_CB13.load(Ordering::Relaxed);
    if count > 0 {
        tracing::warn!(
            "single-pass: {count} shader(s) were created from bytecode that already carried the \
             stereo rewrite, so some resource holds a patched blob; the eject bounce re-creates \
             those still patched"
        );
    }
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
    pub cvs_decided_transformed: usize,
    pub cvs_decided_pristine: usize,
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
        cvs_decided_transformed: CVS_DECIDED_TRANSFORMED.load(Ordering::Relaxed),
        cvs_decided_pristine: CVS_DECIDED_PRISTINE.load(Ordering::Relaxed),
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

/// Log and reset the patched/unpatched draw counts of one G-buffer range -- called at each range's
/// exit, tagged with the `[first, last)` pass-index window the range covered so the frame's several
/// ranges can be told apart in the log.
///
/// `torn` counts the ranges whose guard found the flag already lowered: a non-zero value means the
/// range was closed from outside while it was still running, and every draw after that point was
/// mis-classified as out-of-range.
pub fn log_draw_split(first: u32, last: u32) {
    let patched = PATCHED_DRAWS.swap(0, Ordering::Relaxed);
    let unpatched = UNPATCHED_DRAWS.swap(0, Ordering::Relaxed);
    let split = VIEWPORT_SPLIT.swap(0, Ordering::Relaxed);
    let dup = VIEWPORT_DUP.swap(0, Ordering::Relaxed);
    let in_patched = INSTANCED_RANGE_PATCHED.swap(0, Ordering::Relaxed);
    let out_patched = INSTANCED_RANGE_OUT_PATCHED.swap(0, Ordering::Relaxed);
    let indirect_reissued = INDIRECT_REISSUED.swap(0, Ordering::Relaxed);
    let indirect_forwarded = INDIRECT_FORWARDED.swap(0, Ordering::Relaxed);
    let slot13 = drain_slot13_census();
    let torn = RANGE_TORN.swap(0, Ordering::Relaxed);
    let torn_total = RANGE_TORN_TOTAL.fetch_add(torn, Ordering::Relaxed) + torn;
    if torn > 0 {
        tracing::warn!(
            target: "single_pass",
            "pass range [{first:#x}, {last:#x}) was closed from outside while it ran: the draws after \
             that point were treated as out-of-range ({torn_total} so far this session)"
        );
    }
    if !diagnostic_frame() {
        return;
    }
    let s = substitution_stats();
    // Named per pass, since the whole point is to say *which* passes submit geometry this way.
    let slot13 = slot13
        .iter()
        .map(|(pass, count)| {
            let kind = if is_geometry_slot13_pass(*pass) {
                "geom"
            } else {
                "full"
            };
            format!("{pass:#04x}:{count}:{kind}")
        })
        .collect::<Vec<_>>()
        .join(" ");
    tracing::info!(
        target: "single_pass",
        "pass range [{first:#x}, {last:#x}): {patched} patched, {unpatched} unpatched draws | \
         instanced while it ran: {in_patched} patched in-range, {out_patched} patched out-of-range | \
         indirect: {indirect_reissued} re-issued per eye, {indirect_forwarded} forwarded | \
         torn {torn} ({torn_total} this session) | viewports: {split} split, {dup} identical-dup | \
         recorded VS={} | CreateVertexShader: pending={} reacq[patched={} cb13={} no-refs={} err={}] | \
         census[patched={} no-refs={} deferred={} errored={}] | slot-13 by pass: [{slot13}]",
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

    // Ordering here has to satisfy two constraints that pull against each other.
    //
    // The slots must stay populated for as long as the functions are still patched: a detour that
    // fires with its slot already emptied finds no trampoline and aborts (it runs in a `nounwind`
    // context, so the panic is fatal rather than recoverable).
    //
    // And the trampolines must be freed *outside* the thread suspender: dropping a detour frees its
    // trampoline, a heap free takes the process heap lock, and if a suspended thread holds that lock
    // the free waits on a thread only we can resume -- an unrecoverable wedge, seen as a hang with a
    // thread spinning in the game's `PlatformAllocHook`.
    //
    // So: disable under suspension with the slots intact, resume, and only then reclaim and drop.
    // Between the two, the functions are unpatched, so nothing can enter a detour at all.
    // Fixed-size and not a `Vec`: growing it would allocate, and allocation is exactly what must
    // not happen while other threads are suspended below. `disable_detour!` writes through a
    // checked accessor, so a slot count that falls behind the number of call sites drops the
    // overflowing name from the log instead of indexing out of bounds in this `nounwind` context.
    let mut failed: [Option<&'static str>; 10] = [None; 10];
    let mut failures = 0usize;

    let _ = ThreadSuspender::for_block(|| {
        // A detour left enabled here is a relay still pointing into the about-to-be-freed payload
        // image, so a swallowed failure would be an undiagnosable crash -- record it. Recorded
        // rather than logged because formatting a `tracing` event allocates, which is the very
        // thing that must not happen under suspension.
        macro_rules! disable_detour {
            ($slot:expr, $name:literal) => {
                if let Some(detour) = $slot.get() {
                    // SAFETY: patching the function back runs with all other threads suspended.
                    let bad = match unsafe { detour.disable() } {
                        Err(_) => true,
                        Ok(()) => detour.is_enabled(),
                    };
                    if bad {
                        // `failures` still increments past the end of the array so the count
                        // stays truthful even when a name gets dropped for lack of a slot.
                        if let Some(slot) = failed.get_mut(failures) {
                            *slot = Some($name);
                        }
                        failures += 1;
                    }
                }
            };
        }
        disable_detour!(RS_SET_VIEWPORTS, "RSSetViewports");
        disable_detour!(RS_SET_SCISSOR_RECTS, "RSSetScissorRects");
        disable_detour!(DRAW_INDEXED, "DrawIndexed");
        disable_detour!(DRAW, "Draw");
        disable_detour!(DRAW_INDEXED_INSTANCED, "DrawIndexedInstanced");
        disable_detour!(
            DRAW_INDEXED_INSTANCED_INDIRECT,
            "DrawIndexedInstancedIndirect"
        );
        disable_detour!(DRAW_INSTANCED_INDIRECT, "DrawInstancedIndirect");
        disable_detour!(VS_SET_SHADER, "VSSetShader");
        disable_detour!(CREATE_VERTEX_SHADER, "CreateVertexShader");
        disable_detour!(SET_VERTEX_PROGRAM_CONSTANTS, "SetVertexProgramConstants");
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });

    for name in failed.iter().flatten() {
        tracing::error!(
            "single-pass: {name} did not disable (still dangling into the freed payload image)"
        );
    }

    // Threads are running again and the functions are unpatched, so these drops are unreachable by
    // any detour and their frees cannot deadlock against a suspended lock holder.
    drop(RS_SET_VIEWPORTS.take());
    drop(RS_SET_SCISSOR_RECTS.take());
    drop(DRAW_INDEXED.take());
    drop(DRAW.take());
    drop(DRAW_INDEXED_INSTANCED.take());
    drop(DRAW_INDEXED_INSTANCED_INDIRECT.take());
    drop(DRAW_INSTANCED_INDIRECT.take());
    drop(VS_SET_SHADER.take());
    drop(CREATE_VERTEX_SHADER.take());
    drop(SET_VERTEX_PROGRAM_CONSTANTS.take());
    release_cb13();
    for shader in std::mem::take(&mut *PATCHED_VS.lock()) {
        // SAFETY: every entry was `com_add_ref`'d exactly once when it was recorded.
        unsafe { com_release(shader as *mut c_void) };
    }
    PATCHED_VS_NAMES.lock().clear();
    // The transform decisions go with them. They are normally cleared by the eject's shader bounce,
    // but a session that patched nothing never bounces, and a `static` outlives the payload -- so a
    // re-inject with different flags could otherwise consult the previous session's decisions for
    // any blob the engine still has cached.
    VS_TRANSFORM_CACHE.lock().clear();
    reset_instanced_exposure();
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
        let draw_indexed_instanced_target: DrawIndexedInstancedFn =
            std::mem::transmute(*vtable.add(DRAW_INDEXED_INSTANCED_SLOT));
        let draw_indexed_instanced_indirect_target: DrawIndirectFn =
            std::mem::transmute(*vtable.add(DRAW_INDEXED_INSTANCED_INDIRECT_SLOT));
        let draw_instanced_indirect_target: DrawIndirectFn =
            std::mem::transmute(*vtable.add(DRAW_INSTANCED_INDIRECT_SLOT));
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
            Ok(draw_indexed_instanced_detour_handle),
            Ok(draw_indexed_instanced_indirect_detour_handle),
            Ok(draw_instanced_indirect_detour_handle),
            Ok(vs_set_shader_detour_handle),
            Ok(create_vertex_shader_detour_handle),
            Ok(set_vs_consts_detour_handle),
        ) = (
            GenericDetour::new(viewports_target, rs_set_viewports_detour),
            GenericDetour::new(scissors_target, rs_set_scissor_rects_detour),
            GenericDetour::new(draw_indexed_target, draw_indexed_detour),
            GenericDetour::new(draw_target, draw_detour),
            GenericDetour::new(draw_indexed_instanced_target, draw_indexed_instanced_detour),
            GenericDetour::new(
                draw_indexed_instanced_indirect_target,
                draw_indexed_instanced_indirect_detour,
            ),
            GenericDetour::new(
                draw_instanced_indirect_target,
                draw_instanced_indirect_detour,
            ),
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
        RS_SET_VIEWPORTS.set(viewports_detour);
        RS_SET_SCISSOR_RECTS.set(scissors_detour);
        DRAW_INDEXED.set(draw_indexed_detour_handle);
        DRAW.set(draw_detour_handle);
        DRAW_INDEXED_INSTANCED.set(draw_indexed_instanced_detour_handle);
        DRAW_INDEXED_INSTANCED_INDIRECT.set(draw_indexed_instanced_indirect_detour_handle);
        DRAW_INSTANCED_INDIRECT.set(draw_instanced_indirect_detour_handle);
        VS_SET_SHADER.set(vs_set_shader_detour_handle);
        CREATE_VERTEX_SHADER.set(create_vertex_shader_detour_handle);
        SET_VERTEX_PROGRAM_CONSTANTS.set(set_vs_consts_detour_handle);
        let _ = ThreadSuspender::for_block(|| {
            RS_SET_VIEWPORTS.get().expect("just set").enable().ok();
            RS_SET_SCISSOR_RECTS.get().expect("just set").enable().ok();
            DRAW_INDEXED.get().expect("just set").enable().ok();
            DRAW.get().expect("just set").enable().ok();
            DRAW_INDEXED_INSTANCED
                .get()
                .expect("just set")
                .enable()
                .ok();
            DRAW_INDEXED_INSTANCED_INDIRECT
                .get()
                .expect("just set")
                .enable()
                .ok();
            DRAW_INSTANCED_INDIRECT
                .get()
                .expect("just set")
                .enable()
                .ok();
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

static IN_GBUFFER_RANGE: AtomicBool = AtomicBool::new(false);
/// Real frames since injection, advanced by [`begin_frame`]; the diagnostics' cadence and grouping.
static FRAME_ORDINAL: AtomicUsize = AtomicUsize::new(0);
/// Ranges closed from outside their guard since the last [`log_draw_split`], and the session total --
/// see [`GBufferRange::drop`]. The tear is intermittent, so the total is never reset: a diagnostic
/// frame that reports zero of its own still shows whether it has ever happened.
static RANGE_TORN: AtomicUsize = AtomicUsize::new(0);
static RANGE_TORN_TOTAL: AtomicUsize = AtomicUsize::new(0);

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

    /// The engine name of the shader [`PATCH_PENDING`] refers to, carried alongside it so
    /// [`create_vertex_shader_detour`] can record the name against the `ID3D11VertexShader` it gets
    /// back. The D3D layer never sees the name, and the shader pointer is the only identity the
    /// draw-time paths have, so this is the one point where the two can be joined.
    static PATCH_PENDING_NAME: Cell<Option<String>> = const { Cell::new(None) };
}

/// Set (or clear) this thread's [`PATCH_PENDING`] flag and the pending shader's engine name. Called
/// by the `CreateVertexProgram` hook around the engine's shader creation.
pub fn set_patch_pending(pending: bool, name: Option<&str>) {
    PATCH_PENDING.with(|flag| flag.set(pending));
    PATCH_PENDING_NAME.with(|slot| slot.set(pending.then(|| name.map(str::to_owned)).flatten()));
}

/// The engine name of each recorded patched vertex shader, where one was available (the
/// `CreateVertexProgram` path carries it; the re-acquire path does not). Read only by the diagnostic
/// readouts, never on a draw path.
static PATCHED_VS_NAMES: Mutex<BTreeMap<usize, String>> = Mutex::new(BTreeMap::new());
/// The `ID3D11VertexShader`s created from patched blobs, keyed by their raw pointer.
///
/// The set *owns* a reference to each shader: [`com_add_ref`] on record, [`com_release`] on
/// [`reset_patched_vs`]. Without that reference a shader the game releases could have its address
/// recycled by an unpatched shader, which would then match on `VSSetShader` and be instance-doubled --
/// the recycled draw appears in one eye only. An ordered set rather than a linear scan because the
/// lookup is on the hottest path in the codebase: every `VSSetShader` of every frame, in a feature
/// whose whole purpose is cutting draw-submission cost. (The raw pointer is stored rather than an
/// owned `IUnknown` only because `IUnknown` is not `Send`.)
static PATCHED_VS: Mutex<BTreeSet<usize>> = Mutex::new(BTreeSet::new());

/// Take a reference of our own on a COM object, so the pointer we record cannot be freed (and its
/// address recycled) while we still consider it live. Paired with [`com_release`].
///
/// # Safety
///
/// `object` must be a live COM object pointer.
unsafe fn com_add_ref(object: *mut c_void) {
    // SAFETY: the caller guarantees a live object; cloning the borrowed interface calls `AddRef`, and
    // forgetting the clone is what keeps that reference outstanding.
    if let Some(unknown) = unsafe { IUnknown::from_raw_borrowed(&object) } {
        std::mem::forget(unknown.clone());
    }
}

/// Drop the reference [`com_add_ref`] took.
///
/// # Safety
///
/// `object` must be a pointer this module previously passed to [`com_add_ref`], not yet released.
unsafe fn com_release(object: *mut c_void) {
    // SAFETY: `from_raw` adopts the outstanding reference; dropping it calls `Release`.
    drop(unsafe { IUnknown::from_raw(object) });
}
/// Whether the currently-bound vertex shader is a patched one (updated on `VSSetShader`).
static BOUND_VS_PATCHED: AtomicBool = AtomicBool::new(false);
/// The currently-bound `ID3D11VertexShader` pointer (updated on `VSSetShader`), so an exposed
/// already-instanced draw can be attributed to a shader.
static BOUND_VS: AtomicUsize = AtomicUsize::new(0);
static PATCHED_DRAWS: AtomicUsize = AtomicUsize::new(0);
static UNPATCHED_DRAWS: AtomicUsize = AtomicUsize::new(0);
/// Already-instanced draws with a patched vertex shader bound, split by whether the G-buffer range
/// was up, accumulated per *range* rather than per frame ([`log_draw_split`] resets them). The
/// per-frame `INSTANCED_*` buckets carry the same events; these say *when in the frame* they landed.
static INSTANCED_RANGE_PATCHED: AtomicUsize = AtomicUsize::new(0);
static INSTANCED_RANGE_OUT_PATCHED: AtomicUsize = AtomicUsize::new(0);
static VIEWPORT_SPLIT: AtomicUsize = AtomicUsize::new(0);
static VIEWPORT_DUP: AtomicUsize = AtomicUsize::new(0);
/// How often [`unify_viewport_slots`] had to put a split slot pair back to one region: the number of
/// windows in which an out-of-range patched draw would otherwise have lost its odd-parity instances.
static VIEWPORT_UNIFIED: AtomicUsize = AtomicUsize::new(0);

/// `CreateVertexShader`-detour outcome tallies (cumulative since injection), to diagnose what the
/// shader re-create path -- which bypasses `CreateVertexProgram` -- feeds through the D3D-level
/// substitution: `pending` came pre-substituted from `CreateVertexProgram`; the two `decided_*` buckets
/// re-applied the decision the hook recorded for that blob; the four `reacq_*` buckets are what the
/// detour's own rewrite found for a blob the hook never saw.
static CVS_PENDING: AtomicUsize = AtomicUsize::new(0);
/// A blob the hook had decided a transform for, re-applied here.
static CVS_DECIDED_TRANSFORMED: AtomicUsize = AtomicUsize::new(0);
/// A blob the hook had decided to leave pristine -- overwhelmingly the families a render-block
/// intercept owns. A count here is the intercepts and the rewrite staying out of each other's way; the
/// same shaders showing up under `reacq_patched` instead would mean the decline was being undone.
static CVS_DECIDED_PRISTINE: AtomicUsize = AtomicUsize::new(0);
static CVS_REACQ_PATCHED: AtomicUsize = AtomicUsize::new(0);
/// The incoming bytecode already declared `cb13`, so the rewriter refused it as already-patched.
///
/// No pristine game vertex shader declares `cb13` -- the offline corpus run over all 455 of them
/// reports zero `Cb13AlreadyDeclared`, which is why the register was chosen -- so a non-zero count is
/// unambiguous: something is presenting the mod's *own* rewritten bytecode back to
/// `CreateVertexShader`, from a store the substitution paths do not own (both of them repoint the
/// engine's code pointer only for the duration of the create call and restore it afterwards).
///
/// That matters for eject. The restore bounce re-creates every shader from whatever bytecode its
/// resource holds, with the substitution inert; a resource holding patched bytecode therefore
/// re-creates a patched shader, and there is no inverse rewrite to undo it. Which store that is has
/// not been identified -- it needs a live session with a non-zero count and a breakpoint on the
/// caller. [`warn_if_shaders_hold_patched_bytecode`] makes it visible at eject rather than silent.
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

/// The viewport the **engine** last bound during a collapsed camera scene, whatever its size, recorded
/// by [`rs_set_viewports_detour`].
///
/// [`COLLAPSE_FULL_VIEWPORT`] deliberately records only *scene-sized* binds, because the eye halves
/// were derived from it and following a half-resolution post target would have mis-split the scene.
/// That is right for the scene notion and wrong for the split: several passes redirect their draws to
/// a reduced-resolution off-screen target -- the shared quarter-resolution buffer the low-resolution
/// clouds, the low-resolution particles, and the volumetric spot-light cones all render into, and the
/// downsampled depth buffer -- and a draw into a `W x H/2` target handed a `2W x H` viewport is
/// magnified 2x about the target's origin and cropped, which is a 2x motion gain as well.
///
/// So this record exists alongside rather than replacing: it is always the live bind, so it cannot go
/// stale the way a single shared record would, and the halves derived from it are the halves of
/// whatever target is actually bound.
static CURRENT_ENGINE_VIEWPORT: Mutex<Option<D3D11_VIEWPORT>> = Mutex::new(None);
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
