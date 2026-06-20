# Head and body movement in VR

Giving the player full, comfortable control of where they look and where the character faces, driven by the tracked HMD and later the controllers. This is design plus a reverse-engineering target: the design decides how head and body yaw relate; the RE is finding where the engine writes the head bone and camera so the HMD can own them.

## The core problem

VR sickness is driven mostly by vestibular disconnect — the eyes report a rotation the inner ear never felt. Every scheme below is a different answer to how much of the character's facing (yaw) comes from the player's real head and body versus from artificial input. A second, separate concern is where the visible torso points, which is presentation and IK, not comfort. Two ergonomic limits recur: comfortable sustained head yaw is about 30°, practical maximum about 45°; the eyes turn rather than strain past about 30°.

## Yaw-coupling schemes

- **Decoupled head (A).** Body fixed, head free within a ~30–45° cone; body yaw set by movement or input. Maximally comfortable because nothing rotates the body under the player without input. The natural baseline, but rarely enough on its own for 360° facing.
- **Body-follows-head (B).** Body yaw lazily chases head yaw through a deadzone, then eases, recentering as the head returns forward. This drives the visible torso and the movement frame *only*, never the camera — feeding it to the camera injects vection. The deadzone mechanism is standard (it's VRIK's `MaxRootAngle`); the specific "torso = low-pass-filtered head yaw" angles and time-constants are engineering lore, not a documented platform spec.
- **Stick-driven body yaw (C), head decoupled on top.** Snap turn is instant, in fixed increments (45° is the de-facto default) — the safe artificial turn, since instantaneous rotation produces no optic flow and so no vection. Smooth turn is more immersive and more nauseating; ship it opt-in with limited acceleration and a comfort vignette. Snap must rotate around the *head* position, not the rig centre, with a brief fade to hide the discontinuity.
- **Physical turning (D).** Body yaw from real rotation; best possible comfort, but needs space and can't represent fast or vehicle yaw. Always honoured on top of the others.

## Recommended scheme per JC3 mode

JC3 is a fast open-world action game — shoot, melee, drive, fly, wingsuit, grapple — and is inherently Moderate-to-Intense on the comfort scale; the genre can't be a true "Comfortable" tier, so the play is to ship a survivable default and expose the full option matrix.

**On-foot.** Head fully decoupled and 1:1 (A), free within the cone, never clamped — hand off to body catch-up at the edge rather than clamping. Body yaw stick-driven (C), default snap 45°, smooth + vignette opt-in. Visible torso follows the head (B), deadzone ~15–30°, driving the torso and movement frame only. Locomotion-forward defaults to head-relative (expose hand-relative). Physical turning always honoured.

**Vehicles** — the easy case, because the body root is fixed, so you skip body-yaw decoupling entirely. Head-look 1:1 and free; do *not* clamp it (breaking 1:1 is itself a discomfort source, and the seated pose self-limits travel). Recenter is a manual, bindable button — no auto-snap-back. Decoupled head-look, not look-to-steer: steering stays on the stick, and looking out the side window must not turn the vehicle. No VR snap-turn needed (the real neck covers it). Keep an earth-stable horizon — don't roll it with the vehicle — and treat the cockpit interior as a free rest frame; default a stronger vignette for planes and helis.

**Wingsuit / parachute** — the hardest, most provocative mode, with high continuous speed, banking, and constant optic flow, and no strong shipped precedent, so treat this as a hypothesis to playtest rather than settled practice. Lock body yaw to the flight/travel direction (the steering is the body banking; don't also let the stick free-spin it). Keep the head fully decoupled and 1:1 — the headline feature is looking around while you fly where you're pointed. Use a strong vignette keyed to speed and angular velocity, since banking is high angular velocity and peak conflict. Do *not* roll the camera with the wingsuit bank by default — roll is a top sickness source; make camera-roll-on-bank an opt-in for experienced players and default to a roll-stabilised horizon. Fade or vignette-spike through grapple yanks, which are sudden accelerations.

## Body-from-head heuristic

JC3 has a real rig, so there's a known body bone — but you still decide how its yaw relates to the HMD. The layered estimate: body yaw is a damped follow of head yaw with a deadzone (~10° free, catch-up over ~30° / 0.2–0.5 s — the Cyberpunk third-person-mod free-range/catch-up pattern, mechanically VRIK's `MaxRootAngle`); a small hand-midpoint correction weighted low, since hands move wildly; and the locomotion-forward frame decoupled from both gaze and torso. Drive the existing body bone via scheme B rather than building a physics body.

## HMD pose to camera

The transform chain, root applied outermost so roomscale translation is interpreted in the body's local frame:

    world_camera_eye = body_root · neck/head_attach_offset · hmd_head_pose · (±IPD/2 along head-local X)
    view_eye         = inverse(world_camera_eye)      (paired with that eye's asymmetric projection — vr-runtime blocker 1)

The key move is the yaw split: body yaw comes from the stick (C) or the vehicle/flight root, while head pitch, roll, and residual yaw come only from the HMD, coupled through the deadzone/catch-up above. Head pitch and roll must never feed back into body rotation — tilting your head must not roll the character. Yaw is the only shared axis. `eye_to_head` is a pure ±IPD/2 translation along head-local X with no rotation; each eye still needs its own asymmetric projection.

## Pitfalls

- **No synthetic neck model on 6DoF.** The rig's head-bone parent already provides the pivot; adding a neck model on top double-counts the motion. (Such models exist for 3DoF phone VR — disable on 6DoF.)
- **No smoothing on the HMD→camera path.** Any lag between the rendered viewpoint and real head motion is nauseating. Body yaw can be smoothed; the head path must stay direct.
- **Baked head animation fights the HMD** — the biggest engineering gotcha for a rigged-character game. JC3's rig drives the head bone (idle sway, look-at, cutscene turns, recoil); in first person the HMD must own head orientation and roomscale position, so suppress or blend out the head-bone animation, or apply it only below the neck. This is exactly what UEVR's Freeze Rotation/Position toggles exist for, and it is the key RE target here: find where the rig writes the head bone / camera so it can be overridden.
- **Positional tracking through geometry.** Roomscale head translation is physical and can't be hard-collided — freezing the view while the real head moves is itself nauseating. Mitigate with body placement (so the head rarely reaches walls) and a fade on deep penetration.

## RE notes (release i64)

The camera hook writes position only — the translation columns of `m_TransformF` — plus a hardcoded ~90° FOV; rotation is never written. Before writing rotation, clear the coordinate-frame gate (the §15.7 experiment in `docs/vr-runtime.md`, blocker 3). Vehicles take a different path: `PushRenderContext` (`0x1407ECB00`, verified) has an `IsInDrivingVehicleState` branch that routes the transform differently (a raw matrix, bypassing the jitter freeze), so vehicle head-look behaves differently from on-foot — confirm the cockpit head-bone position is sane (not clipped to seat or world origin) by logging `m_CameraTransform` translation against character world position in a vehicle.

## Options to expose

Movement direction (head / initial-head / hand / initial-hand); turn mode (snap 30/45/90 versus smooth with adjustable speed, snap-fade on/off); comfort vignette (off/subtle/strong, separate move-versus-turn toggles, acceleration-keyed); movement-speed scalar; seated versus roomscale with height calibration; per-mode vignette overrides; wingsuit camera-roll-on-bank (default off).
