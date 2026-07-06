# Motion controllers and roomscale: scope

The plan for turning the VR runtime (issue #12) into a hands-first VR game: motion-controller input, per-hand aiming (left-hand grapple and right-hand gunfire at independent targets, dual-wielded guns at two targets), controller-held weapons, and roomscale locomotion. Grounded in three pipeline recon docs — `docs/engine/aim-pipeline.md`, `docs/engine/grapple-pipeline.md`, and `docs/engine/hands-and-roomscale.md` — and the extracted default gamepad keymap (`settings/keymap_gamepad.bin`, an RTPC container).

Decisions taken up front: weapons are virtual guns in the hand (not laser-pointer arm IK); aim assist is kept but re-derived mod-side per ray, softened; roomscale root motion is in scope; the grapple keeps the game's semantics with the target ray re-sourced to the left hand, with a designed path to gestures later. Both Touch-style and Index controllers are supported via OpenXR suggested bindings.

## Why this decomposes cleanly

The recon found that the game's own architecture separates almost everything we need to separate:

- **Aim state is per-consumer.** `CPlayerAimControl` buckets its single camera raycast by target type: weapons index 0, melee 1, grapple 2. Every grapple consumer — hook fire, the grapple reticle, the fire-arm IK — reads slot 2. Overwriting slot 2 from a left-controller raycast splits the grapple from the guns without touching the shared machinery.
- **Fire direction is one vector per weapon.** `CWeaponBase::GetShotMatrix` builds each shot as muzzle-bone origin toward `m_AimTargetPosition` (+0x3FC). Substituting that vector per weapon from a controller ray gives true per-gun aiming; the origin follows the rendered weapon automatically.
- **The weapon follows a hand bone.** Wielded weapons ride dedicated attach bones (`ATTACH_HAND_RIGHT`/`ATTACH_HAND_LEFT`) resolved through the same `SetJoint` machinery the head override already uses — a controller-posed hand bone carries the gun, its muzzle, and its effects with it.
- **The write path makes controllers native.** `ForceSetPressed`/`ForceSetClicked` on action IDs means the game reads VR input as native input. One correction from the semantic layer, though: while raw actions `FIRE_LEFT`/`FIRE_RIGHT` exist (bound to LT/RT), the game's mode-partitioned button layer (`CButtonMapping::EMapping`) exposes only `MAPPING_FIRE_RIGHT` on foot — dual-wield alternates barrels internally off a single fire input, and `FIRE_LEFT` is consulted only in vehicle/mounted contexts. Per-hand trigger firing is therefore a mod-side per-barrel intervention (phase 4), not an input mapping.
- **Roomscale rides the engine's own character physics.** The on-foot locomotion task solves a world velocity every tick (idle included) and writes it to the Havok character proxy, which handles collision, stairs, and slopes. Adding a chase velocity to that write moves the real capsule with the player's real body, with the engine doing the hard part (see "Roomscale design").

The counterweights: auto-aim is computed upstream on the camera ray (controller rays get none natively — mod-side magnetism replaces it), dual-wield is one weapon object with two alternating barrels sharing one target (a per-shot target swap is needed), the aim *state* flags (`m_AimFlags`/`m_AimingWeapon`) are singular rather than per-hand (weapon-raise gating is a shared problem), and the right-arm aim IK also drives the head effector (must be suppressed so the HMD owns the head).

## Phases

Each phase is independently shippable behind config and playtestable in the headset.

### 1. Controller input foundation

OpenXR action sets — `onfoot`, `vehicle`, `airborne`, `ui` — with one active per frame from the mode detection the headpose latch already does. Grip/aim pose actions for both hands. Suggested bindings for the Touch and Index interaction profiles; the runtime's rebinding UI covers per-user remaps. Output flows through `LocalPlayerActionMap::ForceSetPressed`/`ForceSetClicked` after `InputDeviceManager::Update`, per the timing rules in `docs/engine/input.md`. Deliverable: the whole game playable with controllers acting as a wearable gamepad — no pointing yet, but no gamepad in hand either.

The binding translation is deliberately congruent with the extracted default map (right trigger = fire, exactly as the pad's RT; the left trigger keeps the pad's LT grapple-retract role on foot; face buttons keep the game's clusters). A phase-1 RE task recovers the `CButtonMapping` mapping→action table (data-built; `CPlayerActionObserver` keys the button-hint prompts off it, which VR prompts will eventually want too). VR deletions fund the gaps: `PRECISION_AIM`, `LOOK_*`, `VEHICLE_CAM`, and `LOOK_BACK` dissolve into the headset, freeing the right-stick click for `THROW_GRENADE` (the pad's RB). The left grip takes `FIRE_GRAPPLE` (the pad's LB); the left trigger keeps the pad's own `FIRE_LEFT` + `RETRACT_GRAPPLE` overload. The right grip and left-stick click stay reserved for the gesture layer.

### 2. The aim split

- **Grapple → left hand**: post-hook `CPlayerAimControl::UpdateDirectAim`, re-cast the grapple ray from the left-controller pose, overwrite slot 2 (`m_AimPos[2]`, `m_DirectTargets[2]`, hits/range flags, and the grapple cache). The hook fire, zip/tether semantics, grapple reticle, and fire-arm IK all inherit the controller ray.
- **Guns → right hand**: write `m_AimTargetPosition` per weapon from the right-controller ray at the per-weapon aim update. Mod-side magnetism: score the game's own candidate targets against the controller ray and nudge the written point, with a strength scalar (replaces the camera-coupled native assist).
- **Second reticle**: the grapple reticle follows slot 2 natively; the weapon reticle becomes a mod-drawn quad (the `hud/cursor.rs` machinery) projected from the weapon's aim point with recorded depth.
- Config fallback to gaze aim per consumer, so regressions are a toggle away.

### 3. Hands and guns

Hand-attach bones (`ATTACH_HAND_RIGHT`/`LEFT`) driven to the controller poses via `SetJoint` in the existing character-hook seam — the gun renders in the hand. Arm IK from the shipped pattern (`NRightArmAimIK`-style effector writes on the pass the mod already feeds), both arms, with the head-effector write suppressed. Wrist position effectors place the hands; the open animation question is two-handed weapons (rifle fore-grip vs a one-hand hold) — start one-handed-everything and evaluate. Verify muzzle-origin coherence: shots must originate at the rendered (controller-held) muzzle.

### 4. Dual-wield split

Two interventions on one weapon object. Direction: key the `m_AimTargetPosition` write to which barrel fires next, writing the left- or right-controller target accordingly. Fire: on foot the game exposes a single fire input and alternates barrels internally (`MarkNextWeaponComponentForFire`), so per-hand triggers need the mod to drive barrel selection — left trigger forces the left fire-point and fires with the left target, right trigger the right. Moderate: the alternation function and fire dispatch are identified in `docs/engine/aim-pipeline.md`; the open question is whether forcing the same barrel on consecutive shots upsets ammo/recoil/effects bookkeeping.

### 5. Roomscale

The full design is in "Roomscale design" below; the engine answers live in `docs/engine/hands-and-roomscale.md` §4. In brief: a per-tick velocity nudge chases the body under the player's physical head through the engine's own character physics, facing stays head-owned, crouch ships in stages, and vehicle seating is a pause-and-confirm transition.

### 6. Radial menu and the gesture path

The pad's d-pad is direct slot select (`SELECT_DUEL_WIELD`/`TWO_HANDED`/`TWO_HANDED_SPECIAL`/`EXPLOSIVES`) — a four-sector radial plus explosives, rendered on the floating-panel machinery, pointed by hand ray, on a held face button. This is the pressure valve for the input-surface deficit (VR has ~12 comfortable inputs against the pad's ~16; action sets, deletions, and the radial close the gap). The gesture layer lands afterwards, each gesture retiring a button: over-shoulder holster = weapon switch first (the scarcest surface), then chest reload, physical grenade throw, and grapple pull-to-reel.

## Risks, ranked

1. **Singular aim state** — `m_AimFlags`/`m_AimingWeapon` gate weapon raise, reticle, movement mode, and auto-aim as one state, not per hand (the fps-movement-aim-coupling lesson). Per-hand raise/lower may not be separable; acceptable v1: aiming state is "any hand aiming".
2. **Animation fights** — controller-posed hands vs the authored animation set (recoil, reloads, traversal moves). The hand override likely needs state-aware gating (yield during reloads/mounts), which is tuning-heavy.
3. **Reel-in and traversal states** — `GHS_REELED_*` states carry their own animations and camera behaviour; how they read under a controller-aimed grapple and a free head needs playtesting; the reel-in arm IK reads the live aim slot and needs an ownership decision.
4. **Body-turn reactions** — aim-relative body turning reacts to aim points (`GetAimMoveAngle`, already hooked for the head); off-axis hand targets must not spin the body.
5. **Two-handed weapons** — grip topology vs one controller per hand; deferred by design.
6. **Vehicle per-class inputs** — heli/plane/boat each have axis sets; the extracted keymap maps them onto two sticks + triggers cleanly, but each class needs its own playtest pass.

## Suggested milestone split (issues)

1. Controller input foundation (phase 1) — unlocks headset-native play immediately.
2. Split aiming: grapple-left, gun-right (phase 2) — the marquee mechanic.
3. Controller-held weapons and arm IK (phase 3).
4. Dual-wield independent targets (phase 4) — small, gated on 2+3.
5. Roomscale locomotion and vehicle seating (phase 5).
6. Radial menu and first gestures (phase 6).

Phases 1–2 are the highest value-to-risk; everything downstream of them reuses their seams. The pipeline docs carry the addresses and the open questions per area.

## Per-mode input tables

Derived from the game's own mode partition (`CButtonMapping::EMapping` sections) crossed with the extracted default gamepad keymap. Semantic actions, not raw action IDs — the raw table recovery is a phase-1 task. VR bindings are the draft defaults for a Touch-style layout; Index shares the topology (its extra inputs — touchpad, finger curl — stay unbound until the gesture layer). "Gesture" marks the designed successor to a button. Entries marked *(verify)* need the recovered mapping table or a playtest to confirm the game-side binding.

### On foot

| Semantic action | Pad default | VR draft |
|---|---|---|
| Move / walk | L stick | L stick (magnitude = walk/run) |
| Body turn | R stick | R stick (smooth default, snap option) |
| Fire (incl. dual-wield) | RT | R trigger |
| Fire grapple | LB | **L grip** (grab the world) |
| Retract/reel tethers | LT | L trigger |
| Release / push tethers | B | B |
| Throw grenade | RB | R stick click → gesture (physical throw) |
| Plant / detonate explosive | RB (context) | R stick click (context) |
| Reload | X | X → gesture (chest tap) |
| Jump / stunt / parachute | A | A |
| Use item / enter vehicle / open wingsuit | Y | Y |
| Melee (`MAPPING_HAMMER`) | *(verify)* | gesture candidate (swing); interim shares R stick click *(verify)* |
| Weapon slot select ×4 | d-pad | radial menu on L stick click (four sectors + explosives) |
| Holster (contextual) | *(verify)* | radial center → gesture (over-shoulder) |
| Precision aim | R3 | **deleted** — physically aim |
| Sniper zoom | *(verify)* | gun raised to eye *(design)*; interim A *(verify)* |
| Reel-in context (cancel, boost, slingshot, hang jump) | A/B contextual *(verify)* | A/B contextual, same as game |

### Land vehicles (car; motorcycle variants in italics)

| Semantic action | Pad default | VR draft |
|---|---|---|
| Steer / *lean-tilt* | L stick | L stick |
| Accelerate / reverse | RT / LT | R trigger / L trigger |
| Handbrake | X | X |
| Nitrous / turbo jump | *(verify)* | B *(verify)* |
| Fire vehicle weapon primary / secondary | RB / LB | R grip / L grip |
| *Fire personal weapon (motorcycle)* | RB | R grip, aimed by right hand |
| Enter gunner seat / stunt (roof) | *(verify)* | A |
| Exit vehicle | Y | Y |
| Horn | L3 | L stick click |
| Look back / vehicle cam / recenter cam | R3 | **deleted** — the neck and F7 |

### Helicopter

| Semantic action | Pad default | VR draft |
|---|---|---|
| Collective up / down | RT / LT | R trigger / L trigger |
| Cyclic (forward/back, roll) | L stick | L stick |
| Yaw | R stick X | R stick X |
| Fire primary / secondary | RB / LB *(verify)* | R grip / L grip |
| Exit / stunt / nitrous | Y / A / *(verify)* | Y / A / B |

### Plane

| Semantic action | Pad default | VR draft |
|---|---|---|
| Pitch / roll | L stick | L stick |
| Rudder | X / B | R stick X |
| Thrust up / down | RT / LT | R trigger / L trigger |
| Fire primary / secondary | RB / LB *(verify)* | R grip / L grip |
| Exit / stunt | Y / A | Y / A |

### Boats and jetskis

| Semantic action | Pad default | VR draft |
|---|---|---|
| Steer | L stick | L stick |
| Accelerate / reverse | RT / LT | R trigger / L trigger |
| Fire / *personal weapon (jetski)* | RB | R grip |
| Nitrous / turbo jump | *(verify)* | B |
| Exit | Y | Y |

### Wingsuit and parachute

| Semantic action | Pad default | VR draft |
|---|---|---|
| Steer | L stick | L stick |
| Air brake | *(verify)* | both grips *(design)* or L trigger |
| Boost (weaponized) / evade | *(verify)* | B / A |
| Fire weapon (weaponized wingsuit / parachute) | RT *(verify)* | R trigger, aimed by right hand |
| Open parachute / close | A | A |
| Grapple (slingshot boost) | LB | L grip |

### UI

The floating panel plus the virtual cursor already exist; the VR-native upgrade is a hand-ray laser pointer with trigger as click, B as cancel, and the stick for lists — the `ui` action set replaces all gameplay bindings while a menu is up (`END_UI_MAPPINGS` marks the game's own boundary for this).

## Roomscale design

The handoff-complete design for physical locomotion, resolved against the engine answers in `docs/engine/hands-and-roomscale.md` §4. The governing principle (Boneworks topology, `docs/mod/head-and-body.md`): the head is ground truth and the body chases it — the camera is never derived from the body's motion, so the view stays 1:1 with the real head no matter what the body does.

### The chase loop

The camera is already placed from the measured HMD pose each frame (anchor + cockpit offset), so there is no explicit "consume the offset" bookkeeping — the offset is re-measured from live poses every frame, and anything the body fails to do simply leaves it nonzero. The loop, per sim tick:

1. Measure the body-frame XZ offset from the character to the player's physical head (the cockpit position the pose path already computes).
2. Outside a small deadzone (~10 cm), feed a chase velocity `clamp(offset / dt, max_chase_speed)` into the on-foot locomotion result — a post-call hook on the locomotion task, adding to the solved world velocity it writes to the character proxy (`docs/engine/hands-and-roomscale.md` §4: the task ticks every on-foot frame including idle, so walking from standstill works; the engine's own displacement evaluation is either/or between root motion and code-driven, so the add cannot double-count it).
3. The proxy solves collision, stairs, and slopes. No read-back step: next tick's offset measurement reflects whatever actually happened. A wall between the player and the body leaves a persistent offset — the view stays with the real head, and a `CastRaySimple` probe drives a comfort fade on deep head-through-geometry penetration.
4. Physical-walk and stick-walk compose in one channel: the chase velocity scales down with stick magnitude so deliberate stick locomotion wins, and both express as the same body-local move vector.

Facing: **physical translation never rotates the body** (the settled VR convention — VRIK-family games derive facing from the head, never from travel). The chase velocity is body-local; JC3's aim-relative strafe blend space, already forced by the FPS-movement shim, animates sidesteps and backpedals while the body-follows-head deadzone scheme owns the torso yaw.

Teleports: when `CCharacter::m_NumFramesSinceTeleport == 0` (one int read; every warp writer resets it), the tick re-bases the cockpit baseline instead of feeding a kilometre-long chase spike.

Recenter on foot re-bases **yaw and height only**: the chase loop keeps position converged, so there is no position to recenter, and recentering must never teleport the body through a wall the chase could not cross. Position re-basing exists only in seat transitions.

### Crouch, in stages

1. **Visual crouch** (ship first, config-gated): a low head target with the hips-translation and floor-contact solve steps enabled folds the body through the same HumanIK pass the head target already uses — feet planted, hips drop.
2. **Capsule honesty**: the engine swaps the proxy capsule per state itself (`SetProxyState` → `hknpWorld::setBodyShape`), so a shorter crouch capsule rides the engine's own mechanism rather than raw Havok surgery. Gate on the visual crouch proving out.
3. **AI honesty**: whether enemy perception respects a lowered head is playtest-only; measured in the headset, not designed in advance.

### Seat transitions (roomscale mode only)

Entering a vehicle pauses the world but never the view — a static frame under head motion is instant sickness, and the game's own pause state freezes the camera seams the mod depends on (`UpdateRenderPaused` never runs `CameraTree::UpdateRenderContexts`). The freeze is therefore a mod-side game-clock freeze through the already-hooked `CClock::Update` (zero game dt, skip the sim tick, never enter the paused sub-state), keeping the render, headpose, and camera paths fully live.

1. Enter-vehicle plays the game's own animation; the player watches.
2. When the seat-lock engages (the attach machinery), the clock freezes and the floating panel prompts: take your seat, confirm when ready.
3. On confirm: re-base the cockpit to the seated head reference — the character's own head/eye bones riding the vehicle through `m_AttachBone` · `m_AttachOffset` — unfreeze, and activate the `vehicle` action set. Roomscale chase disengages while seat-locked.
4. Exit reverses: animation completes, freeze, stand-and-step-away prompt, re-base to standing, chase re-engages.

Seated-mode players skip the ceremony entirely (instant auto re-base). Open aesthetic choice: whether the frozen world dims/vignettes to signal stopped time, or stays raw (bullets hanging mid-air) — a config default to be picked in the headset.
