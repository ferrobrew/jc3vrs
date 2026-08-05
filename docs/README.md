# Documentation index

Three kinds of document live here. `engine/` is reverse-engineered ground truth about Just Cause 3 as it is — release addresses, layouts, and lifecycles, independent of what the mod does with them. `mod/` is what we build: design decisions and shipped implementation. `issues/` holds issue-scoped investigations that closed a specific question. [`roadmap.md`](roadmap.md) tracks where the project is going.

One deliberate exception: [`engine/rendering.md`](engine/rendering/rendering.md) documents both the engine's frame pipeline and the mod's stereo driver against it — its section numbers (`rendering §N`) are load-bearing anchors referenced throughout the code and docs, so it stays whole.

## engine/

### rendering/

- [rendering.md](engine/rendering/rendering.md) — the frame pipeline: camera and projection (§2), buffers, the depth-reconstruction passes, the draw entry points and how shaders address the screen (§4), present and `BLOCK_FLIP` (§7), device and context (§8), resolution and render setups (§9), the stereo double-Draw machinery (§11–13).
- [lighting-shadow-pipeline.md](engine/rendering/lighting-shadow-pipeline.md) — the per-frame sun-shadow and global-lighting state: the frame counters, cascade fit and amortization, the clustered froxel light grid and its consumers (§4), `SetGlobalShaderConstants`, and the table of which GlobalConstants are parity- or counter-indexed (the flicker ping-pong surface).
- [shaders.md](engine/rendering/shaders.md) — extracting, disassembling, and patching the game's shaders; tooling in `tools/shaders/`.
- [render-setups-reinit.md](engine/rendering/render-setups-reinit.md) — the runtime resize path: `CreateRenderSetups`, its callers, what `ApplyResize` and `DestroyRenderSetups` touch, and how far the swapchain is separable from the scene targets.
- [model-culling.md](engine/rendering/model-culling.md) — the two model visibility gates: the instance-level BFBC cull, and the per-render-block frustum cull against the active camera that pops buildings at a widened view's edge.

### performance/

- [profiling.md](engine/performance/profiling.md) — what survives of the engine's profiler in release, and the recommended path to per-phase CPU/GPU timings.

### character/

- [skeleton.md](engine/character/skeleton.md) — the Havok pose store, the model-space Joint API, frame ordering, and where to override bones.
- [humanik.md](engine/character/humanik.md) — the HumanIK solver: layout, per-frame lifecycle, effector ids, and the external-target injection recipe.
- [hands-and-roomscale.md](engine/character/hands-and-roomscale.md) — weapon-to-hand attachment, the shipped per-arm aim IK, and the character's velocity-driven collision proxy.

### gameplay/

- [input.md](engine/gameplay/input.md) — the action effector system, action ids, the write API, the semantic button-mapping layer, and the mouse/UI pipeline.
- [aim-pipeline.md](engine/gameplay/aim-pipeline.md) — how the player aims and fires: the per-consumer aim target cache, shot construction, dual-wield, auto-aim, and the camera getters.
- [grapple-pipeline.md](engine/gameplay/grapple-pipeline.md) — grapple targeting, hook flight and attach, and the zip/tether/retract dispatch.
- [parachute-locomotion.md](engine/gameplay/parachute-locomotion.md) — the parachute state task and steering core, and how the camera input matrix is mixed into the chute's steering through the look-steer block.

## mod/

- [vr-runtime.md](mod/vr-runtime.md) — the OpenXR runtime as built: session lifecycle, the frame loop, pose model, per-eye resolution, mirror, and the playtest checklist.
- [hud.md](mod/hud.md) — the floating-panel HUD: the redirect, compositing, and cursor interaction.
- [environment.md](mod/environment.md) — debug-UI control of time of day and weather.

### stereo/

- [single-pass-stereo.md](mod/stereo/single-pass-stereo.md) — rendering both eyes in one geometry walk into a double-wide target: the shader rewrites, the routing, the four ways content goes wrong under the collapse and what fixes each, and the configuration.
- [single-pass-render-blocks.md](mod/stereo/single-pass-render-blocks.md) — the per-render-block record of what single-pass stereo needs from each, and what it took to get there.
- [far-field.md](mod/stereo/far-field.md) — the monoscopic far field (issue #32): identifying the far-regime scene work and sharing it between eyes.
- [foveation.md](mod/stereo/foveation.md) — static foveated rendering (issue #29): the stencil radial-density-masking design, the depth-stencil-state seam, and the build plan.
- [swapchain-ownership.md](mod/stereo/swapchain-ownership.md) — the mod-owned back buffer: substituting the engine's swapchain-derived render setups so the scene renders per-eye while the DXGI buffers stay at the window size.
- [lighting-shadow-vr-interactions.md](mod/stereo/lighting-shadow-vr-interactions.md) — every default-on mod modification touching camera, frame counters, lighting, or shadows, how each interacts with the stereo double-draw, and the candidate per-frame globals behind the terrain-wide sun-shadow flicker.

### rendering/

- [fsr.md](mod/rendering/fsr.md) — FSR anti-aliasing and upscaling in the stereo pipeline.
- [upscaling-evaluation.md](mod/rendering/upscaling-evaluation.md) — Intel XeSS assessed against the shipped FSR2 as anti-aliasing and as a VR upscaler: the Arc-only D3D11 backend, the D3D11on12 route that does work and what it costs, XeSS's sub-rectangle API against the eye seam, and why the recommendation is still to finish FSR2 and validate foveation.

### performance/

- [performance.md](mod/performance/performance.md) — where the frame actually goes, measured: geometry bound by draw submission rather than fill, the post chain bound by pixels, and what single-pass stereo buys.
- [profiler.md](mod/performance/profiler.md) — the in-game profiler (issue #34): puffin CPU scopes across the frame phases and render passes, the GPU timestamp lane, the flame graph, and the F9 trace capture.

### body/

- [head-and-body.md](mod/body/head-and-body.md) — how head and body yaw relate in VR: coupling schemes, the headpose abstraction, the head-bone override, head hiding, and body IK.
- [grapple-comfort.md](mod/body/grapple-comfort.md) — the grapple body-frame filter (issue #36): the hold from fire to landing, the yaw handoff, the landing-snap absorber, and the telemetry capture.
- [parachute-yaw.md](mod/body/parachute-yaw.md) — parachute head-yaw suppression (issue #48): removing the camera-relative look-steer from the chute's steering so the head does not turn it.

### input/

- [input.md](mod/input/input.md) — how the mod taps, consumes, and injects the game's input.
- [controllers-and-roomscale.md](mod/input/controllers-and-roomscale.md) — the motion-controller and roomscale scope: phases, seams, risks, and per-mode input tables.

## issues/

- [08-14-hud-overlays-and-depth.md](issues/08-14-hud-overlays-and-depth.md) — HUD overlays and depth (issues #8, #14).
- [15-enclosed-vehicles.md](issues/15-enclosed-vehicles.md) — enclosed vehicles (issue #15).
- [20-animation-judder.md](issues/20-animation-judder.md) — animation judder (issue #20).
- [32-monoscopic-far-field.md](issues/32-monoscopic-far-field.md) — the render-pass sort/depth-bucket machinery as the distance-split mechanism for a shared far field (issue #32).
- [40-terrain-black-tiles.md](issues/40-terrain-black-tiles.md) — terrain walls and cave ceilings rendering black at grazing angles (issue #40).
