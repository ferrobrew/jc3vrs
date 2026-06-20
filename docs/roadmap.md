# JC3VRS roadmap

JC3VRS is a VR mod for Just Cause 3: render the game in stereo for an HMD, drive the head and body from the player, float the HUD as a panel in 3D, and map VR controllers onto the game's input — across on-foot, vehicles, and wingsuit.

The strategy is **flatscreen first, then OpenXR**. The whole render-and-control pipeline is built and tuned against the flat desktop — stereo as a side-by-side preview, the mouse standing in for the headset — before the OpenXR runtime is wired up. That lets us get stereo correctness, the head/body scheme, the HUD, and input feeling right without a headset in the loop; the OpenXR layer then swaps the desktop present and the mouse stand-in for real HMD pose, field of view, and swapchains.

The concrete reverse-engineering — addresses, struct offsets, exact hook points — lives in the pyxis-defs (as typed definitions with doc-comment caveats) and in the per-topic docs alongside this one. This doc is the map, not the territory.

## Architecture

Two cooperating systems. **Engine-side detours** inside the JC3 process render the scene twice with per-eye cameras and intercept the per-frame state (the clock, the render-list rotation, post-effect accumulators, the UI) that doesn't tolerate being run twice. A **VR runtime layer** in the injected DLL owns the OpenXR session, allocates the per-eye and UI swapchain textures, copies each eye's render into them, and submits the composition layers. The engine never sees the HMD — it renders as if normally, while the present is suppressed and the back buffer is captured per eye. See `rendering.md` for the render pipeline and `vr-runtime.md` for the runtime.

## Where we are

Stereo rendering works on the flat desktop: the scene renders twice per frame with per-eye cameras, the once-per-frame engine state is gated so it doesn't double-step, and each eye is captured and shown side-by-side for fusing. The camera already sits at the player's head (the eye-bone average). The temporal hazards (auto-exposure, anti-aliasing) are handled or scoped.

What's left is the control and runtime work: driving the head and body from the player, the floating HUD, VR input, and the OpenXR runtime itself — the largest remaining piece, since today's build is a desktop stereo prototype with no headset path yet.

## Trajectory

1. **Stereo rendering** — two eyes from the game camera, per-eye geometry and projection correctness, the once-per-frame state gating, and the temporal-effect fixes. Largely working; maturing. (`rendering.md`)
2. **Head and body, flatscreen** — drive the character's head bone toward the player's head (mouse as HMD stand-in), tap the game's own input feeders for look and move, and hang the body off the head with kinematic IK. The head bone is the camera's source of truth, released to physics on loss of control. (`head-and-body.md`, `skeleton.md`, `input.md`)
3. **Floating HUD, flatscreen** — render the HUD into a texture and float it as an in-engine quad per eye, with world-anchored markers reprojected against the live view. Tunable in the desktop preview before a headset. (`hud.md`)
4. **The VR runtime** — bring up the OpenXR session and per-eye swapchains, drive the camera from real HMD pose, build per-eye off-axis projections from the HMD field of view, and render at the per-eye resolution. This swaps the desktop present and the mouse stand-in for the headset. (`vr-runtime.md`)
5. **VR controllers and comfort** — map controller input onto the game's action effectors, add the comfort options (turning, vignette) and the debug/environment tooling. (`input.md`, `environment.md`)
6. **Embodiment depth** (deferred) — full-body IK so the body follows crouch and lean, and the physics-head collision response. Out of near-term scope.

## Out of scope (for now)

- Controller-driven hand aiming — tractable, but a major separate project.
- A full first-person mesh rework — the third-person assets weren't built for it.
- Multiplayer / network sync — JC3 is single-player.

## Detail lives elsewhere

- `rendering.md` — the per-frame render pipeline, stereo dispatch, and the once-per-frame hazards.
- `vr-runtime.md` — OpenXR, per-eye off-axis projection, per-eye resolution.
- `head-and-body.md` — the comfort and embodiment design; per-mode schemes.
- `skeleton.md` — reading and overriding bones; the head and IK mechanics.
- `hud.md` — the floating HUD and the world-to-screen split.
- `input.md` — tapping the game's input, and mapping VR controllers.
- `environment.md` — time-of-day and weather controls.

Concrete addresses and offsets live in the pyxis-defs.
