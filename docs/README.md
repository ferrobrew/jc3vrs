# Documentation index

Three kinds of document live here. `engine/` is reverse-engineered ground truth about Just Cause 3 as it is — release addresses, layouts, and lifecycles, independent of what the mod does with them. `mod/` is what we build: design decisions and shipped implementation. `issues/` holds issue-scoped investigations that closed a specific question. [`roadmap.md`](roadmap.md) tracks where the project is going.

One deliberate exception: [`engine/rendering.md`](engine/rendering.md) documents both the engine's frame pipeline and the mod's stereo driver against it — its section numbers (`rendering §N`) are load-bearing anchors referenced throughout the code and docs, so it stays whole.

## engine/

- [rendering.md](engine/rendering.md) — the frame pipeline: camera and projection (§2), present and `BLOCK_FLIP` (§7), device and context (§8), resolution and render setups (§9), the stereo double-Draw machinery (§11–13).
- [skeleton.md](engine/skeleton.md) — the Havok pose store, the model-space Joint API, frame ordering, and where to override bones.
- [humanik.md](engine/humanik.md) — the HumanIK solver: layout, per-frame lifecycle, effector ids, and the external-target injection recipe.
- [aim-pipeline.md](engine/aim-pipeline.md) — how the player aims and fires: the per-consumer aim target cache, shot construction, dual-wield, auto-aim, and the camera getters.
- [grapple-pipeline.md](engine/grapple-pipeline.md) — grapple targeting, hook flight and attach, and the zip/tether/retract dispatch.
- [hands-and-roomscale.md](engine/hands-and-roomscale.md) — weapon-to-hand attachment, the shipped per-arm aim IK, and the character's velocity-driven collision proxy.
- [input.md](engine/input.md) — the action effector system, action ids, the write API, the semantic button-mapping layer, and the mouse/UI pipeline.
- [render-setups-reinit.md](engine/render-setups-reinit.md) — the runtime resize path: `CreateRenderSetups`, its callers, state assumptions, and swapchain separability.
- [profiling.md](engine/profiling.md) — what survives of the engine's profiler in release, and the recommended path to per-phase CPU/GPU timings.
- [shaders.md](engine/shaders.md) — extracting, disassembling, and patching the game's shaders; tooling in `tools/shaders/`.

## mod/

- [vr-runtime.md](mod/vr-runtime.md) — the OpenXR runtime as built: session lifecycle, the frame loop, pose model, per-eye resolution, mirror, and the playtest checklist.
- [head-and-body.md](mod/head-and-body.md) — how head and body yaw relate in VR: coupling schemes, the headpose abstraction, the head-bone override, head hiding, and body IK.
- [hud.md](mod/hud.md) — the floating-panel HUD: the redirect, compositing, and cursor interaction.
- [input.md](mod/input.md) — how the mod taps, consumes, and injects the game's input.
- [fsr.md](mod/fsr.md) — FSR anti-aliasing and upscaling in the stereo pipeline.
- [controllers-and-roomscale.md](mod/controllers-and-roomscale.md) — the motion-controller and roomscale scope: phases, seams, risks, and per-mode input tables.
- [environment.md](mod/environment.md) — debug-UI control of time of day and weather.

## issues/

- [08-14-hud-overlays-and-depth.md](issues/08-14-hud-overlays-and-depth.md) — HUD overlays and depth (issues #8, #14).
- [15-enclosed-vehicles.md](issues/15-enclosed-vehicles.md) — enclosed vehicles (issue #15).
- [20-animation-judder.md](issues/20-animation-judder.md) — animation judder (issue #20).
