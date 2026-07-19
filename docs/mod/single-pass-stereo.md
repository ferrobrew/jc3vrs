# Single-pass stereo (design + phased plan)

The mod currently renders the scene twice per frame — one full CPU walk of the render-pass list
per eye. Profiling showed the frame is **draw-call-submission bound**, not GPU bound: ~20k draw
calls/frame (~9,900 per eye, doubled), GPU only ~41% utilised (DXVK/nvidia-smi), 0 queue syncs.
The prize is collapsing the two geometry walks into one so the GPU is fed from a single submission.

This documents the design that a three-thread feasibility investigation converged on, and the
phased build. See `docs/mod/profiler.md` for how the bottleneck was measured.

## The technique

**Instance-doubled, double-wide, viewport-routed single-pass** — for the geometry (G-buffer) half
of the frame only; lighting and post stay per-eye.

- **Routing** (confirmed supported by DXVK on NVIDIA): a vertex shader writes
  `SV_ViewportArrayIndex` directly — the D3D11.3 `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer`
  capability, which DXVK reports and compiles (gated on the Vulkan `shaderOutputViewportIndex`
  feature). No geometry shader needed. Probe the capability at runtime; fall back to double-draw if
  absent.
- **Target topology: double-wide, not a texture array.** A 2-slice array would force ~300 pixel
  shaders that sample the scene targets as `Texture2D` to be retyped to `Texture2DArray`. A
  double-wide render target needs none of that — the engine's own `CreateRenderSetups` re-runs with
  a doubled width, `SetRenderSetup` synthesises the viewport from the target size, and SRV types
  never change. Each eye renders into its half via two viewports selected by the viewport index.
- **Instance doubling**: geometry draws issue `2×` instances; the VS reads `eye = SV_InstanceID & 1`,
  picks that eye's view-projection, and routes to its half. Shaders that index per-instance data by
  `SV_InstanceID` are rewritten to `SV_InstanceID >> 1`.
- **Scope**: only the G-buffer pass range (`RP_Z_OCCLUDERS`..`RP_LAST_GBUFFER`, 0x2F..0x55) goes
  single-pass. Lighting (SSAO, SSR, GI, deferred lights) and post stay as two viewport-scoped runs
  over the shared double-wide target — cheap (a handful of fullscreen draws, not the 20k geometry
  draws), and it sidesteps the deferred-unprojection minefield that sank every prior stereo
  retrofit. Single-pass also *removes* the current double-dispatch hacks (EffectInfo double-aging,
  dt accumulators, exposure gating).

Why this is tractable when nobody has retrofitted true single-pass onto a deferred game before: the
blocker elsewhere is per-shader bespoke position math. JC3 has **one** scene view-projection —
`RenderContext::m_OffsetViewProjection`, uploaded to the global VS constant buffer (`cb0`, rows
29–32, with the camera position at row 4). A census of all 455 vertex shaders: ~215 read it from a
byte-uniform location (one/two mechanical rewrites), ~105 consume a CPU-baked copy (per-type CPU
hooks or leave double-drawn), the rest have no position output. And we render correct stereo *today*
via double-draw, so every single-pass patch has a per-pass correctness oracle to diff against.

## The vertex-shader transform

The per-eye data in the position path is exactly five `cb0` rows: `cb0[4]` (camera position) and
`cb0[29..32]` (the view-projection). The transform binds a mod-owned constant buffer at the free
slot **cb13** holding *both* eyes' five rows laid out as `[eye0: 0..4][eye1: 5..9]`, and rewrites the
shader to index it per eye:

1. Add an `SV_InstanceID` input (new `v` register + ISGN entry).
2. Add an `SV_ViewportArrayIndex` output (new `o` register + OSGN entry).
3. Declare `cb13` (five... ten float4 rows) and bump `dcl_temps` by one for the eye register.
4. Prologue: `and rEye.x, vInstanceID.x, l(1)`; `imul null, rBase.x, rEye.x, l(5)`;
   `mov oViewport.x, rEye.x`.
5. Rewrite every operand referencing `cb0[4]` and `cb0[29..32]` to the relative-indexed
   `cb13[rBase.x + k]` (k = 4 for the camera row, `n-29` for the VP rows). Because the eye base is a
   register-relative index, this is a *uniform* operand remap — the same edit for all 155 shaders of
   the model idiom.
6. Rewrite `SV_InstanceID` consumers (if any) to `>> 1`.
7. Fix the SHEX instruction/token counts, the ISGN/OSGN chunks, the container total size, and the
   DXBC checksum (`dxbc_stereo::refresh_checksum`, shared with the fragment-program patching in
   `hooks/graphics_engine/shader.rs`).

The patch is applied in-flight in a `CreateVertexProgram` hook (release `0x141953320`), symmetric to
the existing `CreateFragmentProgram` patching, before the underlying `CreateVertexShader` copies the
bytecode.

## Reference structures (what fxc emits, so the transform can reproduce them)

Compiling a reference VS that writes `SV_ViewportArrayIndex` (through a `D3DCompile` harness under
wine) settles the encodings the transform must produce. Key findings:

- **A vs_5_0 writing `SV_ViewportArrayIndex` compiles** — no need for vs_5_1. But fxc adds an
  **`SFI0` (shader-feature-info) chunk**, an 8-byte body with **bit 13 (`0x2000`)** set:
  `D3D_SHADER_REQUIRES_VIEWPORT_AND_RT_ARRAY_INDEX_FROM_ANY_SHADER_FEEDING_RASTERIZER`. The transform
  must add this chunk (or OR the bit into an existing `SFI0`), else the viewport output is invalid.
  Chunk order is `RDEF, ISGN, OSGN, SHEX, SFI0, STAT`.
- **ISGN** gains `SV_InstanceID` (`sysvalue = 8` INSTANCE_ID, a `uint` component type, at the next
  free input register, mask `.x`). **OSGN** gains `SV_ViewportArrayIndex` (`sysvalue = 5`, `uint`, at
  the next free output register, mask `.x`, never-read-mask `.yzw`).
- **`cb13`** is declared **dynamically indexed** (`dcl_constantbuffer CB13[10], dynamicIndexed`),
  because it is indexed by a register (`rBase.x + k`) rather than an immediate — unlike the game's
  `cb0` (`immediateIndexed`).
- Reflection (`RDEF`) need not be updated: DXVK binds and compiles from the `SHEX` declarations, not
  `RDEF`; and the `STAT` chunk is ignored, so instruction-count bookkeeping is unnecessary — only the
  `SHEX` length dword is fixed.

## Phased build

- **Phase 0 (the spike)**: build the DXBC-assembler skeleton; patch only the ~10-shader `RBIInfo`
  model family; double-wide `MainDepth` + the G-buffers via a re-run `CreateRenderSetups`;
  instance-double at `DrawIndexed`/`DrawIndexedInstanced`; both eyes' rows in a mod-owned cb13;
  everything else double-drawn. Validates *every* novel mechanism (assembler, routing, double-wide
  RT, instance doubling, dual-matrix upload) on the smallest surface, diffed against the double-draw
  oracle.
- **Phase 1**: extend to the full G-buffer range (0x2F..0x55), including terrain's cb2 OffsetVP copy;
  keep lighting-onward per-eye (run the tail twice, viewport-scoped).
- **Phase 2**: velocity/decals; seam clamps in SSAO/SSR (they sample neighbourhoods and will leak
  across the double-wide seam without a UV clamp).
- **Phase 3**: GPU-indirect draws (terrain tessellation / vegetation — instance count lives in a GPU
  buffer, so doubling needs a compute pre-pass) and the baked-WVP straggler shaders, or accept those
  as permanently double-drawn.

## Payload integration — architecture, hook points, and status

This section is the concrete build spec for the in-game side, synthesised from the engine RE in
`docs/engine/rendering.md` (frame pipeline §1–13) and `docs/engine/render-setups-reinit.md`
(double-wide target). Release addresses are this build's RVAs; the layouts are byte-stable.

### The uniform-position path, confirmed against the corpus

The whole premise — JC3 has one scene view-projection in a byte-uniform `cb0` location — now has an
offline proof. `dxbc-stereo`'s `corpus_patch` test runs `patch_vertex_shader` over all 455 extracted
vertex shaders and structurally validates every success. Result:

| Outcome | Count | Disposition |
|---|---|---|
| Patched (per-eye `cb0[{4,29..32}]` remapped to `cb13`) | **196** | single-pass |
| No per-eye references (baked-WVP / no-position / cb2-terrain) | 245 | double-drawn |
| Already declares `SV_InstanceID` (needs the `>> 1` consumer rewrite) | 14 | double-drawn (deferred) |
| Errored / structurally invalid | **0** | — |

So 196 of 455 VS patch cleanly today, and every patch is structurally sound (re-parses, `SFI0`
viewport bit set, no residual per-eye operand). The rewriter is validated corpus-wide *before* any of
it reaches the game, at four levels:

1. the single-shader unit test (`patch.rs`) checks the exact injected encodings against fxc;
2. `corpus_patch` structurally validates all 196 successes (0 invalid);
3. all **196/196** patched blobs are accepted by real Microsoft `D3DDisassemble` under wine (via
   `scripts/dxbc.sh disasm`) — a valid-container proof from the actual D3D tooling, the closest
   offline proxy to DXVK's `CreateVertexShader` accepting them;
4. spot-checking a patched game shader's disassembly (`sh_0067`) shows the exact expected idiom —
   `dcl_constantbuffer CB13[10], dynamicIndexed`, `dcl_input_sgv instance_id`,
   `dcl_output_siv viewport_array_index`, the `eye = id & 1` / `rBase = eye*5` prologue, and every
   `cb0[{4,29..32}]` remapped to `cb13[rBase + k]` while the camera-relative base (`cb12[3]`) is left
   untouched.

### Hook points (all in `jc3gi`, addresses bound)

| Purpose | Function | Release addr |
|---|---|---|
| Patch VS bytecode in-flight | `Graphics::CreateVertexProgram(device, *const CreateVertexProgramParams)` | `0x141953320` |
| VS global constants staging (`cb0` rows) | `RenderEngine::SetGlobalShaderProgramCameraConstants` | `0x140186370` |
| VS global constants GPU upload | `RenderEngine::SetAllGlobalShaderProgramConstants` | `0x140173850` |
| Rebuild scene RTs at a new size | `GraphicsEngine::CreateRenderSetups(this, *const DeviceInfo)` | `0x1400CE930` |
| Runtime resize driver (reuse, swapchain-neutralised) | `GraphicsEngine::ApplyResize(this, w, h)` | `0x1400CFA90` |
| Per-pass viewport bind (viewport follows RT size) | `Graphics::SetRenderSetup` | `0x141966D20` |

`CreateVertexProgramParams` = `{ m_Code: *const u8 @0, m_Size: u64 @8, m_Name: *const u8 @0x10 }`.
The `cb0` VS staging block is `RenderEngine::m_VPGlobalConstData: [Vector4; 49]` (row 4 = camera
world pos, rows 29–32 = the translation-free OffsetViewProjection). `cb13` is free across the game's
VS (they use `cb0`, `cb2`, `cb12`).

### The pipeline (behind `config.stereo.single_pass`, off by default)

1. **VS patch** — a `CreateVertexProgram` detour (`hooks/graphics_engine/shader.rs`, mirrors the
   fragment hook) runs `patch_vertex_shader` and substitutes the patched bytecode. *(Built:
   census/observation only. Substitution + the patched-VS set is the next step.)*
2. **Bound-VS gating for the draw** — to double instances only for patched draws, the draw layer
   must know whether the currently-bound VS is one we patched. Cleanest seam: COM vtable detours on
   the immediate context (`VSSetShader` to cache the bound VS; `DrawIndexed`/`DrawIndexedInstanced`
   to promote to 2 instances) plus recording the patched `ID3D11VertexShader*`. re-utilities supports
   runtime-address detours (`with_detour<F: retour::Function>` / `with_runtime_binder`), so the DXVK
   method pointers (read from a live context's vtable) are hookable. *(Unbuilt.)*
3. **cb13 dual-eye upload** — compute both eyes' five rows (OffsetVP + camera pos) and upload to a
   mod-owned `cb13`, laid out `[eye0: 0..4][eye1: 5..9]`, bound via `VSSetConstantBuffers(13, …)` at
   the start of the G-buffer pass range. The per-eye matrices already exist
   (`vr::frame::EyeRenderParams`, one set per eye). *(Unbuilt.)*
4. **Double-wide render setups** — extend the mod's existing per-eye `CreateRenderSetups` re-init to
   2× per-eye width. The whole scene RT set goes double-wide; viewports follow RT size automatically
   (`SetRenderSetup`), so no per-pass viewport patching. *(Unbuilt.)*
5. **Two-viewport routing** — after the G-buffer RT bind, set two viewports (left/right halves) so
   `SV_ViewportArrayIndex` routes each eye. *(Unbuilt.)*
6. **Capability gate** — `stereo::single_pass::probe` checks
   `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer`; single-pass stays inert without it. *(Built.)*

### Wake-up test checklist (ranked, safest first)

1. **Census (safe, no rendering change).** Inject; open Render tab → "Single-pass stereo"; enable
   "Census only"; click "Reload shaders". Expect roughly **196 patched / 259 double-drawn**
   (245 no-refs + 14 instance-id) and the viewport-routing capability = supported. Non-zero "errored"
   would flag a shader the offline corpus didn't cover (bundle mismatch) — capture its name.
2. **Capability probe.** Confirm the reported capability matches the headset/runtime in use this
   session (Monado/DXVK on the Index expected to support it; confirm, don't assume).
3. *(Once the pipeline lands)* **Single-pass A/B.** Enable the master switch; compare against the
   double-draw oracle (toggle off) for the model-family geometry. Likely first failure modes, in
   order: right-eye half empty (viewport routing / instance doubling not firing), geometry in wrong
   half (viewport index inverted), stale/garbage positions (cb13 layout or upload), unpatched
   geometry missing from one eye (draw-doubling gate).

### Deferred (documented, deliberately not built blind)

- The `SV_InstanceID >> 1` consumer rewrite for the 14 already-instanced shaders (Phase 1+): the
  DXBC transform is offline-testable, but the per-instance *semantics* are not, so it waits until
  Phase 0 validates the core.
- Terrain's `cb2` OffsetVP remap (Phase 1), the SSAO/SSR seam clamps (Phase 2), and the baked-WVP
  CPU dual-upload + GPU-indirect compute pre-pass (Phase 3) — none are offline-verifiable, so they
  stay specified here rather than speculatively built.

## Milestone status (in-game bring-up)

**Milestone A — VALIDATED in-game.** Substitute the patched VS + mirror the current view into a
mod-owned `cb13`, no double-wide/instancing/collapse yet: renders identically to the double-draw
across the whole frame (scene + shadows). Proved the rewriter produces DXVK-accepted shaders, the
`cb13` upload/layout, the shadow-safe view tracking, and the viewport-routing infra. Bugs shaken out
along the way: the `m_Size` truncation (substituted blob is larger — must repoint length, not just
the pointer), and viewport routing for odd-`SV_InstanceID` primitives (fixed by an `RSSetViewports`
COM-vtable detour that mirrors the bound viewport into slot 1, catching the shadow cascades' raw
viewport sets that `SetRenderSetup` misses).

**Milestone B — dual-eye machinery, collapse, and double-wide all BUILT (compile-clean, off by
default); in-headset bring-up in progress.** Gated behind `stereo.single_pass_dual_eye`:
- `cb13` filled with **distinct** per-eye view-projections, computed in mod code from the pristine
  center transform + per-eye `EyeRenderParams` (replicating the double-draw camera math);
- the `RSSetViewports` detour splits the bound viewport into **left/right halves** for the eye
  routing (instead of two identical copies);
- a `DrawIndexed` COM-vtable detour **promotes non-instanced draws to 2 instances** so
  `SV_InstanceID & 1` selects the eye.

Testable as a **diagnostic** (no collapse/double-wide): enabling `single_pass_dual_eye` makes
each eye show a squished side-by-side of *both* eye viewpoints — if the two halves show visibly
different viewpoints and nothing crashes, the dual-eye `cb13` + instancing + eye-half routing all
work.

- **Collapse to a single walk (`single_pass_collapse`) — BUILT.** When `collapse_active()`,
  `hooks/game.rs` `game_update_render` runs one `game.Draw` (dispatch list `&[(0, false)]`) instead of
  the per-eye loop, so the between-eye snapshot/restore (gated on `ordinal > 0`) is skipped for free;
  `hooks/camera.rs` `setup_render_camera` keeps the render camera centered (no per-eye world offset —
  both eyes come from `cb13`, and the shadow-anchor delta is zeroed to match); and
  `hooks/graphics_engine/graphics_engine.rs` `render_engine_post_draw` splits the one back buffer into
  the two eye textures (`CopySubresourceRegion` of each eye-half). This is the actual draw-submission
  win and the riskiest change (in-game iteration, like Milestone A). It requires only
  `single_pass_dual_eye`, not double-wide: without double-wide each eye-half is squished and fills only
  the left portion of its eye texture, and the HUD reaches the left eye only — a bring-up state, not
  the finished look.

  **Eye routing spans the whole camera scene, and the viewport split moves to draw time.** The
  double-draw ran the *entire* frame per eye, so the mod only had to route the G-buffer geometry range
  (`RP_Z_OCCLUDERS..RP_FIRST_SCENE`) — everything after (deferred lighting, water, sky, transparents,
  post) got a second full dispatch. One walk has no second dispatch, so the later *geometry* passes
  (water planes at `RP_REFLECTIVE_WATER_PLANES`, sky `RP_STARS..RP_FOG_GRADIENT`, transparents) must
  route to the eyes too, while the *fullscreen* deferred-lighting/post passes interleaved with them
  must keep the full width (they light/resolve the whole double-wide target in one pass). A single
  contiguous pass range cannot separate the two, so under collapse: (a) `render_pass.rs` marks the
  whole camera scene as routed (`first >= RP_Z_OCCLUDERS`, which excludes the shadow/reflection
  prepasses at `<= RP_LAST_PREPASS` that reuse the same shaders under the sun/reflection view); and
  (b) the viewport split moves from pass-level (`rs_set_viewports_detour`, which now only records the
  full viewport under collapse) to **draw-level** (`ensure_collapse_viewport`), re-binding only on a
  transition: `draw_indexed_detour` splits into the L/R halves for a **patched** geometry draw (and
  leaves unpatched `DrawIndexed` geometry on whatever viewport is bound), while `draw_detour` — the
  non-indexed `Draw` (vtable slot 13), which is how the **fullscreen** deferred-lighting/post passes
  render — resets to the **full** width. Without the `Draw` reset the lighting pass would inherit the
  eye-half the previous geometry draw left bound and light only one eye, leaving the other eye's
  opaque geometry black. (Particle `DrawInstanced` and already-instanced `DrawIndexedInstanced` scene
  draws are not yet routed.)
- **Double-wide render target (`single_pass_double_wide`) — BUILT.** Requires `single_pass_collapse`
  and `vr.native_resolution`. `vr::engine_render_resolution` returns **2× the per-eye width**, which
  the existing deferred-`ApplyResize` native-resolution driver (`vr::resolution`) targets — so the
  whole scene RT set (back buffer, `m_BackBufferLinear`, the G-buffers) is re-created double-wide, and
  the per-pass viewport follows the RT size automatically. The **XR swapchain stays per-eye width**
  (`native_eye_resolution` is unchanged), and the per-eye **capture textures** are sized to half the
  back buffer (`ui::render`), so the collapse's capture split copies each full-width half straight into
  its eye texture (`half_w` == per-eye width == eye-texture width — no squish). Caveats: the engine's
  own `cb0` projection/aspect is now 2×-wide, so **unpatched** geometry (which reads `cb0`, not the
  per-eye `cb13`) renders horizontally squashed; and screen-space/post passes (FSR, SSAO, SSR) run once
  over the double-wide target and will leak across the eye seam until clamped (Phase 2).

Bring-up order on wake: `single_pass` → Reload shaders (Milestone A, should look identical) →
`single_pass_dual_eye` (diagnostic squished double-image) → `single_pass_collapse` (single walk,
squished but no double-vision, both eyes distinct) → `single_pass_double_wide` (clean full-res
single-pass stereo).

## Risk ranking

1. Baked-WVP per-type constant buffers (~105 VS; touches CPU code of ~12 render-block types).
2. DXBC-rewriter correctness (mechanizable, but the novel infrastructure).
3. GPU-indirect draws (can stay double-drawn initially).
4. Seam safety of screen-space passes on a double-wide target (clampable).
5. Occlusion queries / exposure histograms over a double-wide target (conservative, acceptable).

## Stragglers: the non-`cb0` families

The `cb0→cb13` remap and the collapse handle the ~196 model shaders. Five parallel RE passes (corpus
disassembly plus the release IDB) mapped what's left and how to single-pass it properly instead of
double-drawing it.

First, a distinction the census makes but the in-headset image blurs: **a shader being patched is not
the same as that geometry working per-eye.** The rewriter patches every shader that reads
`cb0[29..32]`, so the per-eye math is there — but the draw still has to reach the collapse's
instance-doubling and viewport split. Buildings clear both bars. NPCs clear the first and fail the
second (see below).

### Shader-ready, and confirmed working

- **Buildings, general, and masked models** read `cb0[29..32]` and draw through the plain
  `DrawIndexed` path, so they're doubled and routed — correct in both eyes today. (The 2016 debug
  decomp shows them baking a WVP, but the shipped release build doesn't call
  `CalculateOffsetWorldViewProjectionMatrix` for them — verified by xrefs.)
- **Scene depth and velocity** (passes 48–50) use the models' depth-only permutations, same remap.
  This is the depth the lighting, SSAO, SSR, and FSR read per eye, so it has to be right, and it is.
- **Z-occluders** (pass 47) prime depth for culling; the real Z pass overwrites them, so per-eye
  correctness doesn't matter. Leave them.

### Needs a new mechanism

| Family | How position is computed | Plan |
|---|---|---|
| **Skinned characters and creatures** (`RenderBlockCharacter`/`CharacterSkin`, `RP_CREATURES`) — the NPCs | `o0 = skinnedPos·cb1[4..7]` — a CPU-baked `WorldViewProj` in `cb1` (`LocalConstants[219]`: `World` `cb1[0..3]`, `WorldViewProj` `cb1[4..7]`, `Scale` `cb1[8]`, `MatrixPalette[70]` `cb1[9..]`). **No `cb0` reference.** The WVP is baked inline in `CRenderBlockCharacterSkin::Draw`, *not* via `CalculateOffsetWorldViewProjectionMatrix` (characters don't call it) | reproject |
| **Baked-WVP** (~105 VS; visually Prop, Bark/tree-trunks, Window, RoadJunction, MaterialTune) | `o0 = pos·cb1[0..2] + cb1[3]` — the full WVP baked into `cb1` by `CRenderBlock::CalculateOffsetWorldViewProjectionMatrix` (release `0x140136070`, uploaded to `cb1`, 4 rows) | reproject |
| **Tessellated base terrain** (retail-live `CRenderBlockTerrainPatch`, `0x1403_2E540`; hulls `sh_1492`+, domains `sh_1513`+) | clip built in the domain shader from `cb1`'s `m_OffsetViewProjection` (byte-identical to `cb0`'s). **Hybrid by pass:** far/color/shadow are normal `DrawIndexed` (`0x1403_2E799`); near passes are GPU-indirect (`0x1403_2E747`) | reproject in the DS; ride the eye through the free `TEXCOORD3.z` lane VS→HS→DS (the HS control-point phase is a passthrough — one `mov`). Far/color/shadow single-pass now; near stays double-drawn until the indirect pre-pass |
| **GPU-indirect vegetation and detail terrain** (`CRenderBlockFoliage`/`Bark`; `sh_0224`, `sh_0253`, …) | `SV_VertexID` + structured buffers; baked patch-local→clip in a small per-pass cb; drawn via `DrawIndexedInstancedIndirect` (5-dword args, `InstanceCount` at dword +1, in the GPU-only `veg_draw_indirect` buffer from `m_GenDrawIndirectParamsPerPassCS`) | reproject VS + an **in-place compute pre-pass** doubling each slot's `InstanceCount` (the buffer has no CPU copy) |
| Effects, decals, water-mask, GI-probe (baked `cb1`, low visual weight) | baked WVP | leave double-drawn |
| Sky, atmosphere, UI, screen-space (write `o0` in NDC) | not a scene VP | exclude |

The character finding corrects an earlier misread: a *different* 2-bone skin shader (`cbSkinningConsts.MatrixPalette[2]`, RBIInfo path) does read `cb0`, which suggested characters were `cb0`-family and their breakage a draw-routing bug. The real 70-bone character skin is baked-WVP with no `cb0`, so NPCs need reprojection — not a routing fix.

### Reprojection

One identity handles the whole "needs a new mechanism" group. Every scene shader writes clip position
as `clip_center = VP_center · world`, whatever buffer the VP came from. So:

    clip_eye = VP_eye · world = (VP_eye · VP_center⁻¹) · clip_center = M_eye · clip_center

Post-multiply the shader's own `SV_Position` by a per-eye `M_eye` and it lands exactly in each eye.
The camera-relative idiom (`OffsetVP·(P − campos)` equals `FullVP·P`) folds in when `M_eye` is built
from the full, translation-carrying per-eye VPs — the IPD parallax rides along in `M_eye`, so the eye
offset never reaches the shader. Skinned characters are just the baked-WVP case: skinning is
local→world, which reprojection never sees.

What to exclude: shaders that write `o0` straight to NDC (sky, UI, screen-space). The bytecode can't
tell those from scene meshes, so the existing pass-range ∩ patched-VS ∩ `DrawIndexed` gate stays the
authority for "this is scene geometry."

Two things to verify before trusting reprojection on the baked family:

- Whether JC3 bakes a TAA projection jitter into `m_OffsetViewProjection`. If it does, the baked-WVP's
  `VP_center` still matches — JC3 has one scene VP.
- `M_eye` is near-identity (a few-cm IPD, a few-degree cant), so error is ~1–2 ULP and reverse-Z depth
  survives. Invert `VP_center` and build `M_eye` in f64 on the CPU, store f32, and assert
  `‖M_eye − I‖` stays small — the reverse-Z `VP_center` is near-singular, so f64 earns its keep here.

### The DXBC recipe (implemented: `dxbc_stereo::reproject_vertex_shader`)

Keep the `cb0` remap for the 196 models; it's exact and in-game-proven. Reprojection is the sibling
rewrite for the no-`cb0` families. It reuses everything the remap builds — the `SV_InstanceID` input,
`SV_ViewportArrayIndex` output, `SFI0` bit 13, signature append, checksum, and the same
`and`/`mov oViewport`/`imul` prologue — and swaps only the core:

1. Find the `SV_Position` output register from `OSGN`/`dcl_output_siv position` (normally `o0`).
2. Rename every write to it to a fresh temp `rClip`, bumping `dcl_temps`. `SV_Position` is written on
   every path, so `rClip` ends fully defined, and the rename absorbs masked or multi-instruction
   writes (a separate `o0.z` clip-Z bias). SM5 output registers are write-only, so redirecting the
   writes is the only move. Renaming the write also leaves the shader's own source temp intact — the
   terrain DS reuses its center-clip temp for an LOD fade after writing position, and this preserves it.
3. Before each `ret`, emit `o0 = M_eye · rClip` as four `dp4`s. The prologue computes `rBase = 4*eye`
   and routes the viewport; each `dp4` reads an `M_eye` row as `cb13[rBase + (10 + j)]`.
4. `M_eye` rides `cb13` rows 10–17 (four rows per eye) after the 10 remap rows — `STEREO_REPROJ_CB_ROWS
   = 18`, same `b13` binding. `compute_dual_eye_rows` emits it (still to wire on the payload side).

Validated offline like the remap: the unit tests re-parse the output, assert `SV_Position` is written
only by the four `M_eye` `dp4`s, and check the interface/checksum; a corpus sweep reprojects **225 of
the 245 no-`cb0` VS** cleanly (the other 20 write no position — the terrain VS whose clip is built in
the DS); and real `D3DDisassemble` accepts the character shader with the exact expected idiom.

**What to reproject (the NDC-writer problem).** Reprojection is shader-agnostic, so it would also
transform sky/UI/fullscreen shaders that write `o0` in raw NDC — which `M_eye` corrupts — and the
bytecode can't reliably tell those from scene meshes. Substitution happens at shader creation, before
the draw's pass is known, so the runtime pass gate can't decide it. The payload wire-up therefore gates
reprojection to a **positive allowlist of scene-geometry families** (keyed on the shader name in
`CreateVertexProgramParams`, or a `cb1`-matrix fingerprint): unknown shaders stay double-drawn
(correct, just slower), and no NDC writer is ever reprojected.

### Order

1. **Baked-WVP, including the skinned characters (NPCs)** — biggest bucket, lowest risk. Reuses the
   draw-doubling; only the reprojection recipe and the `M_eye` upload are new. Fixes the NPCs.
2. **Tessellated terrain** — reproject in the domain shader and ride the eye through the free
   `TEXCOORD3.z` lane VS→HS→DS (the HS control-point phase is a passthrough, one `mov`); a DS-stage
   `SV_ViewportArrayIndex` write is legal under the capability. Single-passes the far/color/shadow
   passes (`DrawIndexed`); the near passes are GPU-indirect and wait for step 3.
3. **GPU-indirect vegetation and detail terrain** — the same reprojection VS plus an in-place compute
   pre-pass that doubles each indirect draw's `InstanceCount` (dword +1 of the 5-dword args, in the
   GPU-only `veg_draw_indirect` buffer). Last and hardest; fine double-drawn until then.

### The no-`cb0` shader census (reprojection allowlist)

The 245 no-`cb0` vertex shaders, from a census of the shipped set (`m_Name` against the rewrite
outcome — the payload can re-dump it by flipping `DUMP_VS_NAME_CENSUS`). **The census only sees
shaders an area actually loads, so this list may miss families from biomes/weather not visited; treat
it as a floor, not a complete set.** They sort by disposition:

**Reproject now — the scene-geometry allowlist** (`REPROJECT_NAME_PREFIXES` in `single_pass.rs`,
matched by name prefix): `character*`, `creature` (skinned + rigid NPCs, ~34 permutations incl.
`characterskin*`, `characterdepth*`, `characteroutline`, `charactersphdecal*`, `charactervelocity*`);
`general*` (`general`, `generalglint`, `generalmaskedjc3`, `generaloutline`, `generalprez`,
`generalprezvelocity`, `generalrsm`, `generalshadow` — the static/dynamic models); `prop`, `propdecal`;
`buildingjc3`, `buildingrsm`; `window`; `materialtune`; `open`; `flag`, `flagdepthonly`; `snow`;
`skidmarks`, `skidmarks_normal`; roads `junctionroad*`, `splineroad*`, `dirtroad`.

**Candidate scene models not yet on the allowlist** (unclear family, held back pending a look):
`box`, `notex`, `lrpc`.

**Terrain — Phase 2 (domain-shader reprojection, not the VS):** the VS writes no `SV_Position`, so
these auto-skip the VS reproject. `volumetricterrain*` (~30 permutations: `*4`, `*blend`, `*instanced`,
`*notessellation*`, `*offset*`, `*shadow*`), `terraindetailrt*`, `terrainscroller`,
`terrainshaderforest*`, `controlpoint`.

**Vegetation — Phase 3 (GPU-indirect):** `vegetationbark*`, `vegetationfoliage*`, `leaves`, `grass`,
`treeimpostor*`, `veginteractionvolume`, `vegintrecenter`, `vegintrecovery`.

**Excluded — NDC / non-scene** (`M_eye` would corrupt these; kept double-drawn):
- Sky/atmosphere: `skybox`, `skygradientshader`, `skymodelshader`, `atmosphereprecompute`,
  `atmosphericscattering`, `starsshader`, `softclouds`, `cirruscloudsshadow`, `foggradientshader`,
  `underwaterfoggradientshader`, `addfogvolume`, `fogvolumeapplyfs`, `fogvolumeblur`.
- Post / screen-space: `fxaa`, `temporalaafilter`, `testnoaa`, `motionblur`, `depthoffield`,
  `depthoffieldwithvpi`, `edgedetectionfilter`, `screenspace`, `screenspacetex*`,
  `screenspacesubsurfaceskinseparableblur`, `patternrecognitionfilter*`, `pixelreconstructionfilter`,
  `recreatepositionfromscreenspacedepth`, `ssao_sao`, `ssao_temporalfilter`,
  `deferred_clusteredlighting`, `lightassignmentfill`, `scenevisinit`, `gionly`,
  `skindiffuselightingcapture`.
- UI: `gui`, `quickui`, `2dtex1`, `2dtex2`, `3dtext`, `line3d`, `video`.
- Particles / effects / lights: `particleeffect*`, `beam`, `contrails`, `trail`, `billboard`,
  `meshparticle`, `distortionparticle`, `fxmeshfire`, `bulletsmoke`, `bullet`, `halo`, `halomask`,
  `lensflare`, `sunbeam`, `lightglow`, `lightsource`, `lightsource_fakelight`, `lightningshader`,
  `bavariumshield`, `spotlightcone`, `pointlightreflection`, `alphamask`, `aobox`.
- Water: `nvwater*`, `water*` (`waterboxclear`, `waterbumpcomposite`, `waterdisplacementoverride`,
  `waterfoamsub`, `watergodraysshader`, `watermask`, `waterpaintfoam`, `watersurface`, `waterwake`),
  `waves`, `mirror`.
- Decals (held back — surface-projected, want a look first): `decal`, `decaldeformable`, `decalsimple`,
  `decalskinned`, `decalskinnedgeneralmkiii`, `decalskinnedgeneralmkiiidestructible`, `ssdecal`,
  `ssdefault`.
- Occluders / probes / prez / shadow-only: `rainoccluder`, `rainoccluderblur`, `sphericalharmonicprobe`,
  `renderprez*`, `renderpreznotex`, `rendershadow`, `layeredrsm`, `depthwrite`, `tex0shadow`,
  `tex0shadownoalpha`.
