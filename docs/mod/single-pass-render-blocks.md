# Single-pass stereo, per render block

Single-pass stereo renders both eyes in one geometry walk into a double-wide render target, instead
of re-driving the whole scene draw per eye. The general model path handles the bulk automatically;
the render blocks that don't fit it each need a targeted intercept. This document classifies the
remaining blocks and records the wiring for each.

See [single-pass-stereo.md](single-pass-stereo.md) for the pipeline itself (the cb13 layout, the VS
rewriter, the viewport routing, the collapse).

## The buckets

Every scene draw falls into one of these:

- **(a) cb0-remap / instance-double.** The vertex shader writes `SV_Position` from the global
  view-projection in `cb0` (`m_VPGlobals` rows 29–32, camera at row 4), and the draw is a plain
  `DrawIndexed`/`DrawIndexedInstanced` whose instance count the CPU controls. The `CreateVertexProgram`
  hook rewrites such a VS to read `cb13[(SV_InstanceID & 1)*5 + k]` and emit `SV_ViewportArrayIndex`,
  and the draw detour doubles the instance count and viewport-splits L/R. Handled automatically once
  the VS is patched. An already-instanced block additionally needs its instance-id consumer rewritten
  to `SV_InstanceID >> 1` so the original instance survives the doubling.
- **(b) reproject a baked matrix.** The VS reads a CPU-baked world-view-projection from a per-draw
  constant buffer (not `cb0`). Single-pass reprojects that matrix by the per-eye `M_eye`
  (`clip_eye = M_eye · clip_center`) and re-issues the draw once per eye into the eye's half-viewport.
  This is the TerrainDetail intercept.
- **(c) hull/domain tessellation eye-inject.** The clip transform is built in the domain shader from
  an `m_OffsetViewProjection` constant. The DS is rewritten to reproject by `M_eye` (read from `cb13`
  bound on the domain stage); the paired VS forwards the eye index on a free interpolant. This is the
  base VolumetricTerrain path.
- **(d) GPU-indirect per-eye re-issue.** The draw is `DrawInstancedIndirect` /
  `DrawIndexedInstancedIndirect(NoMutex)` — the instance count lives in a GPU buffer, so the CPU can't
  instance-double and `SV_InstanceID` stays 0 (never selects an eye). The block's `Draw` is detoured
  and re-issued once per eye, with the eye selected by overriding the transform the VS reads (a
  `cb13`-both-slots-set-to-one-eye override for a `cb0`-remapped VS, or a reprojected baked cb for a
  bucket-(b)-shaped VS) and the eye's half-viewport bound.
- **(e) screen-space / NDC-direct.** The VS writes NDC directly with no view transform. Drawn once,
  present in both eyes, excluded from stereo.

## VolumetricTerrainPatch

The retail world's terrain is the patch system, `CRenderBlockTerrainPatch` (per-patch instances of
the quadtree), driven by the `CRenderBlockTypeTerrainPatch` singleton. `Draw` is at `0x14032E540`;
the type-level `Setup`/`SetupConstantBuffers` at `0x14034BB30`/`0x14045D310`.

**Transform source.** `SetupConstantBuffers` binds the global `m_VPGlobals` at vertex `cb0` (the same
rows the general rewriter targets) and the per-patch *streamed* constant buffer at vertex `cb1`
(`SVertexInstanceConstsNoTess = { Matrix4 m_WorldViewProjection; Vector4 m_WorldPos; }`). The
tessellating passes additionally bake `rc->m_OffsetViewProjection` into the hull/domain constants
(`SHullDomainTypeConsts`, filled once per frame per LOD slot). So the base surface goes through cb0
(bucket a/c), while the non-tess path may instead read the baked cb1 WVP (bucket b) — **which of cb0
or cb1 the non-tess VS actually reads for clip position is not yet confirmed** (needs the shader
bundle or a live VS census); it decides whether the DrawIndexed passes are already covered by the
general path or need a bucket-b reproject.

**Draw pass-key routing** (keyed on `m_ActiveRenderPass`):

| Passes | Draw | Notes |
|---|---|---|
| 56, 57 | `DrawIndexedInstancedIndirectNoMutex` | Near patches, GPU-indirect. Bucket **d**. |
| 58, 60 | `DrawIndexed` tail range `[split, end)` | Bucket **a** or **c** (whichever the VS is). |
| 59, 61 | `DrawIndexed` head range `[0, split)` | Same. |
| 14, 38–40 | `DrawIndexed` full range | Depth/other. |

**Wiring recipe.** The DrawIndexed passes ride the general path if their VS is cb0-remapped (verify
via the census). The near passes 56/57 need the bucket-(d) intercept: detour `Draw`, and for each eye
bind the eye's half-viewport and override the transform for that eye (cb13-both-slots if the VS is
cb0-remapped; otherwise reproject the bound cb1 WVP into a mod scratch buffer), then re-issue. Gate
behind its own flag, default off, so it stays isolated from the confirmed TerrainDetail path.

## VegetationBark

Tree trunks and branches, `CRenderBlockBark` (type `CRenderBlockBarkType`, registered name
`"VegetationBark"`). `Draw` (color) at `0x140136F90`, `DrawZ` (depth + velocity) at `0x140136A90`;
type `SetupConstantBuffers` at `0x140102B40`, `Create` at `0x140152DF0`.

**Transform source — baked WVP in cb1, bucket (b).** `Draw`/`DrawZ` stage the clip transform on
vertex `cb1` registers 0–3 via `SetVertexProgramConstants(ctx, 1, 0, &wvp, 4)`, unconditionally,
before any draw-kind routing. Non-instanced: `wvp = M_model(camera-relative) · m_OffsetViewProjection`.
Instanced/billboard: `cb1[0..3] = m_OffsetViewProjection` verbatim and the per-instance model matrix is
applied in-shader. `SetupConstantBuffers` binds `cb0 = m_VPGlobals` too, but only for globals
(wind/time); position does not come from cb0, so the general cb0-remap path does not correct this
block. cb1 also carries opacity/UV (reg 4), the raw model matrix (regs 5–8), and the view matrix
(regs 9–12); `DrawZ`'s velocity pass bakes the previous-frame WVP into regs 5–8.

**Draw-kind routing** (on the instance-data pointer from `CRBIInfo::m_MetaFlags`, then `flags & 8`):
non-instanced `DrawIndexed`; CPU-instanced `DrawIndexedInstanced`; GPU-indirect `DrawIndexedNoMutex`.
All three read the same cb1 regs 0–3, so one reprojection corrects every kind.

**Wiring recipe.** Reproject `cb1[0..3]` by the per-eye `M_eye` (`cb1[0..3]_eye = cb1[0..3]_mono · M_eye`,
row-vector convention — the same `M_eye` the mod builds for cb13) and re-issue per eye into the eye's
half-viewport. Because `Draw` itself bakes cb1, the clean intercept transparently reprojects the
game's own `SetVertexProgramConstants(slot 1)` during a wrapped original-`Draw` call, per eye — see the
shared bucket-(b) mechanism below. The `DrawIndexedNoMutex` (GPU-indirect) sub-path can only be
re-issued, never instance-doubled. Velocity-pass regs 5–8 want the previous-frame `M_eye` (cosmetic;
skip unless chasing motion-vector correctness).

Bark's VS writes `SV_Position`, so the CB-agnostic `reproject_vertex_shader` handles the non-indirect
draws directly — but the VS is shared with the GPU-indirect `DrawIndexedNoMutex` path, and rewriting it
breaks that path (a single indirect instance can't select an eye). So Bark can't be a plain reproject
allowlist entry; it needs the coordinated indirect handling below.

## VegetationFoliage

Grass and ground cover, `CRenderBlockFoliage` (registered `"VegetationFoliage"`), drawn forward-lit in
`RP_VEGETATION_OPAQUE` (0x46). `Draw` at `0x14012DDA0`; type `SetupConstantBuffers` at `0x14010CB00`,
type `Setup` at `0x1401847F0`.

**Transform source — baked OffsetVP in cb2, bucket (b).** `Draw` stages the transform on vertex `cb2`:
reg 0 = the camera-relative world matrix, regs 4–7 = a per-draw copy of `m_OffsetViewProjection`. The VS
composes `clip = world · OffsetVP` from cb2 and writes `SV_Position`, so the CB-agnostic reproject
rewriter applies. cb0 is bound for globals only.

**Draw-kind routing.** Three paths from the instance-data flags: CPU-instanced
`DrawIndexedInstancedNoMutex` (u16 instance count); the **dominant grass path**
`DrawIndexedInstancedIndirect` (instance count in the GPU-only `m_InstDrawParams` args, populated by the
vegetation draw-indirect compute pass); non-instanced `DrawIndexedNoMutex`.

**Wiring.** Same story as Bark: the non-indirect paths are bucket (b), but the dominant grass path is
GPU-indirect and shares the VS, so a naive reproject allowlist entry breaks the grass. This is the
Phase-3 GPU-indirect work: reproject the VS *and* double each `m_InstDrawParams` slot's `InstanceCount`
(dword +1) in an in-place compute pre-pass (the args buffer has no CPU copy), or detour `Draw` and
re-issue the indirect draw per eye with a cb13-both-slots override and the eye half-viewport. The
CPU-instanced path additionally needs the `SV_InstanceID >> 1` consumer rewrite so the doubled instance
id still indexes the original instance.

**Black-in-VR.** Foliage is forward-lit with clustered lighting, not deferred — the type `Setup` binds
the clustered-light constant buffer (FS cb3), the light-cluster index texture (FS t15), GI, reflection,
and sun-shadow cascades to the fragment stage. From `CRenderBlockType::SetupLightingTextures`
(`0x140101160`): the sun-shadow cascades bind at **FS t44/t45** (`0x2C`/`0x2D`) from
`RenderContext+122`/`+123` (falling back to `CShadowManager+25088`), GI/reflection at t12/t31. The
froxel/cluster grid and the shadow sample are indexed from screen position and view depth; if that
volume or those resources are built for a projection/resolution that doesn't match the eye being
rendered (the double-wide/collapse case), the lookup resolves to empty cells and the alpha-blended
foliage receives no light — rendering black, with no deferred pass to rescue it.

Two suspects, one isolation test: (1) the clustered-light grid (cb3/t15), built for the full/center
projection but sampled per eye-half; (2) the sun-shadow cascades (t44/t45), tied to the "shadows a bit
broken" report. **Disable the sun shadows** (`single_pass`'s shadow-disable diagnostic) — if the grass
un-blacks, it's the shadow term (t44/t45); if it stays black, it's the clustered-light grid (cb3/t15).
The fix in either case is per-eye resource correctness, which needs in-game iteration.

## TreeImpostor

Far-distance tree billboard cards, `CTreeImpostorRB`. `Draw` at `0x14034F520`; type `Create` (builds the
static quad index buffer and loads the permutations) at `0x14034EBD0`.

**Transform source — global billboard VP CB, bucket (b).** A single non-instanced `DrawIndexed` over a
static quad index buffer (`6 · min(card_count, 0x1400)` indices); the VS keys off `SV_VertexID`
(`card = id >> 2`, `corner = id & 3`), pulling per-card world position/size/atlas from a texture buffer
at vertex slot 0, and computes clip from the engine's global per-view billboard CB (the
translation-bearing `m_ViewProjectionF`) — not cb0. It writes `SV_Position` directly and there is **no
GPU-indirect path** sharing the VS.

**Wiring recipe.** This is the one clean allowlist win: add the `treeimpostor*` VS name prefixes to the
reproject set. The CB-agnostic reproject rewriter post-multiplies `SV_Position` by `M_eye` and emits
`SV_ViewportArrayIndex`; the plain `DrawIndexed` is then auto-doubled by the existing detour with the
L/R viewport split. The card faces the center camera rather than each eye, but at impostor range the
per-eye facing difference is arcseconds — `M_eye` supplies the correct parallax/disparity, so shared
facing is correct and sufficient. No per-draw special-casing.

## Occluder

Depth-priming occluder proxies, `CRenderBlockOccluder` (registered `"Occluder"`), injected once per
frame into `RP_Z_OCCLUDERS` (pass 47) to prime the main camera depth buffer for early-Z. `DrawZ` at
`0x14017DFA0`; the block is depth-only (null fragment program).

**Transform source — baked WVP in cb1, bucket (b).** The non-instanced path bakes
`WVP = (world − camera_offset) · m_OffsetViewProjection` (`CalculateOffsetWorldViewProjectionMatrix`,
`0x140136070`) into vertex `cb1` regs 0–3 (reg 4 a depth bias) and issues one `DrawIndexed` per box. The
instanced path (`gfx.occluders.use_instancing`) instead reads cb0 `m_VPGlobals` with per-instance world
rows from a stream and issues one `DrawIndexedInstanced` — which collides with the `SV_InstanceID & 1`
eye-doubling.

**Occlusion-culling concern — resolved.** This only primes the GPU depth buffer for the same-frame eye
views; nothing reads it back on the CPU, and the CPU software-occlusion system (BFBC) is entirely
separate. So forcing it per-eye is safe and correct — and necessary: a mono occluder pass primes the
shared depth with one eye's projection, giving the other eye wrong early-Z (over-cull holes or lost
priming).

**Wiring recipe.** Force `gfx.occluders.use_instancing = 0` so every box goes through the non-instanced
`DrawIndexed` (baked cb1 WVP), then treat it as bucket (b): reproject `cb1[0..3]` by `M_eye` and re-issue
per eye into the eye's half-viewport (row 4 depth-bias unchanged). The box count is small and bounded.

## The shared bucket-(b) / (d) mechanism

Several blocks (Bark, the terrain patches, and the GPU-indirect foliage) reduce to the same primitive:
re-issue a block's `Draw` once per eye, with the eye's transform and half-viewport. Two eye-selection
paths cover every case:

- **Reproject a baked cb** (bucket b, when the VS reads a CPU-baked WVP): before each eye's re-issue,
  overwrite the baked matrix rows by `M_eye` (per-row `M_eye · row`, matching the confirmed TerrainDetail
  math). When the block bakes the cb inside its own `Draw`, arm a reproject-interception on the game's
  `SetVertexProgramConstants(slot 1|2, start 0, count ≥ 4)` for the duration of a wrapped original-`Draw`
  call, so the game's own upload is transparently reprojected — no need to replicate the bake.
- **cb13 override** (bucket d, when the VS is cb0-remapped to `cb13`): before each eye's re-issue, write
  the mod's `cb13` with both eye slots set to that eye's rows, so a single (indirect) instance reads the
  correct eye.

Gate the new families behind their own default-off flags, so the confirmed model/terrain-detail paths
stay isolated while each new block is validated in-game.
