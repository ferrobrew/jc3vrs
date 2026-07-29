//! Which single-pass transform each vertex shader gets, and the census that measures the answer.
//!
//! The choice is made by name, because the bytecode alone cannot tell a scene family that should be
//! reprojected from an NDC writer that must not be: the name tables here are the record of which
//! families are which, built from a census over the game's own shader set. A shader is remapped,
//! reprojected, eye-injected for the terrain tessellation path, or deliberately left pristine for a
//! render block that re-issues its draws instead -- and that decision is cached against the blob so
//! both creation paths reach the same answer.

use std::{collections::BTreeMap, sync::atomic::Ordering};

use dxbc_stereo::DxbcError;
use parking_lot::Mutex;

use super::*;

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
/// separate [`single_pass_tree_impostors`](crate::stereo::config::StereoConfig::single_pass_tree_impostors)
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
pub(super) static VS_TRANSFORM_CACHE: Mutex<BTreeMap<(usize, u64), VsTransform>> =
    Mutex::new(BTreeMap::new());

/// The transform [`remember_vs_transform`] recorded for this blob, or `None` if the
/// `CreateVertexProgram` hook never saw it. A recorded [`VsTransform::None`] is a *decision* to leave
/// the shader pristine and is deliberately distinct from never having seen it.
pub(super) fn cached_vs_transform(code: &[u8]) -> Option<VsTransform> {
    VS_TRANSFORM_CACHE.lock().get(&vs_blob_key(code)).copied()
}

/// A pristine shader blob's identity for [`VS_TRANSFORM_CACHE`]: its length and an FNV-1a hash of its
/// bytes. The length is part of the key rather than folded into the hash so that the cheap half of the
/// comparison is exact.
///
/// FNV-1a has no collision resistance, but with ~455 shaders and 64-bit hashes the birthday-bound
/// collision probability is negligible (~1e-15). The length is part of the key to make the cheap half
/// exact, so a collision would require two shaders of the same length with identical hash —
/// astronomically unlikely for this corpus.
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
    Config::lock_query(|c| c.stereo.single_pass.dump_vs_name_census)
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
    let Some(dir) = crate::session::dir().and_then(|r| r.ok()) else {
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
