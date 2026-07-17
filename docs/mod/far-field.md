# Far field (issue #32)

The monoscopic far field: identify the far-regime scene work, skip it for dial-in, and (next)
render it once per frame and share it between the eyes. The engine-side ground truth and the
dead-end record live in [`../issues/32-monoscopic-far-field.md`](../issues/32-monoscopic-far-field.md);
this is the mod design.

## Increment 1 — classification (shipped, `payload/src/far_field.rs`)

Two mechanisms, matched to how the engine organizes the work:

- **Per-entry draw-list split** on the model-family passes (`RP_MODELS_*`, `RP_CREATURES`), whose
  blocks carry real per-instance transforms: a depth-bucket boundary at the configured threshold
  makes the engine's own once-per-rotation sort produce a contiguous `[near][far]` list, and the
  `DoDraw` detour windows the walk onto either run.
- **Type gating** for the inherently-far block types (`VolumetricTerrainPatch`, `TreeImpostor`,
  `TerrainForest`, `Occluder`, `Window`; user-editable): their `IsEnabled` vtable slots point at a
  stub that consults the far-field mode and eye per dispatch. Near terrain is unaffected because
  the engine hands it off to other block types as the camera approaches.

Dial-in modes: Collect (counters only), Skip far, Skip near, Skip far on eye 1. Counters in the
Render/Performance tabs; a dump button logs the full per-pass classification state.

Accepted residuals for the baseline: minor distant bleed, and the non-ocean water tiles, which
straddle the boundary — revisit after sharing lands.

## Increment 2 — sharing (implemented: Variant B)

`Share` mode (`far_field.mode`) runs the design below; the dial-in modes remain for
classification work. Implementation: the dispatch loop in `hooks::game`, the far/near phase state
in `crate::stereo`, and the capture/composite pipeline in `far_field::share`.

### Frame structure: three dispatches

Reuse the stereo machinery (the double-`Draw` loop and its between-eye state save/restore — the
restores now run between every consecutive dispatch pair) with a third, far-only dispatch:

1. **Far dispatch** (eye-0 pose): far model runs + gated far types only (near windows empty), plus
   sky. Post-effects and screen-space passes gated off. Capture the shared far product at the end
   of the scene.
2. **Eye 0 near dispatch**: composite the far product (plain `CopyResource` — same pose, same
   projection), then render near-only as a normal dispatch.
3. **Eye 1 near dispatch**: composite via the homography warp, then near-only.

### The composite: two variants

- **Variant B — share the far G-buffer (build first).** Capture MainDepth + GBuffer0..3 from the
  far dispatch; each near dispatch starts from that instead of a clear, draws near geometry into
  it, and the stock deferred lighting then resolves the *complete* G-buffer per eye, bit-identical
  to today. Sidesteps lighting masking entirely (`CLightingPass` has no pass-level stencil gate, so
  a pre-lit far image would be clobbered or garbage-lit by the per-eye resolve). Costs: ~5 RT
  copies/warps per eye, and lighting stays per-eye (same as stock — no lighting saving, geometry
  saving only).
- **Variant A — share the lit far colour+depth (optimization, later).** One RT + depth per eye and
  the far lighting is paid once, but requires masking the per-eye lighting resolve away from
  far-only pixels (stencil or depth-bound in the clustered shader) — its own dig into
  `CRenderBlockDeferredLighting`'s resolve.

### The warp

Eye 1's composite cannot be a plain copy: the eyes' off-axis projections differ, and a copy shows
the shear (user-observed concern). But the far image and eye 1 share (nearly) one camera centre,
so the mapping is an exact 2D homography `H = P₁ · ΔR · P₀⁻¹` — depth-independent; the IPD
translation is the only residual, and it is the threshold-bounded parallax error (≈0.5 px at
250 m). The warp pass samples colour (and, in Variant B, the G-buffer targets) through `H` and
reconstructs/writes per-pixel depth (`SV_Depth`, reverse-Z per `rendering.md` §2.9). World-space
G-buffer normals warp unchanged. Shaders follow the `capture`/`vr_blit`/foveation precedent
(`payload/src/shaders/`, compiled via shadergen).

Rendering the far dispatch at the eye-0 pose (not centre) makes eye 0's composite a plain copy —
only eye 1 pays the warp, and the residual error lands entirely on eye 1 (IPD instead of IPD/2 per
eye); symmetrize later if it reads as monocular swim.

### Coverage

The far image must cover the union of both eyes' frusta or eye 1's edges sample outside it. The
mod already builds union-FOV projections (cull widen, shadow fit); the far dispatch's projection
reuses that machinery, at a modest pixel-density cost.

### Implementation notes

- **Injection point**: the composite splits the G-buffer `DrawRenderPassRange` at
  `RP_ROAD_STENCIL` (0x35) — after `RP_CLEAR` (0x34) and the Z-prepass prefix. The depth merge
  with the near dispatch's own prepass is fixed-function (`GREATER_EQUAL` + depth write under
  reverse-Z), so far content lands only where nothing nearer claimed the pixel, and equal depths
  (a far model prepassed by both dispatches — the Z passes are not windowed) take the far
  G-buffer content.
- **Capture**: after the far dispatch's G-buffer range completes, MainDepth + GBuffer0..3 are
  copied into the share pipeline's textures (the depth copy is typeless-family with a
  depth-readable SRV).
- **The warp is per-axis affine**, not the full homography: parallel same-centre eyes reduce the
  projection difference to an NDC scale+offset per axis (identity on flatscreen stereo). Canted
  displays (per-eye yaw) would need the full homography — not handled yet.
- **The far dispatch is suppressed beyond the G-buffer**: the scene/post ranges early-out, post
  effects are skipped, its `dt` is zeroed alongside eye 1's, and the shared pre-passes run only on
  the frame's first dispatch (keyed on the dispatch ordinal, which also keys the FSR VP-history
  rotation).
- Screen-space passes in the near dispatches (SSAO/SSR/GI/lighting) run over the composited
  G-buffer — correct by construction in Variant B.

### Open items

- **`RP_CLEAR` semantics**: assumed to clear the G-buffer colours (not depth, which the Z prepass
  precedes); a wrong assumption shows immediately as the composite being wiped — validate on the
  first Share run.
- **Coverage**: the far image is eye 0's frustum; eye 1's outer edge samples outside it and the
  composite discards there (sky shows in a thin strip of far content at one edge). The union-FOV
  far projection is the follow-up.
- **Velocity/TAA/FSR**: gated far types (terrain/impostors) carry cleared (zero) velocity — fine
  while static; far *models* keep real velocity (the Z-and-velocity pass is not windowed).
  Validate FSR shadow-edge behaviour in Share mode.
- **Stencil**: the composite does not carry the far G-buffer's stencil bits (material masks, e.g.
  subsurface skin) — invisible at far-field distances.
- **Variant A** (share the lit far colour+depth, lighting paid once) needs the lighting-resolve
  masking dig.
- **Transparent far content stays per-eye**: Share-gated types render only in the far dispatch's
  G-buffer range, so transparent types cannot be gated (`Window` — car/building glass — vanished
  entirely and was dropped from the default gate list). The future option is a per-entry split on
  the back-to-front transparency passes, whose sort keys are raw distances.
- **Water tiles** (non-ocean) and remaining bleed from increment 1.
