# Single-pass stereo

Double-draw stereo renders the scene twice per frame — one full CPU walk of the render-pass list per
eye. Profiling showed the frame is **draw-call-submission bound**, not GPU bound: ~20k draw calls per
frame (~9,900 per eye, doubled), GPU only ~41% utilised, no queue syncs. Single-pass collapses the two
geometry walks into one, so the GPU is fed from a single submission. See
[`profiler.md`](profiler.md) for how the bottleneck was measured, and
[`single-pass-render-blocks.md`](single-pass-render-blocks.md) for the per-render-block treatment.

Single-pass is opt-in and off by default. It is inert without the DXVK viewport-routing capability.

## The technique

**Instance-doubled, double-wide, viewport-routed** — for scene geometry; the fullscreen lighting and
post passes still run once over the whole target.

- **Routing.** A vertex shader writes `SV_ViewportArrayIndex` directly — the D3D11.3
  `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer` capability, which DXVK reports and compiles (gated
  on the Vulkan `shaderOutputViewportIndex` feature). No geometry shader involved.
  `stereo::single_pass::probe` checks it at runtime and the whole feature stays inert if it is absent.
- **Target topology: double-wide, not a texture array.** A two-slice array would force the ~300 pixel
  shaders that sample the scene targets as `Texture2D` to be retyped to `Texture2DArray`. A
  double-wide render target needs none of that — the engine's own `CreateRenderSetups` re-runs with a
  doubled width, `SetRenderSetup` synthesises the viewport from the target size, and SRV types never
  change. Each eye renders into its half, selected by the viewport index.
- **Instance doubling.** A patched geometry draw is issued with `2x` instances; the VS reads
  `eye = SV_InstanceID & 1`, picks that eye's view-projection out of `cb13`, and routes to its half.

### What the routed range covers

Under the collapse there is only one walk, so the range that routes to the eyes is the whole camera
scene: `first >= RP_Z_OCCLUDERS`, which excludes the shadow and reflection prepasses (`<=
RP_LAST_PREPASS`) that reuse the same shaders under the sun or reflection view. That has to include
the later *geometry* passes — water planes at `RP_REFLECTIVE_WATER_PLANES`, sky
`RP_STARS..RP_FOG_GRADIENT`, transparents — because there is no second dispatch to render them in.

A single contiguous pass range cannot separate those from the *fullscreen* deferred-lighting and post
passes interleaved with them, which must keep the full width. So the viewport split moves from pass
level to draw level: `rs_set_viewports_detour` only records the full viewport, and
`ensure_collapse_viewport` binds per draw — the L/R halves for a patched geometry draw, one eye's half
twice for an unpatched one (re-issued per eye), and the full width for the non-indexed `Draw` the
fullscreen passes use. Without that last reset the lighting pass would inherit the eye half the
previous geometry draw left bound and light one eye only, leaving the other eye's opaque geometry
black.

Without the collapse the routed range is just the G-buffer (`..RP_FIRST_SCENE`), which is a
diagnostic shape rather than a usable one.

#### The range is entered three times per dispatch

`DrawRenderPassRange` is not one call per frame. `CGraphicsEngine::HandleDrawThreadTask` calls it
three times, with bounds that are compile-time constants at each call site: `DrawGBuffer`
(`0x2F..0x55`), `Draw` (`0x56..0x96`), and `DrawPosteffects` (`0x96..0x97`). `PreDraw` — the shadow,
reflection, and vegetation prepasses — runs before all three and does not come through it. So without
the collapse exactly one call qualifies (the G-buffer), and with it all three do, which means three
`enter_gbuffer_range` / guard-drop pairs per dispatch.

Anything hung off a range boundary is therefore *not* per-frame under the collapse. The per-frame
diagnostics (the instanced eye-parity exposure fold) run from `single_pass::begin_frame` instead, and
the per-range draw-split line is tagged with the `[first, last)` window it covers.

#### The range flag belongs to the draw thread

The flag is raised, read, and lowered entirely on the draw thread, inside one
`HandleDrawThreadTask`. The safety clear that catches a range left open by an interrupted dispatch
must run there too — it does, in the `PreDraw` prologue (`single_pass::begin_dispatch`), the one point
in that thread's sequence where no range can be live.

It used to run on the game thread at the top of `CGame::UpdateRender`, which is safe only while every
dispatch is drained before `UpdateRender` returns. With `stereo.defer_frame_tail` on (the default) the
last dispatch is *not* drained: the draw thread keeps walking the previous frame's ~20k draws while
the game thread runs the next frame's sim and re-enters `UpdateRender`. The clear then lands in the
middle of a live range, and every draw after it is treated as out of range — no eye split, `cb13`
mirrored instead of per-eye, the per-eye reprojection matrices dropped. How far the draw thread got by
then varies frame to frame, so whole geometry families (instanced buildings, bark, foliage) blink in
and out. `stereo.single_pass_clear_range_on_dispatch` (on by default) picks the draw-thread clear; off
restores the old game-thread one for A/B. A guard that finds the flag already down when it drops
counts a "torn" range and warns, which is the direct measurement of this happening.

Why this is tractable at all: the blocker for retrofitting single-pass onto a deferred game is
normally per-shader bespoke position math. JC3 has **one** scene view-projection —
`RenderContext::m_OffsetViewProjection`, uploaded to the global VS constant buffer (`cb0`, rows 29–32,
camera position at row 4). And double-draw already renders correct stereo, so every single-pass change
has a per-pass correctness oracle to diff against.

## The vertex-shader transform

The per-eye data in the position path is exactly five `cb0` rows: `cb0[4]` (camera position) and
`cb0[29..32]` (the view-projection). The transform binds a mod-owned constant buffer at the free slot
**cb13** holding *both* eyes' five rows, laid out `[eye0: 0..4][eye1: 5..9]`, and rewrites the shader
to index it per eye:

1. Add an `SV_InstanceID` input (new `v` register + ISGN entry).
2. Add an `SV_ViewportArrayIndex` output (new `o` register + OSGN entry).
3. Declare `cb13` and bump `dcl_temps` by one for the eye register.
4. Prologue: `and rBase.x, vInstanceID.x, l(1)`; `mov oViewport.x, rBase.x`;
   `imul null, rBase.x, rBase.x, l(5)`. The viewport write reads the eye before the `imul`
   overwrites the temp with the row base, so one temp suffices.
5. Rewrite every operand referencing `cb0[4]` and `cb0[29..32]` to the register-relative
   `cb13[rBase.x + k]` (`k = 4` for the camera row, `n - 29` for the VP rows). Because the eye base is
   a register-relative index, this is a *uniform* operand remap — the same edit for every shader of
   the model idiom.
6. Fix the SHEX length dword, the ISGN/OSGN chunks, the container total size, and the DXBC checksum
   (`dxbc_stereo::refresh_checksum`, shared with the fragment-program patching in
   `hooks/graphics_engine/shader.rs`).

The new interface registers come from the **signatures**, not from a `dcl_input`/`dcl_output` scan:
fxc keeps an ISGN entry for an input the shader never reads (`ReadWriteMask = 0`) and emits no
declaration for it, so a declaration scan under-counts and the appended element collides with a
register that is already occupied.

The patch is applied in-flight in a `CreateVertexProgram` hook, symmetric to the existing
`CreateFragmentProgram` patching, before the underlying `CreateVertexShader` copies the bytecode.

## Reference structures (what fxc emits)

Compiling a reference VS that writes `SV_ViewportArrayIndex` (`scripts/dxbc.sh compile`, a
`D3DCompile` harness over the `dxbc-tool` crate) settles the encodings the transform reproduces. It is
committed as `dxbc-stereo/tests/data/ref_vs50.dxbc` and the unit tests diff against it byte for byte.

- **A vs_5_0 writing `SV_ViewportArrayIndex` compiles** — vs_5_1 is not needed. But fxc adds an
  **`SFI0` (shader-feature-info) chunk**, an 8-byte body with **bit 13 (`0x2000`)** set:
  `D3D_SHADER_REQUIRES_VIEWPORT_AND_RT_ARRAY_INDEX_FROM_ANY_SHADER_FEEDING_RASTERIZER`. The transform
  adds this chunk (or ORs the bit into an existing `SFI0`), else the viewport output is invalid.
  Chunk order is `RDEF, ISGN, OSGN, SHEX, SFI0, STAT`.
- **ISGN** gains `SV_InstanceID` (`sysvalue = 8` INSTANCE_ID, `uint` component type, mask `.x`).
  **OSGN** gains `SV_ViewportArrayIndex` (`sysvalue = 5`, `uint`, mask `.x`, never-written mask
  `.yzw`).
- **`cb13`** is declared **dynamically indexed** (`dcl_constantbuffer CB13[10], dynamicIndexed`),
  because it is indexed by a register (`rBase.x + k`) rather than an immediate — unlike the game's
  `cb0` (`immediateIndexed`).
- Reflection (`RDEF`) is not updated: DXVK binds and compiles from the `SHEX` declarations, not
  `RDEF`, and the `STAT` chunk is ignored, so instruction-count bookkeeping is unnecessary — only the
  `SHEX` length dword is fixed.

## The corpus, and how the rewriter is validated

`dxbc-stereo`'s tests run the rewrites over all 455 extracted vertex shaders. The `cb0` remap:

| Outcome | Count | Disposition |
|---|---|---|
| Patched (per-eye `cb0[{4,29..32}]` remapped to `cb13`) | 196 | single-pass |
| No per-eye references (baked-WVP / no-position / cb2-terrain) | 245 | reprojected, or double-drawn |
| Already declares `SV_InstanceID` | 14 | double-drawn (the `>> 1` consumer rewrite is not built) |
| Errored / structurally invalid | 0 | — |

Validation runs at four levels:

1. the single-shader unit tests (`patch.rs`, `reproject.rs`, `terrain.rs`) check the exact injected
   encodings against the fxc reference;
2. `corpus_patch_is_sound` structurally validates every success — the output re-parses, carries the
   `SFI0` viewport bit, has no residual per-eye `cb0` operand, and its checksum is self-consistent;
3. `corpus_patch_signatures_have_no_register_collision` and `corpus_patch_body_and_signature_agree`
   re-read the rewritten signatures with an independent parser, so the signature is both internally
   consistent and consistent with the body it describes;
4. the patched blobs are accepted by real Microsoft `D3DDisassemble` under wine (via
   `scripts/dxbc.sh disasm`) — the closest offline proxy to DXVK's `CreateVertexShader` accepting
   them.

The extract is game-derived and git-ignored. A missing extract **fails** the corpus tests; set
`JC3VRS_ALLOW_MISSING_SHADER_CORPUS=1` to skip them instead.

## Reprojection: the non-`cb0` families

The `cb0` remap covers the model shaders. Everything else writes clip from a CPU-baked copy of the
same view-projection, and one identity handles the whole group. Every scene shader writes
`clip_center = VP_center · world`, whatever buffer the VP came from, so:

    clip_eye = VP_eye · world = (VP_eye · VP_center⁻¹) · clip_center = M_eye · clip_center

Post-multiplying the shader's own `SV_Position` by a per-eye `M_eye` lands it exactly in each eye. The
camera-relative idiom (`OffsetVP·(P − campos)` equals `FullVP·P`) folds in when `M_eye` is built from
the full, translation-carrying per-eye VPs — the IPD parallax rides along inside `M_eye`, so the eye
offset never reaches the shader. Skinned characters are just the baked-WVP case: skinning is
local→world, which reprojection never sees.

`M_eye` is near-identity (a few-cm IPD, a few-degree cant), so the error is a couple of ULP and
reverse-Z depth survives. `VP_center` is inverted in f64 on the CPU — the reverse-Z VP is
near-singular — and stored as f32.

### The DXBC recipe (`dxbc_stereo::reproject_vertex_shader`)

Reprojection reuses everything the remap builds — the `SV_InstanceID` input, `SV_ViewportArrayIndex`
output, `SFI0` bit, signature append, checksum, and the same prologue — and swaps only the core:

1. Find the `SV_Position` output register from `OSGN`/`dcl_output_siv position` (normally `o0`).
2. Rename every write to it to a fresh temp `rClip`, bumping `dcl_temps`. `SV_Position` is written on
   every path, so `rClip` ends fully defined, and the rename absorbs masked or multi-instruction
   writes (a separate `o0.z` clip-Z bias). SM5 output registers are write-only, so redirecting the
   writes is the only move. Renaming the write also leaves the shader's own source temp intact — the
   terrain DS reuses its centre-clip temp for an LOD fade after writing position.
3. Before each `ret`, emit `o0 = M_eye · rClip` as four `dp4`s. Each `dp4` reads an `M_eye` row as
   `cb13[rBase + (10 + j)]`, with `rBase = 4 * eye`.
4. `M_eye` rides `cb13` rows 10–17 (four rows per eye) after the 10 remap rows
   (`STEREO_REPROJ_CB_ROWS = 18`), on the same `b13` binding.

A shader with a `retc` is rejected: a conditional early return would need the epilogue on that path
too.

### The NDC-writer problem

Reprojection is shader-agnostic, so it would equally transform sky, UI and fullscreen shaders that
write `o0` in raw NDC — which `M_eye` corrupts — and the bytecode cannot reliably tell those from
scene meshes. Substitution happens at shader creation, before the draw's pass is known, so the runtime
pass gate cannot decide it. Reprojection is therefore gated on a **positive allowlist of
scene-geometry families**, keyed on the shader name in `CreateVertexProgramParams`
(`REPROJECT_NAME_PREFIXES` in `stereo/single_pass.rs`): unknown shaders stay double-drawn — correct,
just slower — and no NDC writer is ever reprojected.

## The terrain tessellation path

The tessellated base terrain builds clip in the *domain* shader, from `cb1`'s
`m_OffsetViewProjection` (byte-identical to `cb0`'s), so neither the remap nor the VS reprojection
applies. Instead the eye index rides through the pipeline on the free `.z` of the `TEXCOORD3`
interpolant:

- the **VS** (`inject_eye_forward_vertex_shader`) adds an `SV_InstanceID` input and writes
  `eye = id & 1` into that lane, widening the output to `.xyz`;
- the **HS** (`forward_eye_hull_shader`) widens the lane its control-point phase forwards, leaving the
  fork and join phases (tessellation factors) untouched — they reuse the same `o` registers for
  unrelated system values;
- the **DS** (`reproject_domain_shader`) reads the eye from `vicp[0][lane].z`, reprojects its own
  `SV_Position` by `M_eye` exactly as the VS reprojection does, and writes `SV_ViewportArrayIndex` —
  legal from the last pre-rasterization stage under the capability. `cb13` is bound on the domain
  stage for this.

The hull and domain shaders are gated *structurally* (only shaders carrying the free lane transform;
the rest return an error and are left untouched), since they are created without a paired-VS identity.
Shadow-pass permutations are excluded by name, and an unnamed shader counts as a shadow pass — failing
closed costs a terrain draw its single-pass treatment, failing open would eye-transform a shadow-atlas
draw.

## Payload integration

### Hook points

| Purpose | Function |
|---|---|
| Patch VS bytecode in-flight | `Graphics::CreateVertexProgram` |
| Patch HS/DS bytecode in-flight | `Graphics::CreateHullProgram` / `CreateDomainProgram` |
| Mirror/diverge `cb13` per view | `RenderEngine::SetAllGlobalShaderProgramConstants` |
| Mark the routed pass range | `RenderEngine::DrawRenderPassRange` |
| Close a leaked range on the draw thread | `RenderEngine::PreDraw` |
| Rebuild scene RTs at a new size | `GraphicsEngine::CreateRenderSetups`, driven by `ApplyResize` |
| Per-eye render-block re-issue | `RenderBlockBark::Draw`/`DrawZ`, `RenderBlockFoliage::Draw`, `RenderBlockOccluder::DrawZ`, `RenderBlockTerrainDetail::Draw` |
| Baked-constant reprojection | `Graphics::SetVertexProgramConstants` |

Addresses live in the pyxis defs, not here.

On top of those, single-pass installs COM-vtable detours on the live immediate context and device,
lazily on first use and torn down on eject: `RSSetViewports` and `RSSetScissorRects` (mirror or split
the bound viewport), `DrawIndexed` (promote a patched draw to two instances; re-issue an unpatched one
per eye), `Draw` (reset the fullscreen passes to full width), `VSSetShader` (cache whether the bound VS
is patched), and `CreateVertexShader` (record the patched shaders, and catch the resource-recreate path
that bypasses `CreateVertexProgram`). Install and uninstall are serialized against each other: both
suspend every other thread while they patch, so two running concurrently would suspend each other.

### The `cb13` upload

`cb13` is refreshed per pass, at the same cadence as the engine's own `cb0` upload, so it always
matches whatever view is current. Inside the routed range with divergence on it holds **distinct**
per-eye view-projections, computed in mod code from the pristine centre transform and the per-eye
`EyeRenderParams` (replicating the double-draw camera math). Everywhere else — the shadow and
reflection passes, and any frame where divergence is off — both eye slots get the current view and the
`M_eye` block is identity, so a patched shader renders exactly what it would have from `cb0`. That
shadow-safety is why `cb13` tracks the current view rather than being written once.

### Render blocks that bake their own transform

Several render blocks bake a view-projection into a per-draw constant buffer inside their own `Draw`,
across draw kinds that cannot be instance-doubled (CPU-instanced, GPU-indirect). Rather than replicate
each block's bake, the block's whole `Draw` is re-issued once per eye with the eye's half-viewport
bound, and for the duration of each call the `SetVertexProgramConstants` detour is armed to reproject
that block's own upload by the eye's `M_eye`. The arm is keyed to the block's graphics context, and it
reports whether it actually matched — a block that took an internal path staging no such matrix logs
rather than silently rendering both eyes from the centre.

The re-issue marks itself, so the draw and viewport detours leave the block's own calls alone instead
of compounding the split.

The baked matrices are stored **column-wise**: every such vertex shader builds clip with a
multiply-add chain over the four registers (`clip = Σ_i p_i · cb[k+i]`) rather than four `dp4`s, so the
per-eye buffer is the entry-wise `cb[k+i]_eye = M_eye · cb[k+i]_mono`. `cb13`'s own `M_eye` block is
the opposite convention, because the rewriter's epilogue *is* a `dp4` chain. See
[`single-pass-render-blocks.md`](single-pass-render-blocks.md).

### Collapse

With `single_pass_collapse`, `hooks/game.rs` runs one `game.Draw` (dispatch list `&[(0, false)]`)
instead of the per-eye loop, so the between-eye snapshot and restore (gated on `ordinal > 0`) is
skipped for free; `hooks/camera.rs` keeps the render camera centred, since both eyes come from `cb13`
and the shadow-anchor delta is zeroed to match; and `render_engine_post_draw` splits the one back
buffer into the two eye textures. This is the actual draw-submission win.

The collapse is mutually exclusive with the far-field share mode
([`far-field.md`](far-field.md)), which needs the per-eye dispatches it composites into. The UI
enforces that.

### Double-wide

With `single_pass_double_wide`, `vr::engine_render_resolution` returns **2x the per-eye width**, which
the deferred-`ApplyResize` native-resolution driver (`vr::resolution`) targets — so the whole scene RT
set (back buffer, `m_BackBufferLinear`, the G-buffers) is re-created double-wide and the per-pass
viewport follows the RT size automatically. The **XR swapchain stays per-eye width**, and the per-eye
capture textures are sized to half the back buffer, so the collapse's capture split copies each
full-width half straight into its eye texture with no squish.

## Configuration

All under `stereo`, all off by default.

| Flag | Effect |
|---|---|
| `single_pass` | Master switch. Substitutes the rewritten shaders and installs the routing. Forced inert without the viewport-routing capability. |
| `single_pass_patch_dryrun` | Runs the rewrite and tallies the outcomes at shader creation without substituting anything. No rendering change; validates the rewriter against the live shader set. |
| `single_pass_dual_eye` | Makes the eyes diverge: distinct per-eye `cb13`, eye-half viewports, instance doubling. On its own (no collapse, no double-wide) each eye renders into half a per-eye target — squished; a bisection step. |
| `single_pass_collapse` | Collapses the per-eye double-draw to one `game.Draw` walk. Requires `single_pass_dual_eye`. |
| `single_pass_double_wide` | Re-creates the scene targets at 2x per-eye width so each eye half is full resolution. Requires `single_pass_collapse` and `vr.native_resolution`. |
| `single_pass_reproject` | Reprojects the no-`cb0` scene families (characters, props, buildings, roads) instead of leaving them double-drawn. Needs a shader reload to apply. |
| `single_pass_terrain` | Rides the eye index through the terrain tessellation pipeline (VS → HS → DS). Needs a shader reload to apply. |
| `single_pass_tree_impostors` | Reprojects the far-distance tree impostors. Needs a shader reload to apply. |
| `single_pass_bark` | Per-eye re-issue of `RenderBlockBark`'s colour and depth draws. |
| `single_pass_foliage` | Per-eye re-issue of `RenderBlockFoliage`'s draw. |
| `single_pass_occluder` | Per-eye re-issue of `RenderBlockOccluder`'s depth prime. |
| `single_pass_instanced_per_eye` | Per-eye re-issue of an already-instanced `DrawIndexedInstanced` with a patched shader bound. Requires the collapse. **On by default.** |
| `single_pass_uniform_viewport_slots` | Puts both viewport slots back to one region once the G-buffer range ends, so a patched shader's instance parity is a no-op in the passes that are not eye-split. Requires the collapse. **On by default.** |

## Known gaps

- **Unpatched geometry has no parallax.** It is re-issued per eye so it is present and correctly
  sized in both, but it still reads the centred `cb0`. Patching more of the geometry is what removes
  this.
- **`cb0`'s projection is double-wide.** Unpatched geometry that reads `cb0` rather than `cb13`
  renders horizontally squashed under double-wide.
- **Screen-space passes leak across the eye seam.** FSR, SSAO and SSR run once over the double-wide
  target and sample neighbourhoods; they need a UV clamp at the seam.
- **The 14 already-instanced shaders** need their `SV_InstanceID` consumers rewritten to
  `SV_InstanceID >> 1` to survive the doubling. The DXBC transform is offline-testable; the
  per-instance semantics are not.
- **GPU-indirect vegetation** needs an in-place compute pre-pass doubling each indirect draw's
  `InstanceCount` (dword +1 of the 5-dword args, in the GPU-only `veg_draw_indirect` buffer, which has
  no CPU copy) before it can be instance-doubled rather than re-issued.
- **Bark's velocity pass** reprojects only the current-frame matrix; the previous-frame copy at
  `cb1[5..8]` stays centred, so bark's motion vectors are slightly wrong for SMAA and FSR.
- **The occluder intercept assumes `gfx.occluders.use_instancing = 0`** and nothing forces that cvar.

## Already-instanced draws

A draw whose patched VS takes its per-instance data through a vertex-buffer slot (rather than
`SV_InstanceID`, which would have deferred the shader) arrives at `DrawIndexedInstanced` with the
game's own instance ids. `SV_InstanceID & 1` then reads them as an eye parity nobody asked for, so
instance `i` goes to eye `i & 1` — half the batch per eye, or the left eye alone at one instance. This
is what made the buildings flicker: head motion re-sorts which instances are odd.

**Promoting the instance count cannot fix it.** Per-instance vertex-buffer stepping is indexed by the
instance id, so a doubled count reads past the batch's per-instance data. Instead `instanced_per_eye`
makes the parity irrelevant: for each eye it fills **both** `cb13` eye slots with that eye's
view-projection, camera position and `M_eye`, binds **both** viewport slots to that eye's half of the
double-wide target, and calls the original `DrawIndexedInstanced` with the caller's arguments
unchanged. Whichever parity the shader computes, and whichever `SV_ViewportArrayIndex` it writes, it
resolves to the same eye. Afterwards `cb13`'s dual-eye contents and the exact viewport slots that were
bound on entry are restored — `cb13` unconditionally, since every patched shader in the pass reads it
and a pinned one would collapse the frame to mono.

The `cb13` pin writes through a `WRITE_DISCARD` map of the already-bound buffer rather than
`upload_and_bind`, so a handled draw costs three maps and no COM ref-counting. The rows it pins and
restores are the ones `mirror_and_bind_cb13` last uploaded, cached in `Cb13Buffer::rows` — the draw
detour has no `RenderEngine` to recompute them from.

The gate is exactly the case's predicate: a patched vertex shader bound, the render thread inside the
G-buffer range, the collapse on, and `single_pass_instanced_per_eye` (default **on**). The mod's own
draws are excluded — a promoted `DrawIndexed` is re-issued through the trampoline, and a
bark/foliage/occluder per-eye re-issue by the `PER_EYE_REISSUE` marker, which this re-issue raises too.
Everything else is forwarded once, unchanged.

Cost is one extra submission per such draw: measured at a mean of ~131 draws per frame against ~20k
total, dominated by `generalmkiiihwinstanced` (the buildings), then the `vegetationfoliage*hwinstanced`
and `vegetationbark*hwinstanced` families.

The counters that measured the case still run, now splitting it into **handled** (re-issued per eye)
and **exposed** (the flag off, or the re-issue declined for want of a recorded full viewport, a live
context, or `cb13` contents). Exposed draws are further split by instance count — one instance means
the geometry is simply missing from the right eye, more than one means the batch is halved. Each draw
is attributed to the bound `ID3D11VertexShader`, named where the `CreateVertexProgram` path supplied
one (the re-acquire path does not, and those show as `<unnamed 0x…>`). The Render tab reports the last
frame plus a mean over the measured frames with a per-shader breakdown, and `single_pass`-target log
lines carry the same figures every 120 frames. The counters reset with the patched-shader set (a shader
reload), since both are keyed by shader pointer.

Every instanced draw is also bucketed by whether a patched shader was bound and whether the draw was
inside the G-buffer range, so the log distinguishes the draws the re-issue covers from the ones it
never sees. Only the in-range patched draws are eye-split; the ~10x larger out-of-range population is
the shadow, reflection and post work, where the parity has to be neutralised a different way (below).
The per-shader table carries the same in/out split, so a family that only draws outside the range is
visible rather than absent.

## Viewport slots outside the G-buffer range

A patched vertex shader writes `SV_ViewportArrayIndex = SV_InstanceID & 1` **unconditionally** — the
bytecode cannot tell which pass it is in. Only the G-buffer geometry ever binds an eye-half pair, so
everywhere else slot 1 has to be a duplicate of slot 0 for the odd-parity instances to rasterise at
all. `rs_set_viewports_detour` keeps that true for every viewport the engine binds, but the collapse's
own per-draw split (`ensure_collapse_viewport(Split)`) leaves the slots holding *different* halves, and
that state outlives the range: until the next engine bind, an already-instanced draw's odd instances go
to the other eye's half.

`single_pass_uniform_viewport_slots` (default **on**) closes that window. `VIEWPORT_SLOTS_UNIFORM`
tracks whether the two slots hold the same region — every path that binds them records what it left
behind, and anything unknown counts as non-uniform — and `unify_viewport_slots` re-binds slot 0's
region to both slots when they do not. It runs when the G-buffer range guard drops (which also covers
the GPU-indirect draws nothing detours) and, behind the flag, on an out-of-range `DrawIndexedInstanced`
with a patched shader bound. **Slot 0 is never touched**, so the repair can only make dropped instances
reappear; it cannot move what already rendered. The log reports how many repairs a window needed as
`slot-1 repairs`.

## The non-`cb0` shader census

From a census of the shipped set (`m_Name` against the rewrite outcome; the payload re-dumps it by
flipping `DUMP_VS_NAME_CENSUS`). **The census only sees shaders an area actually loads, so this list
may miss families from biomes or weather not visited; it is a floor, not a complete set.**

**Reprojected — the scene-geometry allowlist** (`REPROJECT_NAME_PREFIXES`, matched by name prefix):
`character*`, `creature` (skinned and rigid NPCs, ~34 permutations including `characterskin*`,
`characterdepth*`, `characteroutline`, `charactersphdecal*`, `charactervelocity*`); `general*`
(`general`, `generalglint`, `generalmaskedjc3`, `generaloutline`, `generalprez`, `generalprezvelocity`,
`generalrsm`, `generalshadow` — the static and dynamic models); `prop`, `propdecal`; `buildingjc3`,
`buildingrsm`; `window`; `materialtune`; `open`; `flag`, `flagdepthonly`; `snow`; `skidmarks`,
`skidmarks_normal`; roads `junctionroad*`, `splineroad*`, `dirtroad`. Plus `treeimpostor*` under
`single_pass_tree_impostors`.

**Candidate scene models not on the allowlist** (unclear family, held back pending a look): `box`,
`notex`, `lrpc`.

**Terrain — the tessellation path, not the VS reprojection.** The VS writes no `SV_Position`, so these
skip the VS reproject: `volumetricterrain*` (~30 permutations: `*4`, `*blend`, `*instanced`,
`*notessellation*`, `*offset*`, `*shadow*`), `terrainscroller`, `terrainshaderforest*`, `controlpoint`.
`terraindetailrt*` is deliberately absent — the terrain-detail block is GPU-indirect and is reprojected
by a render-block intercept that rebuilds its `cb1`, so its vertex shader must stay pristine.

**Vegetation — GPU-indirect.** `vegetationbark*`, `vegetationfoliage*`, `leaves`, `grass`,
`veginteractionvolume`, `vegintrecenter`, `vegintrecovery`. Handled by the per-eye render-block
re-issue where a block flag covers them, double-drawn otherwise.

**Excluded — NDC / non-scene** (`M_eye` would corrupt these; kept double-drawn):

- Sky and atmosphere: `skybox`, `skygradientshader`, `skymodelshader`, `atmosphereprecompute`,
  `atmosphericscattering`, `starsshader`, `softclouds`, `cirruscloudsshadow`, `foggradientshader`,
  `underwaterfoggradientshader`, `addfogvolume`, `fogvolumeapplyfs`, `fogvolumeblur`.
- Post and screen-space: `fxaa`, `temporalaafilter`, `testnoaa`, `motionblur`, `depthoffield`,
  `depthoffieldwithvpi`, `edgedetectionfilter`, `screenspace`, `screenspacetex*`,
  `screenspacesubsurfaceskinseparableblur`, `patternrecognitionfilter*`, `pixelreconstructionfilter`,
  `recreatepositionfromscreenspacedepth`, `ssao_sao`, `ssao_temporalfilter`,
  `deferred_clusteredlighting`, `lightassignmentfill`, `scenevisinit`, `gionly`,
  `skindiffuselightingcapture`.
- UI: `gui`, `quickui`, `2dtex1`, `2dtex2`, `3dtext`, `line3d`, `video`.
- Particles, effects, lights: `particleeffect*`, `beam`, `contrails`, `trail`, `billboard`,
  `meshparticle`, `distortionparticle`, `fxmeshfire`, `bulletsmoke`, `bullet`, `halo`, `halomask`,
  `lensflare`, `sunbeam`, `lightglow`, `lightsource`, `lightsource_fakelight`, `lightningshader`,
  `bavariumshield`, `spotlightcone`, `pointlightreflection`, `alphamask`, `aobox`.
- Water: `nvwater*`, `water*` (`waterboxclear`, `waterbumpcomposite`, `waterdisplacementoverride`,
  `waterfoamsub`, `watergodraysshader`, `watermask`, `waterpaintfoam`, `watersurface`, `waterwake`),
  `waves`, `mirror`.
- Decals (held back — surface-projected, want a look first): `decal`, `decaldeformable`, `decalsimple`,
  `decalskinned`, `decalskinnedgeneralmkiii`, `decalskinnedgeneralmkiiidestructible`, `ssdecal`,
  `ssdefault`.
- Occluders, probes, prez, shadow-only: `rainoccluder`, `rainoccluderblur`, `sphericalharmonicprobe`,
  `renderprez*`, `renderpreznotex`, `rendershadow`, `layeredrsm`, `depthwrite`, `tex0shadow`,
  `tex0shadownoalpha`.
