# Single-pass stereo

Double-draw stereo renders the scene twice per frame — one full CPU walk of the render-pass list per
eye. Profiling showed the frame is **draw-call-submission bound**, not GPU bound: ~20k draw calls per
frame (~9,900 per eye, doubled), GPU only ~41% utilised, no queue syncs. Single-pass collapses the two
geometry walks into one, so the GPU is fed from a single submission. See
[`profiler.md`](profiler.md) for how the bottleneck was measured, and
[`single-pass-render-blocks.md`](single-pass-render-blocks.md) for the per-render-block treatment.

Single-pass is opt-in and off by default. It is inert without the DXVK viewport-routing capability.

## The technique

**Instance-doubled, double-wide, viewport-routed** — for scene geometry. The fullscreen passes run
once over the whole target by default; the ones that reconstruct position from depth are re-issued
per eye under a scissor mask instead (see "Four ways content goes wrong under the collapse").

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
`cb0[29..32]` (the view-projection).

> **Referencing one of those rows is not the same as taking your position from them.** `cb0[29..32]`
> can only be a clip transform, but `cb0[4]` is a camera *position*, and shaders read it for shading, for
> a distance fade, and for world-space lookups while getting their clip position from a baked matrix
> elsewhere. The whole vegetation set does exactly that, as does `generaljc3`. Being claimed by the
> remap buys such a shader viewport routing and instance doubling but no per-eye clip, so under the
> collapse's centred render camera *both* eye halves are drawn from the centre viewpoint — and the
> family sits at a rigid half-IPD offset from everything around it, **within a single eye's image**.
> That is visible in the desktop mirror with no headset on, and it grows as you approach, because a
> wrong optical centre displaces geometry by `offset/distance` while a wrong projection or rotation
> displaces it by a fixed angle. See "The vegetation `cb0[4]` misclassification" in
> [single-pass-render-blocks.md](single-pass-render-blocks.md), which also covers the other families
> the remap claims this way.

The transform binds a mod-owned constant buffer at the free slot
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

## Four ways content goes wrong under the collapse

Once the two geometry walks became one, a set of things stopped tracking the world and slid across the
screen as the camera moved: sun shadows, terrain patches, foliage, water, decals, clouds and smoke.
They look like one bug and are four, sharing only a shape — a per-eye quantity paired with a
full-width one. Each has its own seam and its own flag.

They also share a signature worth recognising, because it is what makes the symptom *sliding* rather
than a static mis-scale: a fixed 2x horizontal scale error is a **2x horizontal motion gain**. Content
rendered or sampled at the double-wide scale sweeps past at twice the camera's rate, which reads as
sliding over the world.

**Note on what is not the mechanism.** The obvious hypothesis — that some engine constant holds the
wrong screen size — was investigated and is wrong. The engine has exactly one screen-size constant,
`cb0[8].zw`, derived from the device display size, which the mod's `ResizeBuffers` substitute already
sets to the double-wide width; the buffers it addresses are double-wide too, so it is self-consistent,
as are the `ftoi SV_Position` texel fetches (engine `rendering.md` §4.3). Do not go looking there
again.

### 1. Draws that never get an eye viewport bound

The collapse moves the eye split from pass level to draw level, so a draw arriving on an entry point
nothing detours inherits whatever the two viewport slots happen to hold — dominated by
`CollapseViewport::Full`. Two families arrive that way:

- **GPU-indirect draws** (`DrawIndexedInstancedIndirect` slot 39, `DrawInstancedIndirect` slot 40;
  engine `rendering.md` §4.2). The near tessellating terrain patches and the dominant foliage path
  submit here. Fixed by `single_pass_indirect_per_eye` (**on**): re-issue once per eye with both
  viewport slots pinned. The instance counts live in a GPU buffer and cannot be doubled, so re-issue
  is the only option — the geometry is present and correctly sized in both eyes, without parallax.
- **Non-indexed world geometry** (`Draw`, slot 13). Slot 13 is overwhelmingly the fullscreen-pass
  entry point, so the collapse resets to the full viewport for it — which stretches the decal, road
  and skidmark blocks that also submit there across both halves. Skidmarks are the sharp case: their
  vertex shader *is* patched and the blanket reset defeated the mod's own routing. Fixed by
  `single_pass_slot13_per_eye` (off), routing an allowlist of passes through `instanced_per_eye`. It
  is an allowlist, not a heuristic, because a fullscreen triangle and a decal box are identical at the
  draw call, and misclassifying a fullscreen pass is a visibly wrong frame while missing a geometry
  pass is only the status quo.

### 2. One eye-0 reconstruction basis over the whole double-wide target

A fullscreen pass is one draw with one constant buffer, so a pass that rebuilds world position from
depth gets one basis while its quad spans both halves: the left half gets eye 0's frustum compressed
2x horizontally and the right half a basis unrelated to it. The error is a function of the view
matrix, so it turns with the camera — and the two passes that sample the sun cascade over the
reconstructed positions are what made the shadows slide.

The consumer set is closed at seven (engine `rendering.md` §4.1), and each is now its own flag:
`single_pass_reconstruct_per_eye` and `single_pass_atmospheric_per_eye` (both **on** — splitting only
the deferred resolve leaves atmospheric scattering painting the mistake back over it),
`single_pass_ssao_per_eye`, `single_pass_ssr_per_eye`, `single_pass_subsurface_per_eye`, and
`single_pass_dof_per_eye` (all off, each with its own hazard). `DrawPassThrough` needs nothing — it is
reached only under wireframe.

Two of those blocks do part of their work in **compute**, which no mask can reach: a dispatch ignores
the scissor as thoroughly as it ignores the viewport, its reach is fixed by its thread-group counts,
and both blocks size those from the target's full width and address their textures straight off
`SV_DispatchThreadID` with no origin term in any constant buffer (nor could a UAV supply one — a D3D11
texture UAV picks a mip and an array slice, never a sub-rectangle). So that work is *scheduled* rather
than masked: `reconstruction::DispatchPhase` issues it whole-target on exactly one run of the split —
the **first** for a prologue the masked draws consume (the bokeh near-field coverage), the **last** for
an epilogue that consumes what they produced (the SSR blur). That is the same single whole-target pass
the un-split block makes, so the eye-seam bleed from the horizontal half of each separable blur (six
texels of the block's own working resolution) is inherent to the collapse, not introduced by the split,
and is there with the flags off too.

The shared machinery is `reconstruction::split_fullscreen_pass`, which holds the preconditions and the
demotion rule. Each run is masked with a **scissor**, not a half viewport, so the quad keeps its
one-to-one NDC-to-pixel mapping and its G-buffer sampling stays correct; the per-eye basis comes from
folding the full-target-NDC to eye-NDC remap into the substituted inverse. The demotion rule matters:
a run that never masked has already drawn the whole target, so the split stops after one run rather
than double-exposing an accumulating pass.

`CRenderBlockSSDecal` has the same defect without being a `PerspectiveFovInverse` consumer — its
type-level `Setup` builds a basis inline into fragment `cb1[0..3]` and its pixel shader derives the
depth-fetch UV projectively from its own clip position. `single_pass_ssdecal_per_eye` (off) fixes both
halves together: the block is re-issued per eye restaging that eye's basis, and the 12 `ssdecal` pixel
shaders get one spliced instruction biasing the depth UV into that eye's half. The same `uv` feeds the
reconstruction matrix (which wants the per-eye value) and the depth fetch (which wants the double-wide
one), so they cannot be corrected as one.

### 3. A projective screen UV in per-eye space against a double-wide buffer

The legacy `Water*`/`WaterBox*` family takes its reflection, refraction, and depth UV from a
CPU-staged matrix, projectively, normalized over the **viewport** while the buffers it indexes are the
whole target (engine `rendering.md` §4.3). `single_pass_water_uv_per_eye` (off) composes one more bias
per eye, `u' = (u + eye) · 0.5`, onto the rows the block type staged. Because the type stages them
once per pass, ahead of the `Draw` being re-issued, the fix restages the rows per eye rather than
transforming an upload in flight, and puts the type's own rows back afterwards. It is deliberately
*not* reprojected by `M_eye`: the geometry still rasterizes from the collapsed centre view, so the UV
has to describe where that geometry actually landed. Default off because the affected family may not
be on screen at all at the water-quality setting in use, which makes an A/B the only way to tell the
fix from a no-op.

> **A correction worth keeping.** This mechanism was first attributed to `NvWaterHighEnd`, and the fix
> was first designed as a DXBC rewrite of nine water pixel shaders with eye parity plumbed into a
> pixel shader. Both were wrong. `NvWater*` builds its UV from `SV_Position × cb0[8].zw`, which is
> already consistent under double-wide; the projective-UV family is the legacy non-NV one. And the
> defect lives entirely in a CPU-staged matrix, so no bytecode rewrite and no pixel-shader eye parity
> is needed. `NvWater*` has a different defect — its vertex shader writes clip from a baked
> model-view-projection in its own constant buffer, so both eyes see the collapsed centre view. Flat
> water at the wrong depth is invisible in a screenshot and wrong only in a headset, which is why it
> survived so long; `single_pass_nvwater_per_eye` (off) addresses it, and
> `single_pass_ssdecal_geometry_per_eye` (off) is the same shape for the decal box.

#### The water box surface, and what the legacy family's clip path actually is

The water-box *surface* grid — `NWater::DrawWaterBoxSurface`, run over every registered box from
inside `CNvWaterHighEndRenderBlock::Draw` — was recorded as deliberately uncovered because its type's
`SetupSurface` stages no screen-UV matrix. That is true but incidental; the interesting part is its
clip path, which is the legacy family's and settles what "needs per-eye draw wiring first" meant.

The block stages a box transform (a scale by the half-extents plus a translation of
`centre − camera position`) and `waterboxsurface` does:

    world_rel = box_transform(cb1[0..3]) · position
    clip      = cb0[0..3] · (world_rel + cb0[4])

`cb0[0..3]` is the **full**, translation-bearing view-projection — not the translation-free
`m_OffsetViewProjection` at `cb0[29..32]`. The per-eye register remap covers only `cb0[4]` and
`cb0[29..32]`, so the projection is out of its reach. Disassembling the shipped water vertex shaders
shows this is the whole legacy family's idiom, not one permutation's: `waterbox`, `waterboxbelow`,
`waterboxsurface`, `watershader_lod0`, and `watershader_lod1` all read `cb0[0..3]` plus `cb0[4]`, and
`waterboxclear`, `watergodraysshader`, and `waves` read `cb0[0..3]` alone. (`nvwaterbox` and
`nvwaterbox_tess` read `cb0[4]` alone, with clip from the block's baked matrix — the shape
`baked_cb_block_owns_vs` exists for, though `nvwater*` is not on its list.)

The `cb0[4]` reference alone is enough for the remap to claim these shaders, and it is the
"camera-only" misclassification in its most damaging form: the eye offset gets added to the shader's
reconstructed **world position** and the result is then viewed from the centre, so the geometry is
displaced by the eye offset in the *opposite* direction to the parallax it should have had. In
practice a per-eye re-issue draws one instance, so `SV_InstanceID & 1` resolves to eye 0 in both
halves and both eyes get eye 0's displacement — a rigid ~32 mm world-space offset of the water
relative to everything around it, growing as `offset / distance`, visible in a single eye.

Reprojection is the transform these want: it replaces the clip position wholesale with
`M_eye · clip_center`, so it does not care that the source was `cb0[0..3]` rather than a baked matrix,
and it fixes the parallax, the wrong-direction displacement, and the double-wide squash together. It
is unreachable only because `should_reproject_camera_only` is gated on `REPROJECT_NAME_PREFIXES` and
the `water*` family is on the excluded list.

**It cannot simply be un-gated, and this is the coupling to be careful of.** Reprojecting the
geometry moves where it rasterises, and the same shaders emit a *projective screen UV* on `TEXCOORD1`
from a CPU-staged matrix built out of the centre view-projection — the one mechanism 3 above
corrects, deliberately *without* `M_eye`, because "the geometry still rasterizes from the collapsed
centre view, so the UV has to describe where that geometry actually landed". Reproject the geometry
and that stops being true: the staged rows would then have to be rebuilt from the *eye's* full
view-projection rather than the render context's centre one. The two changes are one change. Doing
either alone trades a known-shape defect for a subtler one, so neither has been made.

The surface grid is the awkward member of the set: it is drawn from `NWater::DrawWaterBoxSurface`
rather than from `WaterBoxRenderBlock::Draw`, so the per-eye re-issue in
`hooks/graphics_engine/water.rs` does not reach it, and it would need its own intercept before the
paired fix could be applied to it.

### 4. A reduced-resolution target handed the scene's viewport

The collapse's viewport split was target-blind: it re-derived the eye halves from the recorded *scene*
viewport before every draw in the range, regardless of what the engine actually had bound. Several
passes draw into a shared quarter-resolution off-screen target (engine `rendering.md` §9), and a draw
into a `W × H/2` target handed a `2W × H` viewport is magnified 2x about the target origin and
cropped to a quadrant — for the right eye, whose half starts at `x = W`, clipped away entirely. That
is the clouds-and-smoke sliding. `collapse_viewport_follows_target` (off) keeps a second record that
is always the live bind, so the split follows the bound target. The single record could not simply be
made to follow the engine, because it is also the scene notion and following a half-resolution post
target would have mis-split the scene itself.

The composes need no matching change and must not get one: they map the whole low-resolution texture
over the whole target with a baked UV, so a half-and-half texture composites correctly on its own, and
both blend rather than overwrite, so re-issuing either per eye would double-composite.

## The clustered froxel grid, per eye

Not a sliding defect, but the same shape and a defect in every collapsed frame: the grid is built with
one eye's projection paired with the **double-wide** tile count, so it is twice as wide as the frustum
it describes and every local light lands in the wrong 64-pixel tiles — for both eyes, and for the ~20
forward-lit render-block types that sample it as well as the deferred resolve. `foliage` reads it a
frame early, inside the G-buffer range. See engine `lighting-shadow-pipeline.md` §4.1–4.2 for how the
grid is built and who consumes it.

`single_pass_clustered_per_eye` (**on**) rides the per-eye resolve re-issue: each run narrows the
light-assignment viewport to its own half of the tile grid, rebuilds the geometry transform from that
eye's projection, makes the tile bounds affine in the *absolute* tile index, and suppresses the second
run's `Graphics::Clear`. The two halves then compose, on the three properties §4.1 records — the clear
is the only whole-target step, the assignment blend is commutative over disjoint tiles, and the fill is
per-tile-local — so the grid ends the frame valid in **both** halves, which is what the forward
consumers need this frame and next. Nothing is written into engine memory; every substitution is a
constant-buffer upload or viewport/clear state the mod already intercepts.

`single_pass_clustered_per_eye_light_view` (**on**) additionally assigns each eye's lights from that
eye's position, by writing the eye's world offset into the translation row of the uploaded `cb2` view
matrix — algebraically identical to biasing the light positions, and it needs no engine write because
that row is exactly where the engine's own zeroed translation goes. The per-eye display canting is not
applied, only the positional offset. It keeps its own flag because the difference may fall below the
64-pixel tile granularity.

The split **declines itself**, leaving the un-split behaviour and logging once, when the eye seam
would not fall on a whole tile column (double-wide width not a multiple of 128), when the bound
assignment target is not the tile grid it sized for, or when the dispatch has no lights. If eye 0
splits but the resolve turns out not to be maskable, the grid is rebuilt whole rather than left
half-cleared.

## Configuration

All under `stereo`. Off by default unless marked.

### Structure

| Flag | Effect |
|---|---|
| `single_pass` | Master switch. Substitutes the rewritten shaders and installs the routing. Forced inert without the viewport-routing capability. |
| `single_pass_patch_dryrun` | Runs the rewrite and tallies the outcomes at shader creation without substituting anything. No rendering change; validates the rewriter against the live shader set. |
| `single_pass_dual_eye` | Makes the eyes diverge: distinct per-eye `cb13`, eye-half viewports, instance doubling. On its own (no collapse, no double-wide) each eye renders into half a per-eye target — squished; a bisection step. |
| `single_pass_collapse` | Collapses the per-eye double-draw to one `game.Draw` walk. Requires `single_pass_dual_eye`. |
| `single_pass_double_wide` | Re-creates the scene targets at 2x per-eye width so each eye half is full resolution. Requires `single_pass_collapse` and `vr.native_resolution`. |
| `single_pass_clear_range_on_dispatch` | **On.** Clears a leaked routed range on the draw thread rather than the game thread. |

### Shader rewrites (need a shader reload to apply)

| Flag | Effect |
|---|---|
| `single_pass_reproject` | Reprojects the no-`cb0` scene families (characters, props, buildings, roads) instead of leaving them double-drawn. |
| `single_pass_reproject_camera_only` | Extends that to the allowlisted families the `cb0` remap claims on a camera-position reference alone (`generaljc3`, `landmark`, `layered`, `layeredblend`), which otherwise get viewport routing but no per-eye clip. |
| `single_pass_terrain` | Rides the eye index through the terrain tessellation pipeline (VS → HS → DS). |
| `single_pass_tree_impostors` | Reprojects the far-distance tree impostors. |

### Per-eye re-issue of a render block

| Flag | Effect |
|---|---|
| `single_pass_bark` | `RenderBlockBark`'s colour and depth draws. Also declines the `cb0` remap on `vegetationbark*` (see below). |
| `single_pass_foliage` | `RenderBlockFoliage`'s colour and depth draws. Also declines the `cb0` remap on `vegetationfoliage*` (see below). |
| `single_pass_occluder` | `RenderBlockOccluder`'s depth prime. |
| `single_pass_nvwater_per_eye` | The WaveWorks water blocks, whose baked model-view-projection leaves both eyes on the centre view. |
| `single_pass_ssdecal_geometry_per_eye` | The screen-space decal box geometry, same shape. On top of `single_pass_ssdecal_per_eye`. |

### Per-eye viewport and draw routing

| Flag | Effect |
|---|---|
| `single_pass_instanced_per_eye` | **On.** Per-eye re-issue of an already-instanced `DrawIndexedInstanced` with a patched shader bound. |
| `single_pass_indirect_per_eye` | **On.** Per-eye re-issue of the GPU-indirect draws (mechanism 1). |
| `single_pass_slot13_per_eye` | Per-eye re-issue of non-indexed world geometry, by pass allowlist (mechanism 1). |
| `collapse_viewport_follows_target` | Derives the eye halves from the bound render target rather than the scene viewport (mechanism 4). |
| `single_pass_uniform_viewport_slots` | **On.** Puts both viewport slots back to one region once the G-buffer range ends, so a patched shader's instance parity is a no-op in the passes that are not eye-split. |

### Per-eye fullscreen and screen-space passes

| Flag | Effect |
|---|---|
| `single_pass_reconstruct_per_eye` | **On.** Deferred clustered-lighting resolve (mechanism 2). |
| `single_pass_atmospheric_per_eye` | **On.** Atmospheric scattering / aerial perspective (mechanism 2). |
| `single_pass_ssao_per_eye` | SSAO. Hazard: a temporal history advanced per invocation, snapshotted and restored across the split — but the AO generation and blur ahead of the mask still re-run unmasked. |
| `single_pass_ssr_per_eye` | Screen-space reflections. The `m_UseComputeBlur` epilogue's two dispatches are issued on the second run only, once, over both halves. |
| `single_pass_subsurface_per_eye` | Screen-space subsurface skin. |
| `single_pass_dof_per_eye` | Depth of field: splits `CDownScale2x2PackFocus::Apply`, whose closing pack draw is the sole consumer of the DoF basis. Its five-dispatch near-field prologue reads no view or projection matrix, so it runs whole-target on the first run only and the second run reads the same coverage. |
| `single_pass_ssdecal_per_eye` | Screen-space decals: per-eye basis plus a spliced depth-UV bias in the 12 `ssdecal` pixel shaders (mechanism 2). |
| `single_pass_water_uv_per_eye` | Legacy water's projective screen UV (mechanism 3). |
| `single_pass_clustered_per_eye` | **On.** Per-eye clustered froxel light grid. |
| `single_pass_clustered_per_eye_light_view` | **On.** Also assign each eye's lights from that eye's position. |

## Why `PreDraw` is outside the collapse, and why extending it over it would save nothing

`PreDraw` is the second-largest GPU item in the frame (~2.16 ms, `docs/mod/performance.md`) and does
not come through `DrawRenderPassRange`, so it is natural to read it as a large pool of draws the
collapse has not claimed. It is not. **Under the collapse there is exactly one dispatch, so `PreDraw`
already runs exactly once per frame.** `hooks/game.rs` builds the dispatch list as `&[(0, false)]`
when `collapse_active()`, and `CGraphicsEngine::HandleDrawThreadTask` calls `CRenderEngine::PreDraw`
once per dispatch. There is no second walk to collapse; the ceiling on submission savings from
extending the collapse over `PreDraw` is **zero**.

The measurement agrees, and is the reason to trust this rather than the reading above. In the
off/on comparison in `performance.md`, `PreDraw` is 2.29 ms with two dispatches and 2.25 ms with one.
If it were duplicated per eye, halving the dispatch count would have roughly halved it. It does not,
because `share_prepasses` (on by default) already elides the view-independent categories on the
second eye of the double-draw path. The ~0.04 ms difference is the residual: the categories that path
still runs twice.

### The classification

`CRenderPass::Draw` (the pre-pass entry, vtable slot 0) selects the pass camera: the pass's own
external camera if it has one, otherwise the camera manager's render camera. `SetRenderContextCamera`
then overrides that from the render context's *pass id* for the shadow families — three disjoint
branches selecting a uniform light camera at `pass − 22`, `pass − 30`, and `pass − 38`, which land
exactly on `PRE_RP_STATIC_SHADOW_0` (22), `PRE_RP_SHADOW_0` (30), and
`PRE_RP_SHADOW_REFLECTIVE_SUN_NEAR` (38). So pass ids 22–40 are rendered from the **sun / light**
view by construction, not by convention, and cannot depend on which eye is being drawn.

| Category | Camera | Per-eye? |
|---|---|---|
| Terrain-patch prep (1–7) | falls through to the render camera | yes |
| Sky-lighting LUT (8) | render camera | held per-eye, conservatively |
| Planar + environment reflections (9–17) | the pass's own reflection camera | no |
| Cloud shadows (18) | world-space | no |
| Vegetation (19–21) | render camera | held per-eye, conservatively |
| Sun-shadow cascade atlas, static + dynamic + reflective (22–40) | light camera, from the pass id | **no**, verified in the binary |
| Water simulation compute (41–44) | own targets and viewports | no |
| Rain occluder (45) | render camera | held per-eye, conservatively |

`SHARED_PREPASS_CATEGORIES` in `hooks/graphics_engine/render_pass.rs` encodes the "no" rows that have
been validated in game (9–18 and 22–44) and elides them on the second eye of the double-draw path.
Under the collapse that elision is inert and correctly so — nothing is duplicated to elide.

### What is left

The prepasses are already at their floor of one issuance per frame. What remains in `PreDraw` is the
game's own baseline shadow, reflection and vegetation cost, the same as flatscreen, and reducing it is
a shadow/reflection quality decision (cascade count, atlas resolution, update cadence), not a stereo
one. Two stereo-attributable riders are worth knowing about, both small and both deliberate:

- `widen_shadow_fit` (**on**) widens the cascade fit to the union FOV of both eyes, which puts more
  geometry inside each cascade and so costs some shadow draws. That is the price of the cascades
  covering both eyes at all.
- `shadow_update_every_frame` (off) would defeat the engine's `2^L` cascade update amortization and
  multiply the shadow render cost. It is a flicker diagnostic; leave it off.

The related item — "roughly 450 instanced draws per frame issued outside the G-buffer range (the
shadow-pass families)" — is the same answer. Those are issued once per frame under the collapse. The
collapse's draw and viewport detours are gated on `in_gbuffer_range()`, which is raised only for
`first >= RP_Z_OCCLUDERS`, so no prepass draw is instance-doubled or eye-split. The only thing that
leaks into them is a patched shader's unconditional `SV_ViewportArrayIndex` write, which
`single_pass_uniform_viewport_slots` (on) already neutralises. There is no submission saving there
either: they are correctness surface, not duplicated work.

## Known gaps

- **Unpatched geometry has no parallax.** It is re-issued per eye so it is present and correctly
  sized in both, but it still reads the centred `cb0`. Patching more of the geometry is what removes
  this.
- **`cb0`'s projection is double-wide.** Unpatched geometry that reads `cb0` rather than `cb13`
  renders horizontally squashed under double-wide.
- **Screen-space passes leak across the eye seam.** FSR and the unsplit screen-space passes run once
  over the double-wide target and sample neighbourhoods; they need a UV clamp at the seam.
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
