# Issue #40: terrain walls and cave ceilings render black at grazing angles in VR

Static reverse-engineering of the base VolumetricTerrain (`CRenderBlockTerrain`) draw pipeline
against the release IDB, the 2016 symbol dump, and the shipped shader bundles. The prior in-headset
work (issue #40, commit `8fa3521`) had confirmed the black tiles are absent from the G-buffer and
had ruled out every block-level lever; this pass maps the full pipeline, corrects two earlier
misconceptions, and lands two live toggles that discriminate the remaining candidates.

## How a base-terrain tile gets drawn (release-verified)

The base terrain is streamed as four patch maps (`terrain_patch_lod9..12`) in the generic Avalanche
patch system; each patch owns a `CRenderBlockTerrain` constructed in the map's constant memory
(`OldTerrainPatchSystem::TerrainPatchSystemCreateLayers`, `0x14032FAB0`). Per frame:

1. **Cull.** `CLandscapeManager::UpdatePatchSystem` (`0x14034B2F0`) assembles up to 12 cull frusta
   and runs a BFBC AABB cull per patch per frustum, writing one bit per frustum entry into the
   patch header's `m_VisibilityBits` (header +0x1C). The entries are: 0 = the occluder manager's
   main cull camera, 1..=N = shadow cascades, then two RSM frusta, and a *second run of the same
   main cull camera* as the final entry (frustum id 12). Both main-camera entries share one cached
   parameter set (`GetBFBCFrustumParamsForCameraAndTime`, `0x1400D68B0`, keyed by camera + time), so
   **the color-pass and depth-prepass bits are computed from identical inputs and cannot disagree**.
   The frustum planes are generic Gribb–Hartmann extractions from `m_View` × `m_ProjectionF`
   (`BFBCCalculateCameraFrustum`, `0x141E3B930`) — convention-agnostic (0 ≤ z ≤ w volume), so the
   mod's stamped union projection yields a geometrically sound frustum, and the mod's
   `disable_bfbc_occlusion` (default on) already truncates the occluder frustums to just the camera.
2. **Submit.** The per-patch fragment `TerrainPatchUpdate` (`0x1410C98A0`; registered under the
   `TerrainPatchUpdate` hash) updates alpha fade, sort id, and the tessellated index count, then
   adds the block to one render pass per set visibility bit: id 0 → the color passes (near-detail
   for base-LOD tiles within one tile of the camera, near for other high-detail-LOD tiles, far
   otherwise — engine pass indices 62/63/64), ids 1..=8 → shadow passes, 9..=10 → RSM, id 12 → the
   depth prepass.
3. **Draw.** Both the prepass and the color pass run the same tessellation pipeline
   (`Setup`/`SetupZ` at `0x14032B6D0`/`0x14032B4D0`, `Draw` at `0x14032BB80`; `DrawZ` is a nullsub —
   the Z-family `SetupZ`/`DrawZ` path at pass ids 47–50/140 is not how terrain reaches the depth
   prepass). The hull program is selected by `HullClipType` (`0x14032B450`).

## Corrected: what `HullClipType` actually selects

The dump symbols (`CRenderBlockTerrain.cpp:1437`) give the real semantics, which differ from the
earlier assumption ("2 = the color-pass LOD clip"):

- **1 = the LOD clip.** Selected **unconditionally for every LOD>9 tile**, in every pass
  (`m_DisableLodClipping` is not even compiled into the release function). The clip hulls
  (e.g. `sh_1493` in `Shaders_F`) sample a `VisibilityMask` texture — the global terrain mask that
  `UpdateGlobalMask` writes from each finer tile's streaming/alpha state — at the patch's four
  corners, and zero the tessellation factors (discarding the patch) when all four read < 0.05,
  i.e. when a finer-LOD tile has *fully faded in* over that footprint.
- **2 = the water clip.** For base-LOD tiles at the context's high-detail LOD when the camera is
  above water: discard patches below the cached water level (the water render pass draws there
  instead). This is what the earlier `force_terrain_hull_clip` toggle overrode — which is why it
  had no effect on the black tiles.
- **0 = no clip.**

The GPU hull culls (back-patch, frustum, cull-by-detail) are separate, flag-gated constants in the
hull/domain CB; the in-headset tests already disabled them at verified offsets with no effect, and
they are per-eye anyway — inconsistent with the eye-identical symptom.

## The mechanism that fits every observation

The LOD clip keys on **residency** (the mask), but whether the finer tile actually draws keys on
**visibility** (the BFBC bits). The engine's implicit invariant is: a resident finer tile that got
culled is off-screen, so the coarse tile covering the same footprint is culled too. If that
invariant breaks — the finer tile's AABB fails the main-camera cull while its footprint is on
screen — then the coarse tile has carved the footprint out (in the prepass *and* the color pass)
and the finer tile fills it in neither: **nothing draws there in any pass**. The pixels keep the
clear/far depth, the sky/atmosphere resolve leaves them dark, and the froxel fog tints them the
observed deep blue — which also explains why they don't read as see-through holes. (The issue's
"the depth prepass still draws them" was an inference, not a measurement; the heavy fog tint is
actually evidence of *far* depth — a genuinely near wall would accumulate almost no fog.)

This fits every data point: tile-quantized staircase boundaries (the finer tile's footprint on the
mask grid), discrete flips on small rotations (the finer tile's AABB crossing a cull plane),
world-locked and identical in both eyes (one CPU cull per frame), immune to every GPU-side lever,
and present since VR terrain first worked.

What it does *not* yet pin down is **why the cull drops a visible finer tile** — the widened union
frustum is conservative and covers both eyes by construction. Candidates, in rough order: the cull
pose lagging or diverging from the render pose (game-camera vs HMD orientation coupling); the
union projection not actually reaching this cull at the moment the cached frustum recomputes; or
the wrong-invariant half being the *mask* (a finer tile marked fully-resident whose draw is
legitimately absent for another reason, e.g. missing stream data).

## The two discriminators (landed as config/debug-UI toggles)

- `force_terrain_lod_clip` — maps `HullClipType` 1 → 0, disabling the LOD clip so coarse tiles draw
  their full geometry. If the black fills with (coarser) rock, the mechanism above is confirmed:
  the black was a finer tile's mask footprint that nobody drew.
- `force_terrain_patch_visibility` — a detour on `TerrainPatchUpdate` that ORs bit 0 and the final
  entry's bit into every patch's `m_VisibilityBits`, so every *resident* tile is submitted to the
  color passes and depth prepass regardless of the cull. If this alone fixes the black, the BFBC
  cull of the finer tile is the root cause, and this toggle (scoped to the main-view bits) is also
  a viable fix — the cost is drawing all resident base-terrain tiles, which is bounded by the
  streaming radius.

Expected outcomes: both fix it → cull is the root cause (prefer the visibility fix, or repair the
union-cull path); only the LOD-clip force fixes it → the finer tile is missing for a non-cull
reason (chase its `Draw` early-outs: displacement texture, indices, alpha); neither fixes it → the
mechanism is wrong and the next probe is a per-pass draw-count comparison from inside
`CRenderBlockTerrain::Draw`.

## Release addresses established this session

| Item | Address |
|---|---|
| `TerrainPatchUpdate` (base system's per-patch fragment; dormant in retail — see the correction below) | `0x1410C98A0` |
| `TerrainPatchSystemFragment` (the **live** patch system's per-patch fragment) | `0x1410CAC50` |
| Construction/destruction stub fragment | `0x1410C8FA0` |
| `CRenderBlockTypeTerrainPatch::Setup` (inlines the live hull-clip selection) | `0x14034BB30` |
| `CLandscapeManager::UpdatePatchSystem` | `0x14034B2F0` |
| `OldTerrainPatchSystem::TerrainPatchSystemCreateLayers` | `0x14032FAB0` |
| `CRenderBlockTerrain::Setup` / `SetupZ` / `Draw` / `DrawZ` | `0x14032B6D0` / `0x14032B4D0` / `0x14032BB80` / `0x14032BB70` (nullsub) |
| `UpdateTessellatedTriangles` | `0x1410C9690` |
| `CRenderBlockTerrain::UpdateSortID` / `UpdateAlpha` / `SetAlpha` / `UpdateGlobalMask` | `0x1410C9090` / `0x14032BED0` / `0x14032BF40` / `0x14032BDB0` |
| `BFBCCalculateCameraFrustum` / `BFBCCalculateOccluderFrustums` | `0x141E3B930` / `0x141E46CA0` |
| `NGraphicsEngine::SetGlobalShaderProgramCameraConstants` (uploads `rc->m_FrustumPlanes` to VS globals slot 38) | dump `0x1401ED500` (release not needed yet) |
| Patch header fields (`m_VisibilityBits` +0x1C, `m_Lod` +0x1A, status +0x14, occludee +0x28) | `patch_system.pyxis` |

Terrain hull/domain shaders in `Shaders_F.shader_bundle`: hulls `sh_1492..sh_1504` (the
mask-sampling LOD-clip variants declare a `VisibilityMask` t0), domains `sh_1513..sh_1521`.

## Correction: the base VolumetricTerrain system is dormant in the retail world

Live process inspection (reading `/proc/<pid>/mem` while the game ran) proved that the toggles above
were written correctly — the type singleton at `0x142EED228` held the UI's values — and that both
detours were installed, yet neither `CRenderBlockTerrain::HullClipType` nor `TerrainPatchUpdate`
was ever called. **The base "VolumetricTerrain" system never draws in the retail world**; every
base-terrain lever ever tested (this session's and the earlier ones) was a no-op because the system
it targets is inactive. The visible terrain — tops and walls — is `CRenderBlockTerrainPatch`, the
volumetric-patch system.

The mechanism analysis transfers to the live system, where it is untested:

- The live per-patch fragment is **`TerrainPatchSystemFragment` (`0x1410CAC50`)** — same
  visibility-bit submission, same frustum-id routing (0 = color, 12 = depth prepass), plus LOD-9
  quadrant sub-blocks for the near passes. (`0x1410C98A0` is the dormant base system's fragment;
  the fragment-name table pairs names and functions across rows, not within them.)
- The live LOD clip has **no callable seam**: the patch type's `Setup` (`0x14034BB30`) inlines the
  selection — near passes (56..=57) bind hull holder 0, other tessellating passes (58, 60) bind
  holder 2, the LOD clip — and `m_DisableLodClipping` is compiled out. The retargeted
  `force_terrain_lod_clip` therefore swaps hull holder 2 with holder 0 on the type object
  (auto-restored on toggle-off).
- The earlier `HullClipType`-forcing test from `8fa3521` targeted the dormant base type
  (`0x14032B450`), so the original "hull clip ruled out" result is void for the live system.
- The `terrain heartbeat (#40)` log line now counts both systems: `hull_calls`/`patch_calls`
  (dormant base) and `fragment_calls`/`setup_calls` (live patch system).

Retest matrix, now against the live system: `force_terrain_lod_clip` (holder swap) and
`force_terrain_patch_visibility` (now also hooked into `TerrainPatchSystemFragment`), with the
heartbeat confirming the hooks fire.

## Identified: the wall surface is `TerrainDetail`

The registry bisect (the new "Render block types" debug panel — the engine's `CRenderBlockFactory`
vector behind the pointer at `0x142ED0F60`; retail stubs `Enable`/`Disable`/`IsEnabled` to
constants, so the panel patches each type's `IsEnabled` vtable slot to a return-false stub) found
the owner: **disabling `TerrainDetail` (`CTerrainRenderBlockDetail`) removes the entire cliff rock
skin** — the surface the black tiles live on. The volumetric-patch debug modes recolor only the
terrain *top* skin, confirming the split.

What is known about the detail system so far (dump): per-tile blocks with
`m_DetailPatchIndex`/`m_DetailLod`; `m_Mesh` points at the patch system's `STerrainPatchMesh`, so
the detail geometry is the GPU-built quad output (`CRenderBlockTerrainSetup`); each patch owns an
`SDetailPatchData` of 5×{`CRenderBlockTerrainDetailSetup`, high-detail, detail} sub-blocks, which
`TerrainPatchSystemFragment` submits for LOD-9 patches in the camera's quadrant — gated by
*position*, not visibility bits, which is why the visibility force could not affect the black.

Next RE target: the GPU quad-selection pipeline feeding these blocks — the
`volumetricterrainenumeratetexels` / `calculatetriangles` / `calculatehistogram` /
`calculatemorphtarget` compute passes and the detail block's own `Draw`/`DrawZ`
(`CDetailRBType::Setup` is at `0x140349C40`). A view-dependent texel enumeration that assumes the
flatscreen FOV would produce exactly the observed eye-identical, tile-quantized, angle-dependent
dropout that no submission-side lever can fix.

## The detail-quad selection pipeline (decoded from the compute shaders)

`CTerrainRenderBlockDetail::Draw` (`0x140326050`) is a `DrawIndexedInstancedIndirect` from a
per-tile 16-byte args slot (indexed by `m_DetailPatchIndex`/`m_DetailLod`) in a buffer owned by the
terrain setup type (`0x142EED240 +104`). The args are written by GPU compute: the
`CRenderBlockTerrainDetailSetup` blocks dispatch three indirect compute stages in the terrain setup
passes (render-pass ids 5/6/7 — `Setup`/`SetupDetail`/`Enumeration`, all pointed at
`m_TerrainCamera` by `TerrainPatchSystemUpdate`), and the compute shaders (`sh_1434`–`sh_1440` in
`Shaders_F`: `patch_meta_data`/`quad_meta_data`/`QuadGroup`/`DetailDrawList`/`DetailDispatchList`)
build the per-tile draw lists.

The draw-list builder (`sh_1434`) selects each quad with, per 6 frustum planes from **global
constant slots 38..=43** (uploaded from the pass frustum camera — the terrain camera — by
`SetGlobalShaderProgramCameraConstants`): keep when `dot(n, center) + d < radius` for all six
(outward normals; inside is negative), AND the quad is within `radius+64` (high-detail bucket) or
`radius+128` (detail bucket) world units of the camera position (slot 4), AND not deeper than 80
units below sea level while the camera is above water. So the detail rock skin exists only within
~64–128 m of the head, and **the only view-direction-dependent input is the terrain camera's
frustum planes** — the planes `widen_terrain_cull` rebuilds.

New discriminator: `terrain_cull_accept_all` (config + "Culling & geometry" checkbox) stamps the
terrain camera's planes with zero normals and a hugely *negative* distance each frame (the engine's
plane convention is outward/negative-inside), making every plane test pass. If the black fills in
with it on, the quad cull is the discard stage and the union widen is somehow not covering what the
eyes see (sign, timing, or coverage); comparing the accept-all result against `widen_terrain_cull`
on/off then isolates the widen's defect.

## Root cause (near-certain): detail-budget exhaustion

The accept-all test *inverted*: a ~170° terrain camera made the black dramatically worse, a
narrower one made it better. The frustum planes do not discard the black tiles — they *admit* quads
into fixed-size GPU buffers. The draw-list builder's tail (`sh_1434`) bumps global vertex/index/
texel cursors (`GlobalData` +24/+28/+32) with unbounded `imm_atomic_iadd` and no capacity check;
when the admitted quad set exceeds the buffers, the overflow writes go out of bounds and D3D11
silently drops them — the losing tiles end up with empty draw args and render black.

The budgets are the buffer sizes in the setup types' `Create` functions, sized for the flatscreen
FOV: detail vertex `0x10000`×16 B, debug vertex `0x10000`×80 B, index `0x40000` B, texel
`0x8000`×16 B (`CRenderBlockTerrainDetailSetup::Create`, `0x14032C000`), and the 4-byte index view
`0x8000` (`CRenderBlockTerrainSetup::Create`, `0x14032EA10`). VR's wide FOV admits roughly 2–3× the
quads within the 64/128 m detail radii and oversubscribes them. This explains every symptom:
mid-view victims (allocation order, not screen position), discrete angle-dependent flips (the
admitted set shifts with view), eye-identical (one shared build per frame), the widen doing nothing
(already overflowing either way), 170° being catastrophic, and flatscreen at default FOV fitting
just under budget.

**Fix (built, awaiting in-headset confirmation):** `terrain_detail_budget_scale` (default 4) with a
debug-UI "Apply" button — patches the five buffer-size immediates
(`TerrainDetailBudgetPatchSites` in the pyxis defs) to `shipped × scale` through the patcher
(auto-reverting on uninject) and re-creates the two setup types via their own `Recreate` with the
render engine's resource context (`RenderEngine::m_ResourceContext`, `+0x18D0`) — the engine's own
settings-change path — processed at frame start alongside the shader-reload request.

## Outcome

**Confirmed in-headset: scaling the detail budget fixes the black tiles** (even 2× sufficed in the
test scene; the shipping default is 4× for headroom, ~20 MB of VRAM). The shipping change is the
budget scale (`terrain_detail_budget_scale`, applied automatically at the first frame start after
injection and re-appliable from the debug UI) plus the render-block-type registry bisect panel that
found the owning block. The full diagnostic state used along the way — the LOD-clip/visibility/
terrain-cull forces, the heartbeat instrumentation, and the base-terrain probes — is preserved on
the `issue-40-investigation` branch.
