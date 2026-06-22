# FSR anti-aliasing and upscaling

The flat game's anti-aliasing is from 2015: SMAA, with an optional T2X temporal variant. The mod already disables T2X in stereo (its shared history ghosts across the two eye dispatches — `force_smaa_1x`, rendering §5.7) and falls back to SMAA 1x, which is a passable edge filter but nothing more. AMD's FidelityFX Super Resolution (FSR) is the modern replacement: a temporal reconstruction pass that does substantially better anti-aliasing than SMAA, and — with one extra piece of plumbing — doubles as a resolution upscaler, which VR will want badly.

This document records the decisions behind integrating FSR: which version, where it dispatches, and the order we build it in. The concrete reverse-engineering it leans on lives in `rendering.md` (the post-effects chain, the buffers, the stereo double-dispatch) and the pyxis-defs.

## Why FSR, and which version

The prior AA survey ruled out the alternatives for a mod shipping into a third-party game on mixed GPUs. DLSS is NVIDIA-only and its license is hostile to this use case; XeSS's cross-vendor path is DX12-only (its DX11 backend is Intel-Arc-only). FSR is MIT-licensed, runs on every vendor, and has a DirectX 11 path. That license and cross-vendor reach are the whole reason it wins.

**Target FSR 3.1.** FSR 4 is disqualifying: it is an ML model gated to AMD RDNA4 (with later INT8 backports to RDNA3/RDNA2), so it never runs on NVIDIA or Intel — a non-starter for a mod that has to run on whatever GPU the player owns. FSR 3.1.x is the newest version that is still plain SM6.2 compute with no ML hardware gate, is MIT, has an explicit native-AA mode (renderSize == displaySize, the DLAA equivalent), and exposes the newer `ffxapi` interface. The official SDK ships DX12/Vulkan backends only; the DX11 backend comes from the community port (`optiscaler/FidelityFX-SDK-DX11`, also MIT). FSR 2.2 is the fallback if the 3.1 DX11 port proves unstable under Proton — it is older and more battle-tested, at the cost of a clunkier API and a manual native-AA setup.

## The engine already produces every input FSR needs

FSR's inputs all exist on the `GraphicsEngine` singleton and are valid at the start of the post-effects block (rendering §4):

- **HDR color** — `m_MainColorBuffer` (`MainColorBuffer`, R11G11B10F). Holds the clean HDR scene at the start of the post chain; the mod already captures it there (`capture_main_color`).
- **Depth** — `m_MainDepthTexture` (`MainDepthBuffer`, D32FS8, reverse-Z, near=1/far=0). FSR must be told the depth is inverted (and infinite-far) to match.
- **Motion vectors** — `m_VelocityBufferTexture` (`motion_blur_velocity_buffer`, ABGR32), per-object screen-space velocity written in `RP_Z_AND_VELOCITY_PASS`. These are the hardest input to synthesize, and the engine already produces them.
- **Exposure** — `CToneMappingEffect::m_CurrentExposure`, metered once per real frame. Only needed on the pre-tonemap (HDR) dispatch path; the post-tonemap path doesn't use it.

The one input the mod must build itself is a **per-eye previous-frame view-projection** for correct stereo motion vectors. The engine snapshots a single previous VP in the sim path (`Camera::UpdateRender`), not per dispatch, so each eye needs its own. This is the same per-eye-VP work already identified for the stereo render generally (rendering §5.7) — FSR inherits it, it is not new to FSR.

## Where it dispatches: post-tonemap first

There are two candidate dispatch points, and the choice is a real tradeoff rather than a settled convention.

**Post-tonemap, at the anti-aliasing hook (the drop-in).** SMAA already runs at step 9 of the post chain (rendering §3), after the HDR→LDR tonemap at step 7, on the LDR temp texture in the manager's three-slot ring. Dropping FSR in at the existing `anti_aliasing_apply` hook means feeding it that same LDR image, in exactly the slot SMAA reads. No HDR flag, no exposure wiring — the image is already exposed. The only mechanical cost is mimicking the slot-ring handoff (read slot N at `mgr + slot + 80/83`, write the result into the next slot, advance the index — the engine AA does precisely this), and a modest quality cost: FSR reconstructs already-tonemapped color, so it can't recover highlight detail the tonemapper already compressed. Effects after step 9 (sun halo, fade) then render on top un-anti-aliased, but those are full-screen overlays (an additive bloom and a color multiply), so it doesn't matter.

**Pre-tonemap, at the post-chain entry (quality-max).** FSR's canonical position is on HDR pre-tonemap color, where its highlight reconstruction has the full dynamic range to work with. We would dispatch at the start of the post block (the `capture_main_color` seam, where MainColor still holds the clean HDR scene), resolve into MainColor in place, and let the rest of the chain tonemap the cleaned image. This is the better-quality input, at the cost of wiring the HDR and auto-exposure flags and using a less conventional injection point.

Note that FSR's usual "before tonemap" placement is driven mostly by *upscaling*: an upscaler must run before resolution-dependent effects so they execute once at display resolution. That argument does not apply at native AA res, so it does not force the early position for us; the remaining pre-tonemap argument is purely the highlight-quality one, which is modest.

**Decision: build post-tonemap first.** It is a true SMAA drop-in — proven slot, no HDR or exposure plumbing — and it is testable on the desktop stereo preview immediately. Once it works we can evaluate whether the highlight-quality gain justifies moving to the pre-tonemap HDR path. The engine's own AA is disabled wherever FSR is active (extend the current `force_smaa_1x` logic to drop `CAntiAliasingEffect::m_Mode` to off — mode 0 is the passthrough branch in `Apply`).

## Design for upscaling, build for AA

VR is the reason to care about upscaling. Stereo means rendering two eyes, each at the runtime's requested supersample (commonly 1.4–2.0× the panel) to counter lens-distortion sampling, at 90 Hz against a hard deadline — several times the pixel cost of a flat frame, on a deferred 2015 engine that isn't cheap. Missing the deadline in a headset means reprojection and judder, which is nauseating rather than merely ugly. Rendering the scene below panel resolution and upscaling is the standard lever, and FSR is one of the few cross-vendor ways to do it with temporal quality instead of a blurry stretch.

The crucial fact: **FSR's AA mode and upscaling mode are the same context, the same inputs, the same dispatch.** The only differences are that `renderSize < displaySize` instead of equal, and that the scene must actually be rendered at the lower per-eye resolution. So the integration we build for AA *is* the upscaler, minus the resolution plumbing — there is no throwaway work. Accordingly:

1. Write the integration with `renderSize` and `displaySize` as explicit parameters from the start; do not hardcode them equal. Native AA is just the first configuration of an upscaler.
2. Ship native AA (1:1) first. It is testable on the desktop preview today, has no dependency on the resolution re-init, and replaces SMAA immediately.
3. When the VR runtime and per-eye resolution re-init land (the VR-runtime task's blocker 2 — re-driving `CreateRenderSetups` / `Graphics::Reset` at a chosen per-eye resolution), drop `renderSize` below `displaySize`. It becomes an upscaler with no FSR-side rework, just a render-scale slider.

This also de-risks in the right order. The per-eye motion-vector work is shared by both modes, but at native res a motion-vector bug is visible and forgiving, whereas at low render scale it is brutal smearing on every head turn. Proving the MVs at 1:1 first means the hard upscaling case inherits a known-good velocity path.

### VR-specific upscaling caveats (for when we get there)

- **Disocclusion on head motion.** Fast head turns reveal large regions with no temporal history; temporal upscalers fall back to spatial reconstruction there, which is softest exactly when you are moving. Favor a conservative render scale (≈77–85%, "Quality"/"Ultra Quality") over aggressive (50%, "Performance").
- **Compositor reprojection interaction.** The OpenXR runtime may apply its own motion reprojection to hit framerate; stacking FSR temporal reconstruction under it can compound artifacts. Tunable, but only evaluable in-headset.
- **Motion vectors matter more.** An upscaler leans on MVs far harder than native AA; this is why the MV path must be solid before render scale drops.

## Runtime toggle

FSR must be switchable on and off at runtime, so we can A/B it against the engine's SMAA live in the preview and judge how well it is actually working. The toggle drives two coupled things together: whether the FSR dispatch runs, and whether the engine's own `CAntiAliasingEffect` is suppressed. Off means engine AA runs as normal and FSR does nothing; on means FSR runs and engine AA is dropped to off. The render-scale parameter (1:1 for now) sits alongside it in config, ready for the upscaling slider later. These live in the stereo/post-fx config block with the other toggles.

## Open risks to verify before/while building

- **DX11 port maturity under Proton/Wine.** The FSR 3.1 native DX11 backend is community-maintained and newer than the FSR2 one. Confirm it builds, loads its shader permutations, and dispatches under Proton before committing; FSR 2.2-DX11 is the proven fallback.
- **Motion-vector convention and scale.** Engine velocity is per-object screen-space ABGR32 written with the previous-frame VP. Confirm the sign, packing, and the scale into FSR's expected units — a wrong scale is ghosting/smearing.
- **Reverse-Z / infinite-far flags.** Depth is reverse-Z near=1/far=0; FSR must be told depth-inverted (and infinite-far if applicable) or reprojection breaks.
- **Jitter ownership.** FSR needs the camera jittered by its own Halton sequence and the identical pixel offset fed to the dispatch, per eye per frame. The engine's 2-phase TAA jitter is the wrong sequence and must be fully replaced (rendering §2.7 describes the projection-jitter injection point).
- **Reactive / transparency masks.** Water, particles, and muzzle flashes can ghost without a reactive mask; plan a second pass if transparencies smear.

## Order of work

1. Vendor the MIT FSR 3.1 DX11 backend and build its shader permutations; get it linking into the payload.
2. Stand up a per-eye FSR context (one per `draw_index`), sized to the per-eye RT, native-AA configured, recreated on resolution change.
3. Dispatch post-tonemap at the `anti_aliasing_apply` hook: feed the slot-ring LDR input, write the resolved output back into the ring, suppress the engine AA. Get it visible in the desktop stereo preview with a runtime on/off toggle.
4. Wire per-eye previous-VP motion vectors and FSR's jitter sequence; verify ghosting is controlled in motion.
5. Evaluate the pre-tonemap HDR path for highlight quality; adopt if it is worth the extra wiring.
6. Later, once the VR runtime provides per-eye resolution re-init, drop render scale below 1:1 to enable upscaling, and expose the render-scale slider.
