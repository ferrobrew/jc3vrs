# Lighting, shadow, and camera interactions with the stereo double-draw

This document maps every default-on modification the mod makes to Just Cause 3's camera, per-frame
frame counters, lighting, and shadow pipeline, and — for each — exactly what per-frame game state it
touches and whether the mod rolls that state back between the two eye draws. It exists to be
cross-referenced against the engine-side inventory of per-frame "ping-pong" surfaces when hunting a
global per-frame flicker: the terrain-wide sun-shadow intensity that alternates frame-to-frame in VR
even with the HMD sitting still on a desk.

This is a description of the mod as shipped (the `Default` impls in `config.rs`), not the full
diagnostic surface. Many `stereo` toggles exist only as bisection levers and default *off*; those are
listed under "Off by default" so the ground truth is complete, but they are not part of the shipped
interaction.

The engine's own frame pipeline and the double-Draw machinery are documented in
[`engine/rendering.md`](../engine/rendering.md) (§2 camera, §11–13 the double-Draw); this doc is the
mod-side companion focused on the shared per-frame state.

## The double-draw loop

`hooks::game::game_update_render` is the driver (`payload/src/hooks/game.rs`). For a stereo frame
(always on in a live VR session, and on by default on flatscreen via `stereo.enabled`) it calls
`game.Draw(spf)` **twice** — once per eye — around a snapshot/restore fence. The engine advances a
pile of per-frame state inside each `Draw`; the mod's job is to make the two dispatches make the same
per-frame decisions so the scene state advances *once per real frame* rather than once per dispatch.

The exact order per real frame:

1. **Sim tick** (original `UpdateRender`): runs `ShadowManager::UpdateRender` (the once-per-frame
   sun-shadow cascade *fit*), camera update, animation. The mod's sim-side shadow/camera tweaks live
   here (see "Sim-side, once per frame" below).
2. **Snapshot (before eye 0)**: capture the state to be pinned back between the eyes — reflection
   `EffectInfo` history, the render frame counters, optionally the CB ring / SSAO / GI indices, and
   the add-buffer parity.
3. **Eye 0 `Draw`**: prologue advances the frame counters; `PreDraw` renders the shared pre-passes
   (shadow atlas, reflections, water); the scene renders; the post chain runs. Drain the async draw
   fragment.
4. **Between-eye restore**: write the snapshotted state back so eye 1 starts from eye 0's phase.
5. **Eye 1 `Draw`**: prologue advances the counters again *from the restored value*, so the net
   per-frame advance is one; scene renders with eye 1's camera.
6. **After the loop**: restore the pristine render camera (default off), submit the VR frame.

## Save/restore ledger — default-on modifications

Each row is a shipped modification touching camera / counters / lighting / shadows. "Between-eye
handling" is what happens at step 4; the final column is the crux: does the mod leave the two eyes
sharing one per-frame value, or does something advance/diverge across the two draws.

| Modification (default value) | Hook / site | Game state read/written | Between-eye handling | Per-frame state fully rolled back? |
|---|---|---|---|---|
| `stereo.enabled` = true | `game_update_render` | Drives `game.Draw` twice | n/a (the driver itself) | The whole point: everything below exists to make the two draws converge to one frame of state. |
| `stereo.cameras` = true / VR per-eye offset | `setup_render_camera` (`hooks/camera.rs`) | `m_RenderCamera` `m_TransformF`/`m_View`/`m_Projection*`/`m_ViewProjection*`; publishes `STEREO_STATE.shadow_anchor_delta` and `vp_history.cur_eye[eye]` | Camera is rebuilt from scratch each dispatch by `SetupRenderCamera`, so no restore needed; `vp_history.rotate()` runs only on eye 0 (once per real frame). | Yes — per-eye divergence is *intended*; the camera is not shared state, it is rebuilt per eye. |
| `restore_frame_counters` = true | `snapshot_frame_counters` / `restore_frame_counters` | `RenderFrameCounters { m_Counter, m_FrameIndex, m_RingIndex }` (`get_render_frame_counters`), advanced once per `Draw` prologue; `m_FrameIndex` drives TAA-jitter phase and shadow-atlas parity | Snapshot before eye 0, restore before eye 1. Net advance = 1 per real frame (eye 1 re-advances from the restored value). | Yes — the frame-counter *sequence* matches the base game (one tick per real frame), so `m_FrameIndex & 1` parity is **not** disturbed by the double-draw. |
| `share_prepasses` = true (gated on `restore_frame_counters`) | `pre_draw` (`hooks/graphics_engine/render_pass.rs`) | On eye 1, clears `RenderPassState::m_Enabled` on the shared pre-pass categories (reflections `9..=18`, sun-shadow cascade atlas `22..=44`), re-enables after | Eye 1 skips these passes and reuses eye 0's persistent targets; passes re-enabled so next frame's eye 0 runs them | Yes — the shadow atlas is rendered **once per real frame** (eye 0 only), both eyes sample it. Requires the counter restore so both eyes agree on the atlas parity slot. |
| `fix_shadow_cascade_anchor` = true | `set_global_shader_constants` (`render_pass.rs`) | Adds `M * shadow_anchor_delta` to `RenderContext.m_ShadowCascades.m_Transform` translation before the constants stage | Per-eye by design: `shadow_anchor_delta` is set per dispatch by the camera hook and cleared each `SetupRenderCamera`; a zero delta is a no-op | Yes — this is a per-eye correction, not shared state; the transform is re-staged fresh each dispatch. |
| `widen_shadow_fit` = true | `shadow_manager_update_render` (`render_pass.rs`) | Widens active camera `m_ProjectionF` data[0]/data[5] to the union FOV around the fit | Scoped save/restore inside the *sim-side* once-per-frame call; restored before return | Yes — sim-side, once per frame, fully restored. |
| `stabilize_shadow_fit` = true | `shadow_manager_update_render` | Horizontalizes active camera `m_TransformT1` row 2 (forward vector) for the fit | Scoped save/restore, sim-side, once per frame | Yes — fully restored. |
| `force_ssao_first_pass` = true | `ssao_draw` (`hooks/graphics_engine/ssao.rs`) | Forces `SSAOPass.m_FirstPass` on each dispatch (skips the temporal history resolve) | Applied per dispatch; the pass clears it internally after its first apply | Yes — each eye computes AO fresh from its own depth; the shared 2-slot history is bypassed. |
| `restore_ssao_history` = true | `snapshot_ssao_history` / `restore_ssao_history_indices` | `SSAOPass.m_PrevFrameIndex` / `m_CurrFrameIndex` (+0x9A0/+0x9A4), advanced once per SSAO draw | Snapshot before eye 0, restore before eye 1 | Yes — both eyes resolve against the same slot; net advance one per real frame. (The shipped `Default` is `true`; the field doc-comment saying "default off" is stale.) |
| `restore_gi_cascade` = true | `snapshot_gi_cascade` / `restore_gi_cascade_index` | `GISolver.m_CascadeToUpdate` (via `LightManager -> m_GIPass -> m_pGISolver`) — which LPV cascade refreshes this draw | Snapshot before eye 0, restore before eye 1 | Yes between the eyes — eye 1 refreshes the same cascade as eye 0. **But** which cascade is fresh still alternates once per real frame (inherent engine cadence); see candidate F. (Shipped `Default` is `true`; the field doc-comment is stale.) |
| `EffectInfo` restore (unconditional) | `snapshot_effect_info` / `restore_effect_info` (`game.rs`) | `GraphicsEngine.m_EffectInfo[0..5].m_FrameIndex` and `m_EffectInfoIndex` — the reflection-proxy depth-history lifecycle, advanced once per scene dispatch | Snapshot before eye 0, restore before eye 1 (not config-gated) | Yes — advances once per real frame; without it water reflections flicker. |
| Add/draw buffer parity (unconditional) | `*get_current_add_buffer()` save/restore (`game.rs`) | The current add-list pointer (`CKeep1000Frames` parity toggle) | Saved before eye 0's draw, restored before eye 1 | Yes — eye 1's `SaveRenderFrameData` zeroes the same add-list, removing eye 0's draw-time additions (SSAO/post blocks). |
| `dedupe_post_block` = true | `render_block_post_effects_draw` (`hooks/graphics_engine/post_effects.rs`) | Gates `RenderBlockPostEffects::Draw` to once per dispatch via `WORLD_POST_BLOCK_RAN` (reset each dispatch by the driver) | Per-dispatch gate; the between-eye add-list restore cannot zero the *draw*-list entry `ApplyWorldFilters` enqueues, so eye 1 would otherwise run the whole post chain (and FSR) twice | Yes — the duplicate post-chain run (which double-steps FSR history and the post slot ring) is suppressed. |
| `gate_eye1_dt` = true | `apply_world_filters` / `apply_global_filters` (`post_effects.rs`) | Zeroes `dt` on eye 1 for `ApplyWorldFilters` (world-fade accumulator) and `ApplyGlobalFilters` (screen-fade alpha, sun-direction / heat-haze accumulators) | Eye 1 passes `dt = 0`, so the dt-driven accumulators step once per real frame | Yes — dt accumulators advance once per real frame. |
| `exposure.gate` = true | `smoothed_exposure_update`, `calc_histogram_mid_bright`, `generate_histogram`, `draw_histogram_window` (`hooks/graphics_engine/tone_mapping.rs`) | Skips exposure smoother + both histogram meters on eye 1 (`SmoothedExposure::Update`, `GenerateHistogramForFinalScene`, `DrawHistogramWindow`) | Eye 1 skips them; both eyes share eye 0's exposure | Once per real frame for the metering. **But** `m_HistogramPingPong` (the exposure histogram double buffer) still alternates per real frame — see candidate E. |
| `force_smaa_1x` = true | `anti_aliasing_apply` (`post_effects.rs`) + jitter drop in `setup_render_camera` | Drops `AntiAliasingEffect.m_Mode` T2X→SMAA 1x (restored after); disables the engine TAA jitter | Per dispatch, restored | Yes for the mode. Consequence: **no T2X temporal averaging**, so any per-frame ping-pong that base JC3 would average is now visible (this is why a global shadow ping-pong surfaces at all). |
| `reconstruct_offaxis_inverse` = true (+ `offaxis_inverse_skip_atmospheric` = true) | `perspective_fov_inverse` (`hooks/graphics_engine/reconstruction.rs`) | Substitutes the exact per-eye off-axis inverse for `Matrix4::PerspectiveFovInverse` in the deferred/SSR/SSAO/atmospheric reconstruction, gated on the live main-camera planes; the atmospheric-scattering pass (which reconstructs the sky and samples the sun cascade) falls back to the symmetric rebuild | Per-eye by design (each eye's own projection); flagged via `IN_ATMOSPHERIC` per pass | Yes — per-eye correction, no shared state advanced. The atmospheric fallback exists specifically because the off-axis sky reconstruction swims across the sun-cascade box boundary (a *contributor* to the distant per-eye shadow flip). |
| `invalidate_terrain_cb` = true (gated on `restore_frame_counters`) | `invalidate_terrain_cbs` (`game.rs`) | Stamps `RenderBlockTypeTerrain(Patch).m_WasCBApplied` with a stale frame number before eye 1 | Forces eye 1 to re-upload its terrain-tessellation CB (else it reuses eye 0's baked view-projection, keyed on the pinned frame number) | Yes — corrects an artifact *created by* the counter restore; each eye gets its own terrain projection. |
| `patch_shadow_pcf_hash` = true | shader patch (`hooks/graphics_engine/shader`) | Removes the screen-space PCF rotation hash from the sun-shadow shaders at creation | Static shader patch, not per-frame | n/a — removes a per-eye/per-pixel PCF rotation so both eyes use the same unrotated 38-tap PCF. |
| `drain_draw_fragment` = true | `drain_draw_thread_fragment` (`game.rs`) | Waits on `GraphicsEngine.m_DrawThreadWorkSignal` after each eye | Ensures eye 0's async draw fragment finishes before the between-eye restore mutates shared render-frame state | Yes — a correctness barrier so the restore does not race the in-flight fragment. Not itself a state modification. |
| `camera.enabled` = true (headpose camera) | `camera_update_render`, `camera_tree_update_render_contexts` (`hooks/camera.rs`) | Writes the active camera `m_TransformT0/T1` (and the camera-tree contexts) from the headpose pair; republishes for sim-phase readers | Sim-side, once per real frame (before the eye loop); the eye offset is layered later per dispatch | Yes — camera placement is per-frame, the per-eye split is downstream in `SetupRenderCamera`. |
| Cull widening: `widen_cull_frustum`, `widen_terrain_cull`, `widen_model_cull`, `disable_bfbc_occlusion` = true | `hooks/graphics_engine` cull hooks | Widen the various cull frusta / drop occluder planes to the binocular union | Sim-side / once per frame, scoped save/restore of `m_ProjectionF`/`m_ViewProjection` | Yes — visibility only; does not touch shadow/lighting *content*. Listed for completeness; not a flicker suspect. |

### Off by default (diagnostic levers — not part of the shipped interaction)

These are relevant to the flicker hunt because several of them exist *precisely* to pin an
otherwise-unrestored per-frame global, and they are **off** in the shipped config, so the state they
would pin is left free-running:

- `restore_cb_ring` = **false** — the `RenderEngine::m_ConstantBufferRingIndex` (+0x16C0) is **not**
  rolled back (candidate A).
- `sync_shadow_atlas` = **false** — the sun-shadow atlas parity double buffer is **not** synced
  (candidate B).
- `shadow_update_every_frame` = **false** — the cascade update-pattern amortization is left in place
  (candidate C).
- `restore_render_camera` = **false** — the pristine render camera is not written back mid-frame
  (candidate G); note it is a hygiene A/B, not believed load-bearing.
- `unjitter_shadow_fit`, `patch_lod_dissolve`, `disable_sun_shadows`, `freeze_shadow_maps`,
  `skip_ssr`, `skip_gi`, `skip_ao_volumes`, `disable_ssao`, `ssao_eye0_only`,
  `gate_setup_render_frame_data`, `gate_hand_back_buffers`, `fog/particles/spotlight_full_res` — pure
  diagnostics, off.

## Sim-side, once per frame (not part of the double-draw fence)

These run inside the original `UpdateRender`, before the eye loop, so they are inherently
once-per-real-frame and their save/restore is scoped, not between-eye:

- **Sun-shadow cascade fit** (`ShadowManager::UpdateRender`): the mod widens (`widen_shadow_fit`) and
  horizontalizes (`stabilize_shadow_fit`) the fit camera, both scoped and restored. The fit reads the
  *active* camera (which the mod does not jitter), so with a static HMD the fit inputs are constant
  frame-to-frame — *except* for whatever per-frame engine counter drives the cascade amortization
  (candidate C) and the fade (candidate D).
- **Headpose camera placement** (`Camera::UpdateRender` / `CameraTree::UpdateRenderContexts`): writes
  the active camera transform from the tick-spaced pose pair.

## Candidate per-frame globals NOT rolled back, or advanced/diverged by the double-draw

This is the list to cross-reference against the engine-side "ping-pong surface" table. Ordered
roughly by how well each matches the reported symptom — a **terrain-wide sun-shadow intensity that
alternates frame-to-frame with a stationary HMD, landing on either eye by phase** — and by whether
the double-draw specifically disturbs it (versus a base-game per-frame toggle that `force_smaa_1x`
merely un-averages).

### B. Sun-shadow atlas parity double buffer — strongest symptom match

- **State**: `ShadowManager::m_AtlasTexture` is a `Texture2DArray` split into two parity halves
  (`m_SliceBase[0]` / `m_SliceBase[1]`). The engine renders the cascade atlas into the half selected
  by `get_render_frame_counters().m_FrameIndex & 1` and the material shaders sample the *same-parity*
  half. Parity flips every real frame.
- **Double-draw handling**: `restore_frame_counters` (on) keeps `m_FrameIndex` on one value for both
  eyes, and `share_prepasses` (on) renders the atlas **once per frame** (eye 0) — so *within a frame*
  both eyes are consistent. `sync_shadow_atlas` (which would copy the freshly-rendered half onto the
  other so parity stops mattering) is **off by default**, so the parity toggle is live.
- **Why it matches**: `sync_shadow_atlas`'s own rationale describes this exact symptom — "the whole
  scene's brightness alternates a few percent." It is a global 2-state per-frame toggle, sampled
  during the eye draws, whose visual signature is terrain-wide sun-shadow intensity.
- **Caveat**: with a truly static pose *and* every cascade re-rendered each frame, the two halves
  would hold identical content and the parity flip would be invisible — so this candidate is only a
  flicker source in combination with candidate C (amortized cascade content that differs between the
  two halves' render frames) or any per-frame perturbation of the fit/content. The `m_FrameIndex`
  parity sequence itself is **not** disturbed by the double-draw (see the ledger), so if this is the
  culprit it is a base-game latent toggle that `force_smaa_1x` un-averages, not a double-draw-created
  divergence.

### C. Cascade update-pattern amortization — content churn feeding B

- **State**: `ShadowManager::m_CascadeUpdateLevels[6]`. Cascade level L re-fits and re-renders only
  every `2^L` frames; between refreshes its fit is copied forward. The distant cascades therefore
  hold content rendered at *different frames* in the two atlas parity halves.
- **Double-draw handling**: `shadow_update_every_frame` (which would zero the levels and force every
  cascade to redraw each frame) is **off by default**. The schedule is built sim-side once per frame,
  so the double-draw does not double-step it — but it is the mechanism that makes the two parity
  halves (candidate B) differ.
- **Why it matches**: `shadow_update_every_frame`'s own note attributes the residual #31 flicker to
  "distant-tree-line shadow content churn, not the update cadence." This is a strong pairing with B:
  the parity flip alternates between two halves whose distant-cascade content was rendered a few
  frames apart.

### A. RenderEngine constant-buffer ring index — genuine double-draw double-step

- **State**: `RenderEngine::m_ConstantBufferRingIndex` (+0x16C0), the per-`Draw` CB pool slot,
  advanced once per `Draw`. It is **not** part of `RenderFrameCounters` and is **not** restored by
  default (`restore_cb_ring` = false).
- **Double-draw handling**: none — it advances **twice per real frame** instead of once, so the
  CB-pool slot phase diverges from the base game. Any pass that reads a previous slot (reprojection /
  previous-frame constants) selects a different slot per eye and per frame.
- **Why it matches (partially)**: this is one of the few globals the double-draw *actively* perturbs
  (base advances once/frame, the mod twice), so it is the best fit for "absent in the base game." Its
  weakness as the shadow-specific culprit is that the sun-shadow cascade constants
  (`m_ShadowCascades`) are re-staged fresh each `SetGlobalShaderConstants`, so they should not read a
  stale slot — but any shadow-adjacent pass that does reproject through the ring would inherit the
  double-step. Worth pinning as an A/B against the symptom.

### E. Exposure histogram ping-pong — global brightness, not shadow-specific

- **State**: `ToneMappingEffect::m_HistogramPingPong` selects between `m_Histogram` and
  `m_Histogram2` per frame; the exposure Update divides the auto-exposure target by the second
  histogram's mid-point.
- **Double-draw handling**: `exposure.gate` (on) skips the metering on eye 1, so both eyes share eye
  0's exposure *within* a frame — but the ping-pong index still flips once per real frame, inherent
  to the engine.
- **Why it's lower-ranked**: exposure modulates the *whole* frame, not the terrain shadows
  specifically, so a pure exposure ping-pong would read as global luma flicker across the entire
  image, not sun-shadow intensity. Include it in the cross-reference but expect the symptom
  description ("terrain's sun-shadow intensity") to discriminate against it.

### F. GI LPV cascade freshness — indirect-light modulation

- **State**: `GISolver::m_CascadeToUpdate` — which LPV cascade is refreshed each GI draw.
- **Double-draw handling**: restored between eyes (`restore_gi_cascade` = on), so the two eyes agree
  within a frame. But *which* cascade is fresh alternates once per real frame (engine cadence), so the
  indirect-light contribution to the terrain can differ frame-to-frame.
- **Why it's a candidate**: GI feeds diffuse terrain lighting; if the sun-shadowed regions'
  indirect fill alternates with the cascade refresh phase, it would read as a shadow-region intensity
  change. Lower confidence than B/C because it is restored between eyes and averaged over multiple
  cascades.

### D. Shadow fade — per-frame sim-side scalar

- **State**: `ShadowManager::GetShadowFade()` (recorded in the trace alongside the staged cascade
  constants). A per-frame sun-shadow strength scalar.
- **Double-draw handling**: computed sim-side once per frame; both eyes read the same value. Not
  disturbed by the double-draw, but listed because it is a direct per-frame multiplier on sun-shadow
  intensity — if it animates frame-to-frame with a static pose, it would produce exactly the reported
  terrain-wide intensity change. Check whether it is constant with a stationary HMD.

### G. Pristine render camera (low priority)

- **State**: `GraphicsEngine::m_RenderCamera` matrices between the eye loop and the next Draw.
- **Double-draw handling**: `restore_render_camera` = **false**, so the last eye's offset/jittered
  matrices persist until the next Draw rebuilds them. Any sim-side reader in that window (the
  suspected one, the sun-shadow fit, actually reads the *active* camera, not the render camera) would
  see eye-1 state. Believed not load-bearing (the fit uses the active camera); kept for completeness.

## Summary for the cross-reference

- The double-draw itself perturbs the base-game per-frame *sequence* of exactly one global by
  default: **the CB ring index** (candidate A, advances twice/frame). Everything else the mod either
  restores between eyes (frame counters, SSAO, GI, EffectInfo, add-buffer) or renders once per frame
  (shared pre-passes / shadow atlas), so their per-frame *phase* matches the base game.
- The likeliest source of a *terrain-wide sun-shadow* ping-pong is therefore not a double-draw
  divergence but a **base-game per-frame toggle that `force_smaa_1x` stops averaging**: the
  **shadow-atlas parity double buffer** (B), fed by **cascade amortization content churn** (C), with
  the **shadow-fade scalar** (D) as a direct per-frame multiplier to rule in or out. The three
  shipped-off levers that would each independently pin one of these — `sync_shadow_atlas`,
  `shadow_update_every_frame`, and (for the ring) `restore_cb_ring` — are the natural A/B set.
</content>
</invoke>
