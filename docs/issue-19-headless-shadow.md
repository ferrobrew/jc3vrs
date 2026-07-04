# Issue #19: Rico's shadow is headless in first-person

Research into why Rico's shadow appears decapitated in first-person VR, and
viable approaches to restore the head for the shadow pass while keeping it
hidden from the colour render.

## The problem

In first-person VR, the player's head mesh is scaled to 0.001 in the
`character_update_prop_effects` detour (release `0x140_7C2_380`), which runs in
the SIM phase after pose finalization. By the time `Game::Draw` runs and the
shadow map is rendered, the head geometry is already near-zero scale — so it
casts no shadow. The shadow appears cut off at the neck.

The head-hide is necessary because the camera is placed at the head bone
position, and without hiding the head, the player sees the inside of their own
head mesh. The hide scales HEAD, fJaw, fMidLwrLip, fLeftMouthCorner,
fRightMouthCorner, and fMidUprLip to 0.001.

## The render pipeline and shadow pass structure

### Frame order

As documented in `docs/rendering.md` §1.3–§1.5 and confirmed in the dump:

1. **SIM phase** (`UpdatePassFinalizePose_Parallel` → `UpdatePropEffects`): the
   pose is finalized, and the head-hide hack runs here. The skinning palette is
   *not* built here — it is built later in the RENDER phase.

2. **RENDER phase** (`CCharacter::UpdateRender`): the character's render update
   runs, which calls `UpdateSkinning(dtf)` → `MakeSkinningPalette(dtf)`. This
   builds the GPU skinning palette from the T0/T1 `hkaPose` pair. The pose
   written by `UpdatePropEffects` is in T1, so the scaled-down head is what
   reaches the skinning palette.

3. **DRAW phase** (`HandleDrawThreadTask`): the render passes execute in order:
   - `+0xB8` `CShadowManager::CommitRenderPassSettings` — commits shadow render
     setups.
   - `+0x12A` `CRenderPass::SetupRenderContext` — sets up the main render
     context.
   - `+0x153` `SetRenderContextCamera` — feeds the render camera.
   - `+0x234` `PreDraw`.
   - `+0x259` `CRenderEngine::DrawGBuffer` (passes `0x2F`–`0x55`): depth/velocity,
     then models. **The shadow atlas is rendered before GBuffer**, in the
     `RP_SHADOW_0..7` / `RP_STATIC_SHADOW_0..7` passes.
   - `+0x35D` `CRenderEngine::Draw` (passes `0x56`–`0x96`): lighting, SSR,
     reflections, main colour.

### How render blocks detect shadow vs. main pass

Every render block's `Draw` method checks `rc->m_RenderStatus` to determine
whether it is in a shadow pass. The relevant values (from `NGraphicsEngine.h`):

```
RENDERSTATUS_DEFAULT           = 0x1
RENDERSTATUS_STATIC_SHADOWMAP  = 0x2
RENDERSTATUS_DYNAMIC_SHADOWMAP = 0x4
RENDERSTATUS_SHADOWMAP          = 0x6  (STATIC_SHADOWMAP | DYNAMIC_SHADOWMAP)
```

The character render block (`CRenderBlockCharacter::Draw`, dump line 87122)
checks `(rc->m_RenderStatus & 6) == 0` to decide whether it is in a
non-shadow pass. When `& 6` is non-zero, the block takes the shadow/depth-only
path — it uses depth-only vertex shaders (`m_VertexShaderDepth4` /
`m_VertexShaderDepth8`) and skips fragment shader setup entirely. This is
visible in the `Draw` method's first branch at dump line ~87260:

```c
if ( (rc->m_RenderStatus & 6) == 0 )
{
    // Non-shadow: full lighting setup, fragment shaders, etc.
    m_ActiveRenderPass = rc->m_ActiveRenderPass;
    if ( m_ActiveRenderPass == RP_OUTLINE_EFFECT || ... )
    { ... }
    // ... fragment program setup, material constants, etc.
}
else
{
    // Shadow / depth-only path: skip fragment setup
}
```

The render blocks are the *same* objects for both shadow and main passes —
they are populated once (in the sim phase) and drawn from the same draw list
for every pass. The block's `Draw`/`DrawZ` method branches on `m_RenderStatus`
to decide what to render. There is no separate "shadow render block" for
characters.

### The skinning palette is shared

The skinning palette (`CSkinningPaletteData::Palette[frame_counter % 2]`) is
built once per render frame by `MakeSkinningPalette(dtf)` and shared across
all passes — shadow, GBuffer, main. The shadow pass reads the same palette as
the main colour pass. So the scaled-down head in the palette affects both the
shadow and the colour render equally.

## Viable approaches

### Approach A: Restore the head for the shadow pass

The shadow atlas is rendered *before* the main colour pass within
`HandleDrawThreadTask`. If we can restore the head scale between the
shadow commit and the GBuffer draw, the head would cast a shadow while still
being hidden from the colour render.

**The challenge:** The skinning palette is built once (in
`CCharacter::UpdateRender`, before `HandleDrawThreadTask`), and the shadow and
main passes share it. Restoring the head for shadows requires either:

1. **Re-building the skinning palette with the head restored between the shadow
   and main passes.** This means:
   - In `HandleDrawThreadTask`, after `CommitRenderPassSettings` (+0xB8) but
     before `DrawGBuffer` (+0x259): restore the HEAD/jaw/lip bones to their
     original scale, call `MakeSkinningPalette` again for the local player
     character, then after the shadow passes complete, re-apply the head-hide
     and rebuild the palette again.
   - **Problem:** `HandleDrawThreadTask` runs on the render worker thread, and
     the skinning palette is shared across all render blocks. Rebuilding it
     mid-dispatch is thread-unsafe (the sim thread may be modifying the pose
     buffers). The `s_SkinnerInProgress` flag in `MakeSkinningPalette` asserts
     no skinning job is running.

2. **Using a separate skinning palette for the shadow pass.** The palette is
   double-buffered (`Palette[frame_counter % 2]`), but both buffers are used
   for T0/T1 interpolation, not for shadow/main separation. There is no
   "shadow palette" slot.

3. **Setting the head scale only in the pose buffer that the shadow reads,
   not the one the colour pass reads.** The `hkaPose` T0/T1 pair is shared
   between both passes — there is no per-pass pose. This is not feasible without
   duplicating the pose buffers.

**Verdict:** Approach A is technically possible but invasive. The cleanest
implementation would hook `CShadowManager::CommitRenderPassSettings` (release
`0x140_177_9C0`) — it runs at `HandleDrawThreadTask+0xB8`, before any shadow
rendering. In the hook's post-call:
- Restore the HEAD bone scale (and facial bones) to their pre-hide values.
- Call `MakeSkinningPalette` with `dtf=1.0` (use T1 only, no interpolation) to
  rebuild the palette with the head at full scale.
- After the shadow passes complete (hook `CRenderEngine::DrawGBuffer` or
  `DrawRenderPassRange` for the `0x2F` start): re-apply the head-hide scale and
  rebuild the palette again.

The risk is the palette rebuild on the render thread: it must be
synchronized with the sim thread's pose access (the `m_PoseAccessCheck`
asserting mutex). The mod already has hooks on the render thread, so this is
within the existing architecture, but the mutex interaction needs care.

**Alternative within Approach A:** Instead of rebuilding the palette, use a
**stencil/clip technique** — render the head geometry at full scale but clip
it from the colour pass using a custom shader constant or stencil bit. This
avoids the palette rebuild but requires shader patches, which is outside the
engine's standard render block path.

### Approach B: Eliminate the need to hide the head

Instead of scaling the head to zero, move the camera forward past the head
geometry so the near-clip plane doesn't intersect it. This overlaps with
issue #5's head-bone orientation override.

**Current camera placement:** The camera hook (`Camera::UpdateRender`, release
`0x140_xxx`) writes translation only — never rotation — and places the camera
at the head bone position (via `GetSafeBoneMatrix(HEAD)`) with small offsets
(`head_offset: (0, -0.1, 0)`, `body_offset: (0, 0.1, 0)`).

**The fix:** Once the head bone is driven from the HMD pose (issue #5), the
camera follows the eye bones for free. The head mesh could be left at full
scale if the camera is positioned *forward* of the head mesh — i.e., the near
plane is in front of the face, not inside the head.

**D3D11 near-plane management:** The engine uses reverse-Z with a near plane
set in the projection matrix. The current hardcoded ~90° FOV projection has a
near plane at some default distance (likely 0.1–0.3 m). Moving the camera
forward by ~0.1–0.15 m (the head radius) and pulling the near plane in to
~0.01–0.05 m would keep the head geometry behind the near plane and invisible
without scaling it.

**Risks:**
- Pushing the camera too far forward causes it to clip through geometry in
  tight spaces (the "positional tracking through geometry" pitfall noted in
  `docs/head-and-body.md`).
- A very near near-plane reduces depth precision (though reverse-Z mitigates
  this).
- The head bone's animated position (idle sway, look-at, recoil) would still
  move the camera if not suppressed — issue #5's head-bone override handles
  this.

**Verdict:** Approach B is the cleaner long-term solution. It eliminates the
shadow problem entirely (no head scaling = head casts a shadow normally), and
it aligns with the issue #5 work. The near-plane adjustment is a standard VR
technique.

### Approach C: Leave the head hidden, fake the shadow

Instead of restoring the head geometry, draw a simple proxy shape (a sphere
or capsule) at the head position that casts a shadow. This is a separate
render block injected into the shadow pass only.

**Feasibility:** The shadow pass renders depth-only from the shadow camera's
viewpoint. A simple proxy mesh at the head bone position would cast a shadow
blob. This is cheap (one draw call) and doesn't touch the skinning palette.

**Problem:** The shadow would not match the head's actual shape — it would be
a generic blob. For first-person VR where the player rarely sees their own
shadow in detail, this may be acceptable. But it's a visual compromise.

### Approach D: Restore head only for shadow pass via pose buffer snapshot

A variant of Approach A that avoids the palette rebuild:

1. Before the shadow pass (in `CommitRenderPassSettings` hook), snapshot the
   current head bone scale.
2. Restore the head (and facial bones) to their pre-hide scale via `SetJoint`.
3. The shadow pass will read the restored head from the model-space pose buffer
   (the render block reads vertex positions from the skinning palette, which
   was built from the pose *before* the head was hidden — see below).
4. After the shadow pass, re-apply the head-hide.

**Critical question:** When is the skinning palette built relative to
`UpdatePropEffects`? The palette is built in `CCharacter::UpdateRender` (the
RENDER phase), which runs *after* `UpdatePropEffects` (SIM phase). So the
head-hide is already baked into the palette by the time the shadow pass runs.
`SetJoint` in the shadow-pass hook would modify the pose buffer, but the
palette — which the GPU actually reads — would still have the scaled-down
head.

This means Approach D requires rebuilding the palette after restoring the
head, which brings us back to Approach A's palette rebuild challenge.

## Summary of findings

| Question | Answer |
|---|---|
| Does the shadow pass use the same skinning palette as the main pass? | Yes — `Palette[frame_counter % 2]` is built once per frame and shared across all passes. |
| How does a render block know it's in a shadow pass? | `rc->m_RenderStatus & 6` (RENDERSTATUS_STATIC_SHADOWMAP \| RENDERSTATUS_DYNAMIC_SHADOWMAP). |
| When is the skinning palette built? | In `CCharacter::UpdateRender` (RENDER phase), via `UpdateSkinning(dtf)` → `MakeSkinningPalette(dtf)`. After `UpdatePropEffects` (SIM phase). |
| Can we restore the head between shadow and main? | Only by rebuilding the skinning palette mid-dispatch, which is thread-unsafe without care. |
| Is there a separate shadow render block for characters? | No — `CRenderBlockCharacter::Draw` branches on `m_RenderStatus` internally. |

### Approach E: skip the head render blocks in non-shadow passes

*(Added after issue #5 landed, superseding the recommendation below.)*

Rather than mutilating bones at all, skip the head's *draw calls* outside the
shadow passes. The findings above already establish the machinery: the same
render blocks are drawn for every pass, and each `Draw` branches on
`rc->m_RenderStatus & 6` to detect shadow versus main. A hook on
`CRenderBlockCharacter::Draw` / `CRenderBlockCharacterSkin::Draw` that
early-outs for the head blocks when `(m_RenderStatus & 6) == 0` yields:

- the head, hair, and eyes fully hidden from the colour, depth, and GBuffer
  passes (no more eye-conflict — the eye and facial geometry lives in the head
  blocks, so the per-bone scale list and its uncovered children disappear);
- the shadow passes untouched: full-headed shadow with zero palette work;
- the entire scale-based head-hide deleted (the head bone override for the
  camera/pose remains).

**Identifying the head blocks:** Rico's model ships distinct head materials —
`mc_rico_head_dif/mpm/nrm`, `mc_rico_hair_*`, `mc_eye_gloss_alpha_dif` under
`models/jc_characters/main_characters/rico/textures/` — and an RBM model is
one render block per material, so the head/hair/eyes are their own blocks.
`CRenderBlockCharacter` carries its material (`CMaterial_N<11>`) with
texture-holder accessors on the vtable (`GetMaterialTextureHolders`), so a
block can be classified once (cached by block pointer) by matching its texture
names against the head set. Because only Rico uses `rico_body_lod*.rbm`, a
texture-name match alone identifies the player's head — no block-to-character
ownership mapping is needed. (Rico skins/DLC variants need their texture names
checked; the cinematic model `rico_cin_body_lod1.rbm` is separate.)

**To establish during implementation:** the release addresses of
`CRenderBlockCharacter::Draw` / `CRenderBlockCharacterSkin::Draw` (and whether
a separate depth/`DrawZ` entry point needs the same gate), the
`m_RenderStatus` offset in the render-context type, and the texture-holder
walk. Reflections and other non-shadow passes inherit the skip (headless in
mirrors — matches today's behaviour).

## Recommended approach

**Superseding note:** with issue #5 landed (the head bone is player-driven and
the camera is placed from the animated anchors), the up-to-date recommendation
is **Approach E** — it removes the bone-scale hack instead of extending it,
fixes the visible-eyes conflict, and leaves the skinning palette alone.

**Short-term (before issue #5 lands):** Approach A with the palette rebuild.
Hook `CShadowManager::CommitRenderPassSettings` (release `0x140_177_9C0`),
restore the head bones, rebuild the palette, then re-hide after shadows. This
is invasive but the mod already has render-thread hooks.

**Long-term (with issue #5):** Approach B — eliminate the head-hide entirely
by positioning the camera forward of the head mesh with a tight near plane.
This solves the shadow problem, the decapitated-shadow problem, and simplifies
the pose pipeline. The head bone would be HMD-driven (issue #5), and the
camera would follow the eye bones, so the head geometry is naturally behind
the camera.

## Key release addresses

| Symbol | Release address |
|---|---|
| `CCharacter::UpdatePropEffects` | `0x140_7C2_380` |
| `CCharacter::UpdateSkinning` | `0x140_77E_150` |
| `CAnimationControl::UpdateSkinning` | `0x140_43F_CA0` |
| `CPoseProducer::MakeSkinningPalette` | `0x140_C3A_FF0` |
| `CShadowManager::CommitRenderPassSettings` | `0x140_177_9C0` |
| `CRenderPass::SetRenderContextCamera` | `0x140_187_430` |
| `CRenderBlockCharacter::Draw` | (search by name in IDB) |
