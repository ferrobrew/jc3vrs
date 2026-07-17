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

## Increment 2 — sharing (design)

### Frame structure: three dispatches

Reuse the stereo machinery (the double-`Draw` loop and its between-eye state save/restore) with a
third, far-only dispatch:

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

### Open items

- **Clear ordering**: the composite must land after the engine's own depth/G-buffer clears and
  before geometry. The per-pass `PreDraw` clear flags and `RP_CLEAR` (0x34, which runs *after* the
  Z passes in index order) need a trace to pin the exact injection point per target.
- **Capture point**: end-of-scene for the far dispatch (the `capture_main_color`-style hooks are
  precedent); G-buffer captures are new targets on the capture state.
- **Screen-space passes in the near dispatches** (SSAO/SSR/GI) run over the composited G-buffer —
  correct by construction in Variant B.
- **Velocity/TAA/FSR**: the far pixels carry no per-eye velocity; FSR's stereo MV correction
  (`fsr.md`) may need the far region treated as static — validate for shadow-edge flicker.
- **Water tiles** (non-ocean) and remaining bleed from increment 1.
- **Between-eye state**: the third dispatch multiplies the per-dispatch side effects the stereo
  machinery already gates (EffectInfo ring, dt accumulators, draw-done signal) — audit
  `rendering.md` §5/§13 against three dispatches.
