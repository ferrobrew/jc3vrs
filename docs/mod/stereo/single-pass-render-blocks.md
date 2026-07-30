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

  **The baked matrix is stored column-wise.** Every bucket-(b) VS in the bundle consumes its four
  registers as a multiply-add chain rather than four `dp4`s — for TerrainDetail (`sh_0224`), Bark
  (`sh_0246`), Foliage (`sh_0249`) and Occluder (`sh_0138`) alike the body reads

  ```
  mul r1, p.yyyy, cb[k+1]
  mad r1, p.xxxx, cb[k+0], r1
  mad r1, p.zzzz, cb[k+2], r1
  add o0, r1,     cb[k+3]          // or a fourth mad on p.wwww
  ```

  so `clip = Σ_i p_i · cb[k+i]`: each register is a **column** of the transform acting on the position
  as a column vector, not a row. The per-eye buffer is therefore `cb[k+i]_eye = M_eye · cb[k+i]_mono`,
  one entry at a time — the same `mul_vec4` the mod applies. (`cb13`'s `M_eye` block is the other
  convention: the rewriter's epilogue is a `dp4` chain, so those rows are rows.)
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

**Wiring — no block intercept, but the near passes are covered.** The `DrawIndexed` passes ride the
general path when their VS is cb0-remapped. The near passes 56/57 have no bucket-(d) block intercept;
instead `single_pass.indirect_per_eye` (on by default) detours the two GPU-indirect context-vtable
slots wholesale and re-issues any such draw once per eye with both viewport slots pinned to that eye's
half. That makes the near patches present and correctly sized in both eyes without parallax — before
it, they inherited whatever viewport the previous draw left bound, usually the full double-wide one,
which stretched them 2x horizontally and made them appear to slide across the world. A block-level
intercept overriding the transform per eye is what would add the parallax; it is not built.

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

**Wiring — `single_pass.bark`, built, on by default.** `Draw` and `DrawZ` are detoured and re-issued once per eye
into the eye's half-viewport, with `cb1[0..3]` reprojected as `cb1[k]_eye = M_eye · cb1[k]_mono` (one
entry at a time — the same `M_eye` the mod builds for cb13). Because `Draw` itself bakes cb1, the
intercept reprojects the game's own `SetVertexProgramConstants(slot 1)` during a wrapped
original-`Draw` call rather than replicating the bake — the shared bucket-(b) mechanism below. This
covers all three draw kinds uniformly; the `DrawIndexedNoMutex` (GPU-indirect) sub-path can only ever
be re-issued, never instance-doubled.

The velocity pass's previous-frame WVP at regs 5–8 is **not** reprojected: it would need the previous
frame's `M_eye` retained and a second armed range. Bark's motion vectors therefore carry the centre
view's parallax, which SMAA and FSR reproject slightly wrongly.

Bark's VS writes `SV_Position`, so the CB-agnostic `reproject_vertex_shader` would handle the
non-indirect draws directly — but the VS is shared with the GPU-indirect path, and rewriting it breaks
that path (a single indirect instance cannot select an eye). That is why Bark is not a plain reproject
allowlist entry and takes the whole-`Draw` re-issue instead.

## VegetationFoliage

Grass and ground cover, `CRenderBlockFoliage` (registered `"VegetationFoliage"`), drawn in
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

**Wiring — `single_pass.foliage`, built, on by default.** Same story as Bark: the non-indirect paths
are bucket (b), but the dominant grass path is GPU-indirect and shares the VS, so a reproject allowlist
entry would break the grass. `Draw` (colour, `0x14012DDA0`) and `DrawZ` (prez/shadow/velocity/RSM,
`0x14012D9B0`) are therefore detoured and re-issued per eye with `cb2[4..7]` reprojected, which covers
every draw kind without touching the VS. Both entry points stage the same `cb2[4..7]` copy of
`m_OffsetViewProjection`, and both have to be reprojected: a prepass left at the centre view primes
depth for geometry neither eye draws there, and the per-eye colour fragments are rejected against it.
`DrawZ`'s velocity permutation also stages the previous frame's offset VP at `cb2[11..14]`, which is
left centred — the same deferral bark's velocity matrix takes.

The cheaper shape — reproject the VS and double each `m_InstDrawParams` slot's `InstanceCount` (dword
+1) in an in-place compute pre-pass, since the args buffer has no CPU copy — is not built; it would
also need the `SV_InstanceID >> 1` consumer rewrite on the CPU-instanced path so the doubled instance
id still indexes the original instance.

### Black-in-VR: the clustered grid is *not* the cause

The earlier version of this section claimed the black grass was the clustered froxel grid and that
`single_pass.clustered_per_eye` had fixed it. Both halves of that are wrong, and the shader bundle
says so directly.

**The visible grass is deferred, not forward-lit.** The block's colour permutations split cleanly:

| Permutation | Outputs | `LightLookup` (t31) / `LightingFrameConsts` (cb3) |
|---|---|---|
| `vegetationfoliage` (`sh_0794`), `…nodiscard`, `…objectspacenormalmap`, `…transmission` | 4 × `SV_Target` | **absent** |
| `vegetationfoliageblend` (`sh_0796`), `…blendaoit` | 1 × `SV_Target` | present |

The four-target permutations are G-buffer writers; only the `blend` pair is forward-lit. The user's own
session log settles which one runs: the foliage colour draws are counted inside the collapse's G-buffer
pass range `[0x2f, 0x55)` (`vegetationfoliagehwinstanced`, ~21k draws/frame in the instanced eye-parity
census), and `RP_VEGETATION_TRANSPARENT` is outside it. So the grass that renders black writes the
G-buffer and is lit by the deferred resolve — the same resolve that lights the terrain correctly.

**Even for the forward permutation, an empty grid cannot produce black.** In `sh_0796` the whole
clustered-light section is guarded and purely additive:

```
ishr r12.xy, r6.xyxx, l(6, 6, 0, 0)          // absolute tile from ftoi(SV_Position.xy)
ld_aoffimmi_indexable(0,0,k)(texture3d) …, t31.xyzw   // four chunk bitmasks
or r2.x, r1.y, r1.x
if_nz r2.x
  …                                           // per-light loop
  mad r9.xyz, r21.xyzx, r17.xyzx, r9.xyzx     // accumulate only
endif
```

`r9` (sun + sky SH + GI + reflection) is complete *before* the lookup. A wrong or empty grid removes
local point/spot light; it cannot zero the sum. The grid is a real correctness problem for local
lighting — it is not a black-maker.

**And the per-eye split has never engaged at the mod's own render resolution.** `TileGrid::splittable`
requires the double-wide width to be a multiple of `2 * 64`. The VR render size is `2015 × 2240` per
eye, so the collapse target is `4030` wide, and every session logs

```
WARN single_pass: per-eye froxel grid declined: the 4030px double-wide render width is not a
multiple of 128 …; the clustered light assignment ran whole-grid
```

That is why toggling `single_pass.clustered_per_eye` changed nothing. Making the split usable means
rounding the per-eye render width up to a multiple of 64 in `vr::engine_render_resolution` (so the
double-wide is a multiple of 128) — worth doing for the local-lighting correctness it buys, but it will
not touch the black grass.

**Corrected resource map.** The old text's slot attributions were wrong; from
`CRenderBlockType::SetupForwardLightingResources` (`0x140101250`),
`CRenderBlockType::SetupLightingTextures` (`0x140101160`), and the shader bundle's own binding tables:

| Slot | What it actually is | Source |
|---|---|---|
| FS cb3 | `LightingFrameConsts` (point/spot light arrays, `AOLightInfluence`, `SkyAmbientSaturation`) | `RenderContext + 0x3A8` |
| FS t15 | `GbufDiffuse` — `CGraphicsEngine::m_GBufferTexture[0]`, i.e. GBuffer0 | engine, not the context |
| FS t31 | `LightLookup`, the froxel grid (`texture3d`, `uint4`) | `RenderContext + 0x3B0` |
| FS t14 | `ShadowMapTexture`, the sun-shadow cascade atlas (`texture2darray`) | shadow manager |
| FS t44/t45 | `HorizonMap0`/`HorizonMap1` — world-space horizon maps sampled at `worldXZ * 3.05e-5 + 0.5` | `RenderContext + 122/123`, falling back to `CShadowManager + 25088` |

`m_GBufferTexture[0]` is named in the symbol dump's `CRenderBlockType::SetupForwardLightingResources`;
the t44/t45 identification comes from the `HorizonMap0`/`HorizonMap1` binding names in `sh_0796`, whose
sample is world-space, not screen-space. `docs/engine/rendering/lighting-shadow-pipeline.md` §4.2 calls t44/t45
the sun-shadow cascades and should be corrected the same way.

**What is still open.** The deferred permutation's G-buffer albedo is

```
o0.xyz = DiffuseMap.rgb · AmbientOcclusionMap.x(v1.zw) · cb1[3].xyz · v5.xyz
```

and its only screen-space input is the LOD-dissolve screen door
(`dp2 v0.xy, l(0.467944, -0.703648)`, compared against `1 - v5.w`) — which is what produces the
*speckle* in the screenshots, and which goes fully opaque at `v5.w = 1` and fully discarded at
`v5.w = 0`. `v5` is `TEXCOORD4`: a constant `(1,1,1, cb2[8].x)` in the non-instanced VS (`sh_0249`),
but a **per-instance colour + fade** in the instanced ones — a structured-buffer fetch indexed by
`SV_InstanceID` in `sh_0253`, and the per-instance vertex attribute `v7` in `sh_0251` (the variant the
log shows actually running). So "dark and speckled together" is the signature of `v5` reading wrong,
and after that of the G-buffer *normal* (`o1`), which would leave the deferred resolve computing
`N·L ≈ 0` on foliage while the terrain beside it lights correctly.

The one-toggle experiment that separates them is `single_pass.foliage`, and it used to be unable to
engage at all — see the next section, which is now fixed.

## The vegetation `cb0[4]` misclassification (fixed)

The rewriter's per-eye set is `cb0[{4, 29..32}]`, and it patches any vertex shader that references one
of them. But those five rows are not one thing. `cb0[29..32]` is `m_OffsetViewProjection` — it can only
be a clip-space transform. `cb0[4]` is the camera world position (`RenderContext + 0x260`, the render
camera's `m_TransformF` translation row, copied by `SetGlobalShaderProgramCameraConstants` at
`0x140186370`), and shaders read it for several unrelated reasons.

Every `vegetation*` vertex shader that reads `cb0` reads **only** `cb0[4]`, and none of the 32 of them
references `cb0[29..32]` — most declare `dcl_constantbuffer CB0[5]`, which cannot even address those
rows. Their roles:

| Family | `cb0[4]` use | Clip position |
|---|---|---|
| `vegetationfoliage*` | `add r0.w, r1.y, cb0[4].y` — turns the camera-relative position back into a world one for the wind-noise texture lookup | `cb2[4..7]`, the baked `OffsetVP` |
| `vegetationbark*instanced` | `add r1.xyz, r0.xyzx, -cb0[4].xyzx` — makes the per-instance world position camera-relative | `cb1[0..3]`, the baked `OffsetVP` |

Neither takes its clip position from `cb0`, so the remap bought them nothing: instance-doubling and
`SV_ViewportArrayIndex` routing put them in both eye halves, but both halves were drawn from the
collapsed **centre** camera. It also *locked out the fix*: `baked_cb_intercept_ready` declines whenever
`BOUND_VS_PATCHED` is set, so `single_pass.foliage` and `single_pass.bark` could never run.

### What that looks like, and why it is a *mono* defect

The symptom was first reported as vegetation that is roughly in the right place but does not stay put
as the camera moves, and it was first explained here as zero disparity between the eyes. **That
explanation was wrong**, and the correction matters because it changes which mechanism to chase: it was
observed in the **left-eye desktop mirror, with the headset not being worn**. A single view cannot show
a stereo-depth defect at all.

Under the collapse the render camera stays centred and each patched shader gets its eye's view from
`cb13`; so in the left half of the double-wide target, terrain, props and characters are drawn from the
**left eye's** viewpoint while the vegetation is drawn from the **centre**. Within that one image the
vegetation therefore sits at a rigid world-space offset of `campos_centre − campos_eye0` — half the
IPD, ~32 mm — from the ground it grows out of. `world_offset` is `eye_position − 0.5·(pos0 + pos1)`
(`vr::frame`), so it is the eye-to-midpoint vector: lateral, in the camera's own basis, with no forward
component for a symmetric headset.

The reason this only shows up when you walk up to a bush is a general property of projection, not a
coincidence: **two pinhole projections that share an optical centre differ by a homography on the
image**, i.e. by a function of ray *direction* only. So a wrong projection or a wrong view rotation
displaces geometry by a fixed angle at every depth, and a wrong *optical centre* displaces it by
`offset/distance`. Only the second can change as you approach. That is the discriminator, and it points
at the viewpoint:

| Candidate | Distance-dependent? | Magnitude |
|---|---|---|
| Vegetation drawn from the centre viewpoint instead of the eye's | **yes**, `32 mm / d` — 0.4° at 5 m, 3.7° at 0.5 m | half the IPD |
| Bark's remapped `cb0[4]` against a centre-baked `OffsetVP` | no — same optical centre, different projection, so a fixed angular warp | — |
| Foliage's remapped `cb0[4]` in the wind-noise UV | no | bounded by the *vertical* component of a 32 mm offset, against a world-scaled noise lookup |

So of the two `cb0[4]` mis-substitutions, bark's actually *supplied* the correct eye viewpoint (it
rebases by `cb0[4]`, which the remap made per-eye) and left only the fixed projection difference, while
foliage's is confined to a wind-noise coordinate and is negligible. The distance-dependent term belongs
entirely to foliage, whose clip path never touches `cb0` and was therefore wholly centre-viewed. The
`cb0[4]` misclassification is what *prevented the repair*, not what caused the displacement.

What the arithmetic predicts precisely is a rigid ~32 mm lateral displacement that swings around with
the camera heading and grows angularly as you close on it. Whether "recedes as I approach" is the right
words for that is interpretation — separating a lateral offset from a depth one needs a frame capture,
which has not been taken. The mechanism is pinned by the distance dependence regardless of the
adjective.

The stereo consequence — both eyes seeing the identical centre image, so the vegetation also fuses at
infinite depth in a headset — is real, and the same fix addresses it. It is simply not what was seen
here.

**The fix, in `stereo::single_pass::baked_cb_block_owns_vs`.** While a family's intercept flag is on,
its vertex shaders are declined by the `cb0` remap at creation, so the block intercept owns them end to
end. It is name-prefixed (`vegetationbark`, `vegetationfoliage`) but bytecode-confirmed: a permutation
that really does read `cb0[29..32]` is left to the remap. The decline and the intercept share a flag
because either alone is wrong — declining without the intercept forwards an unpatched instanced draw
once at whatever viewport is bound, which is the 2× horizontal stretch.

Reprojecting `cb2[4..7]` by `M_eye` is exactly the right correction, and not only approximately: the
shader computes `clip = OffsetVP_c · (world − campos_c) = VP_centre · world`, and
`M_eye · VP_centre = VP_eye · VP_centre⁻¹ · VP_centre = VP_eye`. The eye's viewpoint *and* its
projection both land, for every draw kind the block issues.

### The other 27 shaders claimed on `cb0[4]` alone (triaged)

Fifty of the bundle's 455 vertex shaders are claimed on a `cb0[4]` reference alone — 23 vegetation, and
27 others, pinned by `corpus_vegetation_reads_the_camera_position_but_never_the_view_projection`. Since
the defect is mono-visible, they deserve a second look, but the conclusion is that *declining the remap
is not what fixes them*. Being claimed is not the disease; taking clip from a centre-camera matrix is,
and that is true whether or not the remap claims the shader. Un-patching without supplying a per-eye
transform makes things worse, not better — an unpatched instanced draw is forwarded once at whatever
viewport is bound.

All fifty were then read out of the disassembly one at a time, and the classification is pinned by
`corpus_camera_only_population_is_the_triaged_fifty`. The axis that matters is where each takes its
clip from:

- **A baked world-view-projection in the shader's own constant buffer.** Reprojectable:
  `M_eye · VP_centre = VP_eye`, exactly. `generaljc3` (`sh_0065`), `landmark` (`sh_0085`), `layered`
  (`sh_0086`), and `layeredblend` (`sh_0087`) are one shared body — `clip = cb1[0..3] · objectPosition`,
  `cb0[4]` differenced into a `dp3` for a LOD fade (`mad_sat o3.w, r0.x, l(-0.0001), l(1.0)`), and a
  post-projection depth bias `mad o0.z, cb2[0].x, r0.w, r0.z`. None of them is an NDC writer: each
  multiplies an object-space position by a 4×4 matrix. All four are now on `REPROJECT_NAME_PREFIXES`
  and take `single_pass.reproject_camera_only` (on by default, requires `single_pass.reproject`), at no
  extra draw cost. The vegetation families and `terrainshadowsimple` are also in this group but are
  owned elsewhere — the block intercepts and the shadow atlas respectively.
- **The global full view-projection, `cb0[0..3]`.** `RenderContext::m_ViewProjectionF`
  (translation-bearing, mapping *absolute* world to clip), staged into `m_VPGlobalConstData[0..3]` by
  `SetGlobalShaderProgramCameraConstants` — verified in the IDB, not just read off the def. It is
  per-view data, but it is **not** one of the five rows the remap makes per-eye, so these shaders are
  centre-viewed for a reason the remap could never have fixed. The legacy water surfaces (`waterbelow`,
  `waterbox`, `waterboxbelow`, `waterboxsurface`, `waterhighend`, `watershader_lod0/1`), the clouds
  (`cirrusclouds`, `cloudflythrough`, `clouds`, `cloudsshadow`), and `weather` are all here. They are
  structurally reprojectable — the reprojection post-multiplies whatever clip the shader computed, so
  the source buffer is irrelevant — and because they are already *claimed*, their draws are already
  instance-doubled and viewport-routed, so switching them needs no new draw wiring. They are
  deliberately left alone anyway: their name spaces contain outright NDC writers a prefix would sweep
  up (`waterbumpcomposite` writes `mad o0.xy, v0.xyxx, l(2,-2), l(-1,1)`, `waterfoamsub` passes `v0`
  straight to `o0`), the cloud distances make `32 mm / d` vanish, `cloudsshadow` is a shadow pass, and
  `weather`'s cull branch writes a literal `o0 = (2,2,2,1)`.

  Three of the legacy water permutations — `waterbox`, `waterboxbelow`, `waterboxsurface` — reconstruct
  their world position from `cb0[4]` before that multiply (`add r1.xyz, r0.xyzx, cb0[4].xyzx` ahead of
  the `cb0[0..3]` chain), so for them the remap is not merely inert: it shifts the reconstructed world
  position by the eye offset while the projection stays centred, adding the error instead of correcting
  it. (`waterbelow`, `waterhighend`, and `watershader_lod0/1` do *not* — they build an absolute grid
  position from `cb2[0]` and read `cb0[4]` only for an output view vector. Worth separating, because it
  is the difference between a wrong sign and a no-op.) Reprojecting them is still not a standalone
  change: `single_pass.water_uv_per_eye` biases their projective screen UVs on the premise that the
  geometry lands at the *centre* view, so the two have to move together, with the staged UV rows
  rebuilt from the eye's full view-projection. The water-box surface grid is drawn from
  `NWater::DrawWaterBoxSurface` rather than `WaterBoxRenderBlock::Draw` and would need its own
  intercept as well.
- **Direct NDC writes.** `lpvinit` and `lpvinitbilinear` rasterize the light-propagation volume from a
  vertex id straight into device coordinates (`mad r0.x, r0.x, l(2.0), l(-1.0)`, `o0.w = 1`). They must
  never be reprojected. Their only `cb0` reads are the scalars `cb0[4].w` and `cb0[5].w`, so the remap
  claims them on a lane that is not a position at all — and `cb13` reproduces `cb0[4].w` verbatim, so
  the claim is inert.
- **No clip position at all.** `terrainprezsimple`, `nvwaterbox_tess`, and the five
  `particleeffecttess*` emit tessellation control points; clip is built downstream in the domain
  shader, and the reprojection refuses them with `NoPositionOutput`. The particles read only
  `cb0[4].w` / `cb0[5].w`.

`nvwaterbox` (`sh_0163`) and `nvwaterbox_tess` (`sh_0290`) sit apart, and they are the clearest
remaining instance of the bug this whole section is about. Both add the camera position to a
model-space position before the baked `cb1[0..3]` multiply, so `cb0[4]` really is on their position
path. `single_pass.nvwater_per_eye` is precisely a baked-cb block intercept — it restages that matrix
from the eye's own camera — but `nvwater*` is **not** on `BAKED_CB_VS_NAME_PREFIXES`, so the remap
claims these two before the handler that owns them is ever consulted, exactly as it did `generaljc3`.
The remapped `cb0[4]` is then added on top of a transform that already accounts for the eye offset,
displacing the water box by that offset again. Half an IPD on a water surface, so not urgent, but the
fix has the same shape as the vegetation one: add the prefix so the remap declines while the intercept
is on. It is not done here because it needs a `BlockIntercept` variant for the WaveWorks flag (which
`water.rs` currently queries directly) and live validation that the decline and the intercept's
`draw_per_eye_half_ignoring_bound_vs` re-issue compose — the two must land together, since declining
without the intercept forwards the draw once at whatever viewport is bound.

**The bound-shader gate also had to change.** `baked_cb_intercept_ready`'s `BOUND_VS_PATCHED` check
reads the shader bound by the *previous* draw, because these blocks bind their own inside the `Draw`
being wrapped. Bark is drawn interleaved with patched model geometry, so the check would stand its
intercept down on a neighbour's shader. `BoundVsGate::Owned` skips it for the two families whose
shaders are declined at creation and therefore provably unpatched; every other caller keeps
`BoundVsGate::Checked`.

## TreeImpostor

Far-distance tree billboard cards, `CTreeImpostorRB`. `Draw` at `0x14034F520`; type `Create` (builds the
static quad index buffer and loads the permutations) at `0x14034EBD0`.

**Transform source — global billboard VP CB, bucket (b).** A single non-instanced `DrawIndexed` over a
static quad index buffer (`6 · min(card_count, 0x1400)` indices); the VS keys off `SV_VertexID`
(`card = id >> 2`, `corner = id & 3`), pulling per-card world position/size/atlas from a texture buffer
at vertex slot 0, and computes clip from the engine's global per-view billboard CB (the
translation-bearing `m_ViewProjectionF`) — not cb0. It writes `SV_Position` directly and there is **no
GPU-indirect path** sharing the VS.

**Wiring — `single_pass.tree_impostors`, built.** The one clean allowlist win: `treeimpostor*` is a VS
name prefix in the reproject set. The CB-agnostic reproject rewriter post-multiplies `SV_Position` by
`M_eye` and emits `SV_ViewportArrayIndex`, and the plain `DrawIndexed` is then instance-doubled by the
draw detour with the L/R viewport split. The card faces the centre camera rather than each eye, but at
impostor range the per-eye facing difference is arcseconds — `M_eye` supplies the parallax, so shared
facing is sufficient. No per-draw special-casing.

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

**Wiring — `single_pass.occluder`, built with an unenforced precondition.** `DrawZ` is detoured and
re-issued per eye into the eye's half-viewport with `cb1[0..3]` reprojected by `M_eye` (row 4, the depth
bias, unchanged). The box count is small and bounded.

This assumes the non-instanced path. Nothing in the mod writes `gfx.occluders.use_instancing`, so if
the instanced path is selected the block bakes no `cb1` matrix and the re-issue reprojects nothing —
both eyes are primed from the centre view. The intercept logs when its armed reprojection never fires,
which is what surfaces the condition; forcing the cvar needs the cvar seam, which is not RE'd.

## The shared bucket-(b) / (d) mechanism

Several blocks (Bark, the terrain patches, and the GPU-indirect foliage) reduce to the same primitive:
re-issue a block's `Draw` once per eye, with the eye's transform and half-viewport. Two eye-selection
paths cover every case:

- **Reproject a baked cb** (bucket b, when the VS reads a CPU-baked WVP): before each eye's re-issue,
  overwrite the baked matrix entries by `M_eye` (entry-wise `M_eye · cb[k]`, since the entries are the
  matrix's columns). When the block bakes the cb inside its own `Draw` — all four built intercepts do —
  a reproject-interception is armed on the game's `SetVertexProgramConstants` for the duration of a
  wrapped original-`Draw` call, so the game's own upload is transparently reprojected and the bake need
  not be replicated. The arm is keyed to the block's graphics context: the detour sees every
  vertex-constant stage in the process, and `(slot, offset)` alone does not identify a block.
- **cb13 override** (bucket d, when the VS is cb0-remapped to `cb13`): before each eye's re-issue, write
  the mod's `cb13` with both eye slots set to that eye's rows, so a single (indirect) instance reads the
  correct eye. **Not built** — no block uses this path yet.

Each family has its own flag (`single_pass.bark` and `single_pass.foliage` on by default;
`single_pass.occluder`, `single_pass.terrain`, and `single_pass.tree_impostors` off), so the model and
terrain-detail paths stay isolated while a new block is validated in-game.

A re-issue marks itself while it runs, so the draw and viewport detours leave the block's own calls
alone rather than compounding the split, and the intercept declines outright if the bound vertex
shader turns out to be one the rewriter patched — that block already renders both eyes from a single
draw.
