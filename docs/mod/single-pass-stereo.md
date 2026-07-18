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
   DXBC checksum (`refresh_dxbc_checksum` in `hooks/graphics_engine/shader.rs` already exists).

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

## Risk ranking

1. Baked-WVP per-type constant buffers (~105 VS; touches CPU code of ~12 render-block types).
2. DXBC-rewriter correctness (mechanizable, but the novel infrastructure).
3. GPU-indirect draws (can stay double-drawn initially).
4. Seam safety of screen-space passes on a double-wide target (clampable).
5. Occlusion queries / exposure histograms over a double-wide target (conservative, acceptable).
