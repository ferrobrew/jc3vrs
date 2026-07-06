# Hands, arms, and roomscale

Scoping VR motion controllers for JC3VRS: virtual hands that hold weapons, both arms aimable at
independent targets (right gun one way, left grapple another), and roomscale locomotion where
physically walking moves the in-game character. This is a reverse-engineering recon of the three
systems that would carry those features — the weapon-attachment (prop) chain, the shipped per-arm
aim IK, and the character root/collision-capsule movement path — with feasibility verdicts and the
interface points with the aim and grapple pipelines.

All addresses are release-build RVAs (`JustCause3.exe`, 2026 no-Denuvo IDB), read from the release
decompile; the 2016 symbol dump was the locator only. This is design/RE notes, not shipped code.

Related: `docs/engine/humanik.md` (the solver, effector ids, the injection seam), `docs/engine/skeleton.md` (the
Joint API and pose pipeline), `docs/mod/head-and-body.md` (the "Boneworks alignment" topology this
implements). Interface points with the concurrent aim and grapple work are flagged inline.

## 1. Weapon-to-hand attachment (the props system)

### The weapon model follows a hand *safe bone*, not `UpdatePropEffects`

`Character::UpdatePropEffects` (**0x1407C2380**) is misleadingly named for this purpose. In the
release build it does three things: a head-bone `SetJoint` (the `0xA877D9CC` = `HEAD` safe bone —
this is the seam the mod already hooks), then `CInventory::UpdateAttachmentEffects`
(**0x1408C6D30**), then `UpdateBalloonHead`. It does **not** compute the wielded weapon's transform.

The weapon model's placement is a *bone attachment*. Each weapon carries, per weapon-state, a
`NBone::ESafeBoneIndex` telling it which bone to ride:

- `CWeaponBase::GetCurrentBoneAttachement(weaponIndex)` returns
  `m_lBoneAttachementPerWeaponState[m_State]` — one of the `ATTACH_HAND_*` safe bones.
- `CWeaponBase::ChangeBoneAttachement(bone, weaponIndex)` (**0x140958AF0**) rebinds it.
- `CWeaponBase::IsLeftHanded` is literally `bone == ATTACH_HAND_LEFT`.

The relevant safe-bone hashes (`NBone::ESafeBoneIndex`):

| Bone | Hash |
|---|---|
| `ATTACH_HAND_RIGHT` | `0x65C5D2EB` |
| `ATTACH_HAND_LEFT` | `0x4190BFF7` |
| `ATTACH_HAND_RIGHT2` | `0x7BF80F49` |
| `ATTACH_HAND_LEFT2` | `0x5DF39D10` |
| `RIGHT_HAND` / `LEFT_HAND` | `0x69E77FA6` / `0x57C83F95` |

So the transform chain is: **weapon world ← attach/grip offset ← hand attach bone ← character
world**. The `ATTACH_HAND_*` bones are dedicated attach points (grip offset already baked into the
bone's rest frame relative to the wrist), which is why a weapon sits in the grip without a separate
offset matrix in the common path.

### Where the weapon's world transform is computed per frame

`CWeaponBase::GetTransform` (**0x140964E50**) returns `CWeaponData::GetWorldMatrixT1`
(**0x1409601E0**), which reads the weapon's own **cached** world matrix at `CWeaponData+0x198`
(optionally multiplied by a uniform scale when a size flag `== 3`). That cache is refreshed each
frame from the attach bone by the model-instance attachment update (the weapon is a child model
instance whose parent is the hand attach bone). Everything downstream — the render submit, the
`GetIPfxInstanceClosestTo` physics query — reads this cached `T1`.

### Overriding the weapon to follow a controller

Two clean override points, in increasing order of surface area:

1. **Override the hand attach bone.** `SetJoint(ATTACH_HAND_RIGHT, desiredGripModel)` in the same
   post-finalize seam the head override uses (`UpdatePropEffects`, or the pre-IK finalize hook).
   Everything that reads the weapon transform — model, muzzle effects, and the weapon's own physics
   proxy — derives from this bone, so they all move together and stay self-consistent. This is the
   recommended path: it's the same `SetJoint` machinery already proven for the head, and it does not
   touch fire logic (below). The arm IK (§2) that would otherwise fight it is a *separate* concern:
   with controller-driven hands you would drive the arm to the controller instead of to the aim
   target (or disable the game's arm IK), so there is no conflict to reconcile.
2. **Override `CWeaponData`'s cached `T1`** (`+0x198`) directly, or hook `GetWorldMatrixT1`. This
   moves the rendered gun but leaves the hand bone (and thus the hand mesh and the arm IK's grip)
   where the animation put it, so the hand and gun visibly separate. Only useful if the hand is
   hidden. Not recommended.

### Dual-wield: one weapon object, per-gun attach bones

A dual-wield slot (`E_SLOT_DUAL_WIELD`) is **one `CWeaponBase`** holding a *vector* of `CWeaponData`
(`m_lWeaponData`, one entry per gun), not two weapon instances. Each gun carries its own
`m_lBoneAttachementPerWeaponState[m_State]`, indexed by `weapon_index`:
`GetCurrentBoneAttachement(weapon_index)` (**0x140CB58F0**) returns *that gun's* attach bone for the
current weapon state, and `ChangeBoneAttachement(bone, weapon_index)` (**0x140CB58B0**) rebinds one
gun's bones independently (it writes all 11 weapon-state slots). The second gun rides the `…2` attach
bones — the equip code (`NGSONodes`) assigns `ATTACH_HAND_RIGHT` / `ATTACH_HAND_RIGHT2` to gun 0/1 on
the right, `ATTACH_HAND_LEFT` / `ATTACH_HAND_LEFT2` on the left.

Two consequences for controller-driven hands:

- **Placement is fully splittable.** Each gun has an independent attach bone, so `SetJoint` on gun
  0's bone → right controller and gun 1's bone → left controller places the two pistols in two hands
  pointing two ways, with no shared transform to fight. Combined with the per-barrel aim-target write
  (`docs/engine/aim-pipeline.md`, dual-wield) each gun fires at its own hand's target.
- **State is shared, and that is the seam.** The whole pair has one `m_State`, one aim-flag, and one
  reload/equip/holster animation; `GetCurrentBoneAttachement` is gated on that single `m_State`. So
  the override is clean only in the free-aim states (`E_WS_WIELDING`, `E_WS_DUAL_WIELDING`) — when
  `m_State` leaves them, a single canonical two-gun animation drives both hands, and holding the
  attach-bone override through it looks broken if the hands are far apart. The fix is a state-gated
  handoff: yield the hands to animation control outside the wielding states (let the reload play, the
  hands reconcile to the scripted pose), and reclaim them to the controllers on return. Dual-wield is
  the most visible instance of the general "yield during scripted animations" rule (§ feasibility
  verdicts), because two independently-posed hands have the furthest to travel back.

### Muzzle-flash / shell effects and fire logic — the aim-pipeline interface point

The wielded-weapon *effects* (muzzle flash, fire particle, casings, sounds) live in the
`CAttachedEffectContainer` and are updated by `CAttachedEffectContainer::UpdateAttachedEffects`
(**0x140466F30**), per-instance via `SAttachedInstance::UpdateTransform` (**0x140453D00**). Each
attached instance resolves its transform through a **virtual resolver** (a vtable call that reads a
bone/socket on the *weapon* model — the muzzle bone), copies the 0x60-byte transform block, applies
its transform modes, and derives a per-frame velocity (`Δposition · 30`, i.e. the position delta
scaled to the fixed tick) for the effect's own motion.

Two consequences for the aim pipeline:

- **The muzzle transform is derived from a bone on the weapon model** (resolved relative to the
  weapon's cached `T1`, which rides the hand bone). Override the hand bone (option 1) and the muzzle
  flash follows for free — it is downstream of the same cache. Overriding only `CWeaponData T1`
  (option 2) also carries the muzzle, because the resolver composes on that matrix.
- **Bullet direction is NOT the muzzle transform.** Fire logic raycasts from the aim camera / aim
  control, not from the muzzle bone (the muzzle is cosmetic). This is the clean seam with the
  **aim-pipeline agent**: moving the *rendered* gun to a controller does not change where shots go —
  that stays entirely on the aim pipeline's camera/aim-control path (`GetCameraMatrix`,
  `CPlayerAimControl`, see `docs/engine/aim-pipeline.md` "The camera getters"). If controller-relative firing is
  wanted, that is a change to the aim pipeline's ray origin/direction, decoupled from this section.

## 2. Per-arm aim IK — pointing the arms

The game ships **two** per-arm aim-IK systems, both player-only, both feeding `CHumanIK` rotation
effectors on `PASS_MAIN` at `SolveStep::UPPER_BODY` (7), both gated on the same blackboard aim-IK
weight `0xE81C147E`. There is **no generic left-arm aim IK** — the second system is the grapple
reel-in arm.

### Right-arm aim IK (the gun arm) — three effectors, including the head

`NRightArmAimIK::Update` (**0x140845B10**) runs only for `CCharacter::IsPlayer`. It reads the aim
weight (blackboard `0xE81C147E`), builds `inverse(m_WorldMatrixT1)` (world→model), brings the aim
target into model space, then loops over **three** effectors, calling
`NRightArmAimIK::UpdateAimEffector` (**0x140838EC0**) for each on `m_HIK` (`CCharacter+0x10E0`):

```
struct NRightArmAimIK::SInstanceProperties { CVector3f m_PoleVector; SAimEffectorProperties m_AimEffectors[3]; };
struct NRightArmAimIK::SAimEffectorProperties { int m_BoneIndex; CVector3f m_BonePosition; float m_Gain;
                                                CVector3f m_AimDirection; CVector3f m_RotationAxis; };
enum EAimEffectorTypes { AET_RIGHT_ARM = 0, AET_RIGHT_HAND = 1, AET_HEAD = 2 };
```

So the gun-aim IK drives the **right arm, the right hand, and the head** toward the aim target
(per-effector gains in `dword_142D66078/7C/80`). `UpdateAimEffector` computes the current→target
rotation (axis + `acos` angle) and calls `AddEffectorTargetRotation` (axis-vector overload,
**0x140838EC0** uses `0x140408BB0`) with `SolveStep=7`, `Pass=0` (MAIN), then writes
`m_TargetReachR[effector]` (`CHumanIK+0x330`, dword index `+204`).

**Interaction risk (own head effector).** Because the gun-aim IK drives the **head effector (15)**
itself, it competes with the mod's head override and the body-IK head target from `docs/engine/humanik.md`.
When the player aims, the game already turns the head toward the aim reticle. The VR head must win;
the aim IK's head effector should be suppressed (drop `AET_HEAD` from the loop, or zero its reach)
when the headpose owns the head.

### Reel-in aim IK (the grapple arm) — one arm effector + chest — grapple-pipeline interface point

`NReelInAimIK::Update` (**0x140841D70**), also player-only and gated on `0xE81C147E`, drives a
**single** arm effector (`SInstanceProperties` has `m_AimEffectors[1]`) via
`NReelInAimIK::UpdateAimEffector` (**0x140838A80**), plus a separate **chest-end** rotation. Its
distinguishing behaviour: it reads `CCharacter::GetGrapplingHook` (**0x140760830**) and aims the arm
and chest at the **hook's world position** (not the aim camera). It uses
`NAnimationSystem::CHumanIK::GetChestEndEffectorId` (**0x1403BCDD0**) for the chest target and
`NAimIKUtil::GetIKWeightValue` (**0x140802BA0**) for the reel-in weight envelope. The arm reference
bone is `RIGHT_ARM` (`0x19D4B6CF`); it also samples `SPINE` (`0xE28C84B`).

This is the **grapple-pipeline interface point**: the reel-in arm already points at
`GetGrapplingHook`, so a controller-driven left/off-hand grapple would either (a) replace the hook
position the reel-in IK reads, or (b) drive that arm's effector from the controller ray the same way
`UpdateAimEffector` does. The grapple *targeting* (where the hook fires/attaches) is the grapple
agent's domain; this section only owns the *arm pose* that visualizes it.

### Who activates them, and can the mod drive both from arbitrary directions

Both `Update` functions are registered as **animation state tasks** — the only xrefs are data
references from task-descriptor tables (`0x142a3c204`/`0x142a3bbf4` and the `0x1430xxxxx` mirrors),
i.e. they run when the corresponding state is active in the character state machine (weapon-aim
state → right-arm IK; grapple reel-in state → reel-in IK). Both queue their targets *during*
animation-graph evaluation, well before the `HasTargets(PASS_MAIN)` gate in
`UpdatePassFinalizePose_Parallel` (`docs/engine/humanik.md`), so their targets and the mod's head target
coexist on the same solve (the pass step is the max; effectors are keyed by id).

**Driving both arms from controller rays is feasible and low-risk**, using the exact pattern the mod
already uses for the head effector (`payload/src/hooks/character.rs`,
`AddEffectorTargetRotationVector` on `PASS_MAIN`):

- Queue an `AddEffectorTargetRotation` for the right-hand/right-arm effectors from the right
  controller ray, and for the reel-in arm effector from the left controller ray, each with its
  `m_TargetReachR`. Rotation effectors on distinct effector ids do not collide.
- **Hand *position* effectors** can place the wrists at controller positions outright:
  `GetEffectorIdFromBoneIndex(m_HIK, wristBoneIndex)` yields effector **3 (left wrist)** / **4 (right
  wrist)** (see the effector table in `docs/engine/humanik.md`), then
  `AddEffectorTargetPosition(effector, posModel, …)` + `m_TargetReachT[effector] = w` on `PASS_MAIN`
  or `PASS_SECONDARY`. `PASS_SECONDARY` is the game's own hand/grip pass
  (`UpdateSecondaryHandIKPass`), so the game already does exactly this for grip — putting the wrist
  where a controller is fits the existing machinery.
- **Conflict with the wielded weapon's animation set.** A hand position/rotation effector that
  disagrees with the animation's grip fights the weapon-hold pose: the wrist is IK-pulled to the
  controller while the animation still drives the fingers/forearm and the weapon rides the *attach*
  bone (§1). Keep them consistent — if the hand is IK-driven to the controller, the weapon should
  ride that same hand (override the attach bone to match, §1 option 1), and blend the game's grip
  weight down. Driving a hand effector while the animation set expects a two-handed hold on the same
  arm is the untested case worth playtesting.

## 3. Roomscale root motion

The ask: translate the character root + collision capsule by small per-frame XZ deltas (physical
walking) while respecting collision, stairs, and slopes, and disable it in vehicles.

### Where the world transform is authoritative

`CCharacter::m_WorldMatrixT0` / `m_WorldMatrixT1` are the authoritative pair (release offsets
`CCharacter+0x27F0` / `+0x2830` = 10224 / 10288 — confirmed by the aim IK's `+10288` reads).
`GetWorldMatrix`/`GetWorldMatrixRef` return `&m_WorldMatrixT1`. The render-facing
`m_GraphicsWorldT0`/`T1` (`CTransform`) are derived from the world matrices
(`SyncGraphicsMatricesToWorld` → `CTransform::FromMatrix4`), and the frame lerps T0→T1 by `dtf`.
Direct writers exist — `CCharacter::WriteWorldMatrix` (full matrix),
`WriteWorldMatrixTranslation` (translation only), `WriteWorldMatrixOrientation`
(**0x1408D73A0**) — but writing the world matrix directly is a **teleport**: it bypasses the
collision solve, so it is the wrong tool for roomscale walking (fine for a snap-turn recenter, §
below).

### The displacement path — ride this for collision-respecting roomscale

`NStateTask_LocoUtil::EvaluateCharacterDisplacement` (**0x14081AB90**) is the per-tick movement
producer, and it is a **pure velocity function**: given the character, its new transform, and flags,
it fills `wanted_velocity_ws` (world) and `wanted_velocity_ls` (local) — it does **not** write the
root. Its default path reads the animation's `GetRawRootVelocity` and rotates it into world space;
its code-driven path instead reads a target direction from the blackboard (`0x7DF24A88`, with a
previous-dir `0x370A3A61` and a marker `0xE844061C`) and is enabled by the `m_CodeDriven*` flags
(`m_CodeDrivenDisplacement`, `m_AllowCodeDrivenDisplacementUntilTolerance`,
`m_EnableCodeDrivenDisplacementDuringBlend`, and `m_AngleCorrectionEnabled` /
`m_MoveActionParams.m_AngleCorrectionRequestedDir`). It is called by
`NStateTask_MovementLocomotionTask::Update` (**0x140829E80**, on-foot),
`NStateTask_MovementStuntingTask`, and `NPhysicalAnchorWarpTask`.

The wanted velocity is then handed to the **Havok character proxy**:
`CPfxCharacterInstance::CCharacterInput::SetWantedVelocity` (**0x14075FD90**) feeds the character
proxy input, the physics step solves it against collision (stairs/slopes/walls), and the character's
`m_WorldMatrixT1` is written from the solved proxy position. `CPfxCharacterInstance` is the character
proxy wrapper (`m_Avatar` is the `CAvatar`; `m_PendingProxyState`/`m_DefaultProxyState` pick the
capsule/quadruped shape). `SetWantedVelocity`'s other callers are the grapple/jump/stunt/ragdoll
tasks that drive the proxy directly.

**Roomscale can ride the displacement path so collision is respected for free.** Two ride points —
but note before choosing: §4.1 below supersedes option 1. The on-foot path does *not* go through
`SetWantedVelocity` (that channel serves the proxy-driven states); the on-foot task writes its solved
world velocity directly to `CPfxCharacterInstance + 0x3C`, and the roomscale add belongs there.

1. **Additive wanted velocity** — superseded by §4.1: add `roomscaleDeltaXZ / dt` to the wanted
   velocity. Originally scoped against `SetWantedVelocity`; the verified on-foot seam is the
   post-task write to the proxy-input velocity at `+0x3C`. The physics proxy walks the extra
   distance and resolves collision either way; the capsule follows because the proxy *is* the
   capsule.
2. **Code-driven displacement direction** — set `m_CodeDrivenDisplacement` and publish the roomscale
   direction to blackboard `0x7DF24A88`, letting `EvaluateCharacterDisplacement` produce the wanted
   velocity. Heavier (interacts with the game's own code-driven displacement users) and mainly a fit
   if roomscale should compose with authored displacement.

Ride point 1 is the recommendation: it is the same seam the game itself uses to move the proxy, it
is collision-correct by construction, and it is a small per-frame add rather than a state-machine
change. The residual XZ error between the player's real position and the (collision-clamped) capsule
is the "positional tracking through geometry" pitfall from `docs/mod/head-and-body.md` — mitigate with a
fade on deep penetration, not a hard freeze.

### Vehicles — roomscale must disable (seat-lock)

In a vehicle the character is **attached**: `CCharacter::m_Attachable` (`CAttachable`),
`m_attachType` (`CCharacter::AttachType`), and `m_attachedObject` hold the parent binding; the
`AttachTo` virtual (vtable slot `+320`, wrapped by `SetYAlignedAttachTo` **0x14079D540**) sets it,
and while attached the character's transform is parented to the vehicle seat rather than driven by
the character proxy. Roomscale locomotion must
be gated off whenever the character is seat-attached (`m_attachType != NONE` / the in-vehicle state):
the body is fixed to the seat, the head stays free (this is the "easy case" from
`docs/mod/head-and-body.md` — no body-yaw decoupling in vehicles), and adding wanted velocity to a
seat-locked character would fight the parent transform.

### Teleport / warp API (for reference — not the walking path)

For discrete relocation (snap-turn recenter around the head, or a comfort warp), the direct world
matrix writers above are the mechanism, and the engine's own `CTeleport` object
(`SetTransform` **0x1406A5800**, `GetTransform`/`NeedsUpdate`, event-driven) plus the animation
warp tasks (`NPhysicalAnchorWarpTask`, `NStateTask_MovementHeightWarpTask`) exist for scripted
warps. These bypass collision by design and are appropriate only for instantaneous moves, never for
continuous roomscale.

## Feasibility verdicts

**(a) Controller-held weapon rendering — EASY.** The weapon rides a dedicated hand attach bone
(`ATTACH_HAND_RIGHT`/`LEFT`) via the same `SetJoint` machinery the mod already uses for the head;
`CWeaponData`'s cached `T1` and the muzzle effects both derive from it, so overriding one attach bone
moves gun + muzzle flash together and consistently. Fire direction is on the aim pipeline, not the
weapon transform, so nothing about shooting breaks. The only care is keeping the hand mesh with the
weapon (drive the same attach bone, not the weapon cache).

**(b) Dual independent arm aim IK — MODERATE.** The pattern is proven (the mod already queues a head
rotation effector on `PASS_MAIN`), and the two shipped systems (`NRightArmAimIK`, `NReelInAimIK`)
show the exact call shape for per-arm rotation effectors plus optional wrist *position* effectors
(ids 3/4). Driving both arms from independent controller rays is a straightforward extension. What
lifts it above "easy": the right-arm IK **also drives the head effector** (must be suppressed so the
HMD wins), and a hand effector that disagrees with the wielded weapon's animation grip needs the
weapon attach bone driven to match and the game's grip weight blended down — an untested interaction
worth in-headset validation.

**(c) Roomscale walking via the displacement path — MODERATE.** The clean ride exists: add roomscale
XZ velocity to what the on-foot locomotion task feeds `SetWantedVelocity`, and the Havok character
proxy resolves collision/stairs/slopes for free — no bespoke physics. What keeps it from "easy":
correctly scoping the add to on-foot-only states (disable in vehicles via `m_attachType`, and in
the many proxy-driven states — grapple, jump, stunt, ragdoll — where other tasks already own the
wanted velocity), tuning the `dt` mapping so the proxy speed reads as 1:1 with real walking, and
handling the capsule-clamp residual (the through-geometry fade) rather than freezing the view.

## Open questions

- **Head-effector arbitration.** The gun-aim IK (`NRightArmAimIK`, `AET_HEAD`), the mod's head
  `SetJoint`, and the body-IK head target (`docs/engine/humanik.md`) all touch the head. Exact suppression
  order for the aim IK's head effector when the HMD owns the head is untested.
- **Reel-in ↔ grapple targeting handoff.** Whether the controller-grapple should feed the reel-in
  IK a synthetic hook position (`GetGrapplingHook`) or bypass it and drive the arm effector directly
  — coordinate with the grapple-pipeline work.
- **Hand effector vs. two-handed holds.** Position/rotation effectors on a wrist while the animation
  set expects a two-handed weapon hold on that arm — the fighting case; needs playtest.
- **`SetWantedVelocity` add vs. animation root motion.** How an additive roomscale velocity composes
  with the animation's own root velocity (double-counting during a walk cycle) — likely wants the
  add gated to when the locomotion state is idle/near-idle, or the root motion scaled down.
- **`dt` and tick timing.** `EvaluateCharacterDisplacement` and the effect velocity both assume the
  fixed sim tick (the `·30` in `SAttachedInstance::UpdateTransform`); the roomscale delta must be
  mapped to that tick timeline, not the render frame (the same tick-vs-frame care as the headpose in
  `docs/mod/head-and-body.md`).
- **Capsule size in roomscale.** Leaning/ducking physically moves the head but the proxy capsule is
  a fixed shape (`m_PendingProxyState`); whether crouch/lean should reshape the capsule or only
  offset the head is unresolved.

## 4. Roomscale-locomotion handoff — closing the unknowns

Follow-up RE that resolves the six blockers for the roomscale-locomotion handoff. Same conventions:
release-build RVAs from the 2026 no-Denuvo IDB, 2016 symbol dump as locator only. Two of these
findings correct earlier assumptions in §3 and the open questions above — flagged inline.

### 4.1 Idle ticking — the locomotion task *does* run at idle (corrects an open question)

`NStateTask_MovementLocomotionTask::Update` (**0x140829E80**) runs **every on-foot sim tick,
including standing idle with no input** — it is not gated to an active move. The proof is by
xref: the only callers of `NStateTask_LocoUtil::EvaluateCharacterOrientation` (**0x14081F8C0**) are
this task, `NStateTask_MovementStuntingTask::Update` (**0x14082B440**), and
`NPhysicalAnchorWarpTask::Update` (**0x14083E210**). The mod's mode detector already observes its
`ORIENTATION_EVAL_CALLS` counter (the `EvaluateCharacterOrientation` detour) advancing on every
on-foot tick including idle (`docs/mod/head-and-body.md`); on-foot idle is neither stunting nor
warping, so the advancing counter *is* the locomotion task ticking. The "move/aim task counters stop
while idle" the mod saw are a narrower, action-specific task, not this one.

At idle the task still runs the full pipeline — `EvaluateCharacterSpeed` (**0x14081AB10**, ~0),
`EvaluateCharacterOrientation`, `EvaluateCharacterDisplacement` — and still writes a wanted velocity
(≈0) to the character proxy every tick. So the per-tick feed seam is live at idle; the mod does not
need an always-on task or a state-machine change.

**The on-foot feed does not go through `SetWantedVelocity` (corrects §3).** At its tail
(`0x14082A658`) the locomotion task writes its solved world velocity **directly into the character
proxy input at `CPfxCharacterInstance + 0x3C`** (a `CVector3f`: xy at `+0x3C`, z at `+0x44`; a byte
"has surface dir" flag at `+0x39`), where `CPfxCharacterInstance` is `CCharacter[1034]`
(`CCharacter + 0x2050`). It never calls `CPfxCharacterInstance::CCharacterInput::SetWantedVelocity`
(**0x14075FD90**) — that entry point is used only by the *proxy-driven* states (ragdoll get-up,
in-air grapple-fire, grappling-hang, stunting, jump, grapple-yank, upright), which write the
`CCharacterInput` sub-object at `+0x2934` instead. So there are **two distinct proxy-input
channels**: `+0x3C` (on-foot locomotion) and the `CCharacterInput` wanted-velocity at `+0x2934`
(everything else).

**Cleanest idle-and-walk feed:** hook `NStateTask_MovementLocomotionTask::Update` post-call for the
local player and add `roomscaleDeltaXZ / dt` into the just-written `CPfxCharacterInstance + 0x3C`
vector (re-normalizing the surface-dir flag handling is unnecessary — the physics step reads the raw
vector). This is the on-foot analogue of the additive `SetWantedVelocity` seam §3 proposed for the
proxy-driven states, and because the task ticks at idle the add works from a dead stop. Do **not**
target `SetWantedVelocity` for on-foot roomscale — it is not on the on-foot path.

### 4.2 Raycast / scene-query API — `CPhysicsSystem::CastRay` and friends

`CPlayerAimControl::UpdateDirectAim` (**0x140CE5350**) calls the engine's general scene query
directly (no wrapper): `CPhysicsSystem::CastRay` (**0x140286740**). The physics-system singleton is
`qword_142EDC120`.

```
bool CPhysicsSystem::CastRay(CPhysicsSystem*,
                             const CRay3&        ray,        // { CVector3f start; CVector3f direction; } WORLD space, direction normalized
                             float               minFraction,// 0.0 in the aim call
                             float               maxDistance,// world-space length along direction
                             CCastRayInfoBase*   outHit,     // nullptr = boolean-only
                             CRaycastFilter*     filter,     // nullptr = default filter
                             unsigned int        flags,      // 0 in the aim call
                             int                 layer);     // 21 in the aim call ("aim/query" layer)
```

Coordinate space is world. `layer=21` is the layer the aim query uses; `flags` is a
collision-filter-info word (0 for a plain query). Backend is Havok `hknpWorld::castRay`
(**0x141500BE0**).

- **Filter semantics.** `CRaycastFilter` ctor (**0x1401FCBA0**):
  `CRaycastFilter(int layerA, int layerB, IPfxInstance* ignoreInstance, const hknpWorld*)` — the
  `ignoreInstance` lets you exclude the player's own proxy. The aim path uses the subclass
  `CIgnoreMaterialFlagFilter` with `SetIgnoreBulletPass` / `SetIgnoreRayPass` / `SetIgnoreCameraPass`
  (**0x140B86840/60/80**) — JC3 materials carry per-pass "holey" flags (foliage a bullet passes
  through, glass, etc.); the filter chooses which passes count as a hit. `CPlayerAimControl` wraps
  this in `CPlayerAimRaycastFilterCacheFirstHoleyHitAndMinDistanceHit` (a caching collector that
  records both the first holey hit and the nearest solid hit).
- **Result.** `CCastRayInfo : CCastRayInfoBase` (`Reset` **0x1402558C0**) carries hit
  position/normal/fraction, hit body/instance, `GetHitMaterial` → `EGameMaterialId`
  (**0x1402B3BA0**), `DidHitBulletPassMaterial` (**0x14023BDF0**), `GetHitNameHash`
  (**0x1403E48E0**).
- **Thread-safety / phase.** These are sim-phase queries against the live Havok world.
  `UpdateDirectAim` runs inside the aim update on the sim thread (`CPlayerAimControl::UpdatePreSim`
  **0x140C65920** / `UpdateAllTargets` **0x140CE7690**), *not* during the physics solve and *not*
  from a render worker. Call the probe from a sim-phase hook (the same phase the mod already uses for
  aim/HIK work), never from the camera/render hook.

**Simpler variants** (all `CPhysicsSystem` members, all world space):

- `CastRaySimple` (**0x140286BE0**): `bool CastRaySimple(const CVector3f& start, const CVector3f& end,
  unsigned int flags)` — builds the ray, no hit-info, no filter, hardcodes `layer=21`, returns a
  boolean. **This is the ideal head-through-geometry penetration probe** (start = last-safe head,
  end = current head; a `true` return means the segment is blocked).
- `CastRayTerrain` (**0x140286CE0**) — terrain-only.
- `CastRayWaterSurface` (**0x14023C150**) — water plane.
- **Shape casts** for magnetism volumes: `CastSphere` (**0x140202C90**)
  `bool CastSphere(const CVector3f& start, const CVector3f& end, float radius, float, hknpCollisionQueryCollector&, unsigned int, float)`;
  `CastShape` (**0x1401F0890**) for an arbitrary `hknpShape*` with a rotation; and the free function
  `SweptSphereCast` (**0x140738670**) filling a `CCastResult`. `CMagnetWeaponComponent::CastSphereAgainstStatic`
  (**0x14098A850**) is a worked consumer to copy for magnetism candidate scoring.

### 4.3 Teleport / discontinuity detection — the `+0x2B08` flag bit

The character teleport writers (all bypass the collision solve):

- **Fast travel / mission warps:** `CGameWorld::TeleportPlayer` (**0x1409AF820**) *queues* a deferred
  request on `CGameWorld` (applied later by `UpdateTeleport`) and never touches the character
  directly; `TeleportPlayerInstant` (**0x140A126C0**) does the actual move — it calls the object's
  virtual world-transform setter (vtable slot `+0x90` / index 18 on the character's transform
  subobject `CCharacter + 8`), then `CCharacter::ForceNeutralState` (**0x1407FD120**), sets the
  teleport flag `CCharacter + 0x2B08 |= 4` (verified in the release decompile), and re-bases the
  camera via `CGameCameraManager::ResetCamera` (**0x14077BCE0**). The warps are bracketed with
  `NEvent::CPostEvent::PostMsg("game_teleporting_initiated" / "…_completed")` — a clean string-keyed
  choke point if event-level notification is wanted.
- **Scripted teleport objects:** `CTeleport::Teleport` (**0x14050E360**), `CGameWorld::UpdateTeleport`
  (**0x140A128A0**), `NStateTask_InputVehicleExitTask::TeleportUpwards` (**0x14081A3C0**).
- **Vehicle:** `CVehicle::TeleportVehicle` (**0x140F4FC60**) (the seated character rides via the
  attach, §4.4).
- **Orientation-only / warp-task writers:** `CCharacter::WriteWorldMatrixOrientation`
  (**0x1408D73A0**, sole caller `SetOrientation(CQuaternion)` **0x140803C10**) and the animation warp
  tasks (`NPhysicalAnchorWarpTask`, `NStateTask_MovementHeightWarpTask`).

**Correction (release-verified): the `m_NumFramesSinceTeleport` counter does NOT work as a
detector.** An earlier draft of this section recommended `m_NumFramesSinceTeleport == 0`; independent
verification falsified it. The field is `++`'d each tick (in `CCharacter::DebugVerifyCharacter`,
called unconditionally), but **no writer ever resets it** — the apparent `= -1` in the dump is a
local-variable fallback inside a warning-log call, not a field write. With no reset, the field counts
frames since spawn and `== 0` never fires on a teleport.

**The verified uniform signal is the flag bit `CCharacter + 0x2B08 & 4`**, set by
`TeleportPlayerInstant`. Caveat: the flag-*setter* set has not been exhaustively enumerated (raw
offset xrefs are not a single choke point), so combine it with the robust fallback: a per-tick
`length(T1.translation − prevT1.translation) > threshold` distance heuristic (threshold well above
wingsuit speed × tick), which catches any writer the flag misses. Use either firing to suppress the
roomscale add and re-base the VR rig for one tick. The `game_teleporting_initiated` /
`…_completed` `PostMsg` events remain the clean choke point for the scripted warps.

### 4.4 Seat pose reference — `m_AttachBone` + `m_AttachOffset` on the character

When the character seat-locks into a vehicle the pose is **not** stored on the vehicle's `CSeat`
(that struct is interaction-graph metadata only: `m_GraphNode`, `m_DoorSlot`, `m_WindowPartInstance`,
`m_PlayerUsable` — no transform). It is defined on the **character**, by an attach *bone + offset*:

- `CCharacter::m_AttachBone` (`NBone::ESafeBoneIndex`) — a safe bone / socket **on the parent
  (vehicle) model**.
- `CCharacter::m_AttachOffset` (`CMatrix4f`) — the character's transform *relative to that bone*.
- `m_Attachable` (`CAttachable`), `m_attachType` (`AttachType`), `m_attachedObject` — the binding
  (§3). The Y-aligned attach entry is `CCharacter::SetYAlignedAttachTo` (**0x14079D540**), which
  computes a yaw-only alignment from the target frame and calls the `AttachTo` virtual (vtable slot
  `+0x140` / index 40); `AttachToWithCurrentOffset` (**0x1403FEE10**) preserves the live offset.

So while attached, **character world = vehicle.SafeBoneWorld(`m_AttachBone`) · `m_AttachOffset`**. The
seat's head/eye reference the mod should re-base the VR cockpit to is therefore just **the
character's own head/eye bones** — the exact `GetSafeBoneMatrix(HEAD)` / eye-bone reads the mod
already does on foot — because the whole skeleton rides the vehicle through the attach. No new
vehicle-side read is needed; the seat pose is expressed as the character's animated head bone, which
is already vehicle-relative.

**Do not use the vehicle camera as the seat reference.** The in-vehicle camera is a *third-person
chase* rig (`SGenericVehicleCamera`: `m_CameraPosition`, `m_RotationPoint`, `m_LookAtPoint` relative
to the vehicle body, plus spring/lag/FOV tuning) — it has no seat-eye anchor, so it is a poor
fallback. If a vehicle-frame anchor is ever needed independent of the character skeleton, compose
`m_AttachBone` world (from the vehicle's animated model) with `m_AttachOffset` directly.

### 4.5 Root-motion summing — REPLACE, not add; the two dir sources are exclusive

`NStateTask_LocoUtil::EvaluateCharacterDisplacement` (**0x14081AB90**) is a strict **either/or**, not
a sum. Its first branch (taken when the animation is *not* mid-blend, no code-driven marker
`0xE844061C` is set, and it is outside the special segments) reads
`CAnimationControl::GetRawRootVelocity` (**0x140434F20**), rotates it into world space, writes the
wanted velocity, and **returns immediately** (`0x14081AC63…ACA2`). Only if that branch is skipped
does it fall through to the code-driven path, which reads the target direction from blackboard
`0x7DF24A88` (with previous-dir `0x370A3A61` and the marker), builds a direction, and rotates it out.
**Animation root motion and the blackboard-`0x7DF24A88` direction never both contribute in one call**
— it is one source per tick.

Consequently, adding a roomscale velocity on top of stick locomotion **does not double-count against
`EvaluateCharacterDisplacement`**, because the mod's additive term is applied *after* the task, at the
proxy-input write (`CPfxCharacterInstance + 0x3C`, §4.1), not inside the displacement evaluator.

**Safe composition rule:** `proxy_wanted_velocity_ws = engine_result + roomscaleDeltaXZ / dt`, where
`engine_result` is whatever the locomotion task already wrote to `+0x3C` (animation root motion *or*
code-driven displacement, whichever the tick chose — the mod need not care which). The one real
double-count risk is **stick-driven walking**: while the stick is deflected the animation walk cycle
already produces root velocity, so a *physical* walk added on top stacks with it. Gate the roomscale
add to the idle/near-idle case (stick near center) or scale it against stick magnitude — the same
gating §3's open question anticipated, now with the mechanism confirmed: the add is at `+0x3C`, the
engine value is self-consistent, and the only overlap is real-walk-plus-stick-walk, not
root-motion-versus-blackboard.

### 4.6 Capsule shape and pause

**(a) Capsule dimensions and runtime resize.** The character proxy's capsule is a Havok `hknpShape`
selected per **proxy state**. `CPfxCharacterInstance::SetProxyState` (**0x140239580**) is the swap:
it stores the new state at `CPfxCharacterInstance + 0xC8`, fetches the per-state shape from the
avatar template's shape table (`*(this+240)->vtbl[2]()` indexed by state), and calls
`hknpWorld::setBodyShape(world, bodyId, shape)` (**0x1414CF370**) to hot-swap the shape on the live
body, plus a byte at `+0x31` on a linked object. `BuildDefaultCharacterContext` (**0x14024D370**)
builds the Havok *character-state* machine (on-ground / in-air / jumping / flying) but not the
capsule dims themselves — those live in the pre-registered `hknpShape` objects in the avatar template
table, keyed by `EProxyState`. **So the engine itself already resizes/swaps the capsule** (the
`m_PendingProxyState` / `m_DefaultProxyState` mechanism) for its own state changes.

A safe runtime height-reduction path for physical crouch: register a shorter capsule `hknpShape` once
and call `hknpWorld::setBodyShape` on the character body id
(`CPfxCharacterInstance::GetCharacterRigidBodyId` **0x14024DA40**) to swap to it, restoring the
default on stand — exactly what `SetProxyState` does, so it is a proven, thread-consistent path (do
it on the sim thread, as the engine does). Reuse the existing proxy-state swap machinery rather than
mutating shape geometry in place. Note the engine also swaps proxy state for swimming/vehicles/ragdoll
transitions, so a crouch swap must cooperate with (defer to) those state changes, not fight them.

**(b) What the game's pause freezes, and a camera-live alternative.** A real pause (pause menu up)
switches `CGameStateRun` into its paused update, which runs `UpdateRenderPaused`
(**0x1409AE200** in release; the larger symbol'd variant in the dump). That path drives the *render*
systems — landscape LOD, UI prerender, video, `WaitForCPUDrawToFinish`, resource streaming — and it
**reads** the active camera's already-computed `m_TransformF`, but it does **not** call
`GameCameraManager::UpdateRender` or `CameraTree::UpdateRenderContexts`. Those are the very seams the
mod's camera hook rides (`docs/mod/head-and-body.md`), so **under a real pause the camera transform
is frozen and the HMD view stops tracking the head** — unacceptable for VR. Entering pause also sets
`CPhysicsSystem::m_Pause = 1` and `CClock::Pause(true)`; leaving it clears both
(`CGameStateRun.cpp`: `m_Pause = 0; CClock::Pause(instance, 0)`).

The clock split makes a camera-live freeze possible. `CClock::Update` (**0x140093230**) keeps two
independent accumulators: the **real** clock (spf/fps at `+0x10/+0x14`, always advances) and the
**game** clock (`+0x44` is the game-pause bool; when set, the game-time counters at `+0x44/+0x48` and
tick counters at `+0x48/+0x50` stop, gated by `v13 = *(a1+44)`), with a timescale multiplier at
`+0x20`. `CClock::Pause` (**0x140091BB0**) toggles that game-pause bool. The mod already hooks
`CClock::Update` (`payload/src/hooks/clock.rs`).

**Recommended VR pause:** freeze *gameplay time* (game-clock pause / zero the game dt, and skip the
character + vehicle sim tick) **without switching `CGameStateRun` into its paused sub-state**, so the
normal Run render path keeps calling `GameCameraManager::UpdateRender` / `CameraTree::UpdateRenderContexts`
every frame and the mod's camera hook stays live — the HMD view keeps tracking at full rate while the
world holds still. This is the "pause, physically take your seat, confirm, unpause" transition: the
mod owns `CClock::Update`, so it can zero the game dt there (leaving the real/render dt intact) and
gate the sim update, rather than invoking the game's own pause, which darkens the camera seam.

## Feasibility verdicts (roomscale-locomotion handoff)

- **Idle-feed — EASY.** The locomotion task ticks at idle and writes the proxy input at
  `CPfxCharacterInstance + 0x3C` every tick; a post-call add from a standstill needs no state-machine
  change (§4.1).
- **Penetration / magnetism probe — EASY.** `CPhysicsSystem::CastRaySimple` (boolean LOS) and
  `CastRay`/`CastSphere` (full hit info) are directly callable from a sim-phase hook with world-space
  args and material-pass filtering (§4.2).
- **Teleport detect — MODERATE (revised).** The `+0x2B08 & 4` flag bit plus a distance-delta fallback covers the
  warp writers uniformly, no writer hooks or thresholds (§4.3).
- **Seat re-base — EASY.** The seat eye reference is the character's own head/eye bones (already read
  on foot), riding the vehicle via `m_AttachBone` + `m_AttachOffset`; no vehicle-side read needed, and
  the chase camera is explicitly not the reference (§4.4).
- **Velocity compose — EASY/MODERATE.** Root motion vs. blackboard direction is exclusive (no
  double-count there); the add is safe as `engine_result + roomscaleDeltaXZ/dt` at `+0x3C`, with the
  one caveat of gating against stick-walk overlap (§4.5).
- **Crouch capsule — MODERATE.** A shorter `hknpShape` swapped via `hknpWorld::setBodyShape` reuses
  the engine's own proxy-state mechanism (safe on the sim thread), but must cooperate with
  engine-driven swaps (swim/vehicle/ragdoll) (§4.6a).
- **Pause-with-live-camera — MODERATE.** Achievable by freezing game-time via the already-hooked
  `CClock::Update` and skipping the sim tick while staying out of `CGameStateRun`'s paused sub-state
  (which otherwise freezes the camera seam); not a one-liner, but the clock split and the hook both
  already exist (§4.6b).

## 5. Per-tick mode detection — the action-set selector

The recon for the mode detector the controllers-and-roomscale phase 0 (`docs/mod/controllers-and-roomscale.md`)
depends on: reliable per-tick signals, readable from the local `CCharacter` (or a cheap singleton),
that discriminate the player's mode finely enough to drive OpenXR action-set selection — on foot,
in a vehicle (and its class), wingsuit, parachute, grapple traversal, and game-UI-up. Today's headpose
detector is binary (on-foot vs. other, from the `EvaluateCharacterOrientation` counter, `sim.rs`); this
section replaces it with a direct read of the engine's own state. Same conventions: release-build RVAs
from the 2026 no-Denuvo IDB, 2016 symbol dump as locator only.

### 5.1 The central mechanism — the character state-bitflag words

Almost every "what is the player doing" predicate the game ships (`CCharacter::IsUsingWingsuit`,
`IsReelingIn`, `IsSwimming`, …) is a one-bit read of a small array of 64-bit **state-bitflag words**
owned by the character's control parameters. The mod can read the same words directly and skip the
call overhead entirely.

- `CCharacter::m_ControlParameters` is a pointer at **`CCharacter + 0x2718`** (`*(character + 1251)`
  in qwords — confirmed in the release decompile of `IsInVehicleAttachState`).
- The state-bitflag array (`m_ControlParameters->m_UserVM.m_CurrentStateBitFlags`) begins at
  **`m_ControlParameters + 0xE8`**: **word0 = `+0xE8`**, **word1 = `+0xF0`**, **word2 = `+0xF8`**
  (each a `u64`). So the whole read is `flags = *(u64*)(*(u64*)(character+0x2718) + 0xE8 + 8*word)`.
- A handful of the flags additionally gate on `m_AnimatedModel.m_InstanceData` being non-null
  (`*(character + 0x1958)`); the animation instance must exist for the bit to be meaningful (matters
  only in the first frames after spawn/stream-in).

The bit assignments below are read straight from the release accessors (verified against the release
IDB, not just transcribed from the dump), so they are the ground-truth bit layout for this build:

| Predicate | Release addr | Word | Bit (mask) | Notes |
|---|---|---|---|---|
| `IsUsingWingsuit` | `0x14075F630` | 0 | 0 (`0x1`) | gated on `m_InstanceData` (`+0x1958`) |
| `IsUsingParachute` | `0x14075F550` | 0 | 35 | parachute deployed and controlling descent |
| `IsFreefalling` | `0x14075F570` | 0 | 5 (`0x20`) | |
| `IsFalling` | — | 0 | 4 or 5 (`0x30`) | freefall OR fall |
| `IsFastfalling` | `0x14075F910` | 0 | 33 | dive |
| `IsReelingIn` | `0x14075F530` | 1 | 44 | grapple reel traversal in progress |
| `IsRidingMC` | `0x14075F380` | 1 | 37 | on a motorcycle |
| `IsSwimming` | `0x14075F450` | 1 | 56 | |
| `IsUnderwaterSwimming` | `0x14075F470` | 1 | 57 | |
| `IsInAttachedToParachute` | `0x14075F7C0` | 1 | 26 (`0x4000000`) | attached-to-vehicle-parachute variant |
| `IsPreReelingIn` | `0x14075F770` | 1 | 27 (`0x8000000`) | reel-in wind-up |
| `IsPreHang` | `0x14075F790` | 2 | 7 (`0x80`) | |

These are pure reads of game-thread data; do them on the sim tick (the same phase the headpose sim
already runs), never off a render worker — the words are written during the character's animation-graph
evaluation.

### 5.2 The state-machine-hash predicates (vehicle-riding, reeled-in, hang)

A second class of predicate is not a single bit: it walks the animation rule system
(`m_AnimatedModel.m_RuleSystems[0]->m_StateMachineInstance->m_CurrentState->m_HashID.m_Hash`) and
compares the current animation state hash against a set of known state ids, sometimes OR-ed with a
bitflag or the grapple-hook state. These cost a few pointer chases but are still cheap, and they are
the authoritative signal where a raw bit is ambiguous:

| Predicate | Release addr | What it establishes |
|---|---|---|
| `IsInRidingInVehicleState` | `0x14077EA60` | seated-and-riding (idle/driver/passenger/reverse states, or word1 bit37 set) |
| `IsInDrivingVehicleState` | `0x14077EAF0` | specifically the driver seat's driving states |
| `IsInVehicleAttachState` | `0x14077F080` | riding **or** the enter/exit/switch-seat/eject transitions (already pyxis-bound) |
| `IsReeledIn` | `0x14077ED10` | fully reeled onto an attach point (reads hook `m_State` ∈ {3,4,5,6} + word0 bits 32/42 + stunt/hang state hashes) |
| `IsGrappleHanging` | `0x14079E290` | hanging from a reeled grapple (hook `m_State == GHS_REELED_HANG`, or `S_GRAPPLE_HANG`/`S_IDLE_HANG_STUNT`, or word0 bit42) |
| `IsStuntTraversing` | `0x14077EF60` | stunt-position traversal (`S_STUNT_FWD/RIGHT/LEFT/BWD`) |
| `IsInMountedGunState` | `0x1407B0F60` | manning a mounted/emplaced gun |

`IsInVehicleAttachState` is the widest "the body is bound to a vehicle" test (it returns true through
the whole enter/exit animation, not only when settled), which is exactly what roomscale wants to gate
off (§3, §4.4): the mount animation is part of the seated envelope.

### 5.3 Vehicle presence and class

The seated vehicle and its class come from the interaction graph, not a flag:

- `CCharacter::GetVehicle` (`0x1407D5D90`) fills a `boost::shared_ptr<CVehicle>` (walks the interaction
  graph's top-root object, `rtti_cast<CVehicle>`); `GetVehiclePtr` (`0x1407D5E30`) is the raw-pointer
  wrapper. Both take/release a shared-ptr refcount (an interlocked inc/dec), so they are a touch heavier
  than a field read — fine per tick, but cache the pointer within a tick rather than calling repeatedly.
- **Class** is decided by RTTI `IsType` against the vehicle's class-hierarchy `TYPE_ID`, exposed as
  ready-made character predicates:

| Class predicate | Release addr | `TYPE_ID` tested |
|---|---|---|
| `IsAttachedToVehicle` | `0x1407D5EA0` | any (has a `CVehicle` and is graph-attached) |
| `IsAttachedToLandVehicle` | `0x1407D5FA0` | `CLandVehicle` (car + motorcycle) |
| `IsAttachedToAirVehicle` | `0x1407D6060` | `CAirVehicle` (plane + helicopter) |
| `IsAttachedToSeaVehicle` | `0x1407D6120` | `CSeaVehicle` (boat + jetski) |
| `IsAttachedToHelicopter` | `0x1407D61E0` | `CHelicopter` |
| `IsAttachedToMech` | `0x1407D62A0` | via `CVehicle::IsMech` (`0x140F23990`) |

These give the coarse split directly. The `CButtonMapping::EMapping` sections split finer than the RTTI
bases do — car vs. motorcycle, boat vs. jetski, plane vs. helicopter — so map the six action-set classes
as:

- **Helicopter** = `IsAttachedToHelicopter`.
- **Plane** = `IsAttachedToAirVehicle && !IsAttachedToHelicopter` (both derive from `CAirVehicle`).
- **Motorcycle** = `IsAttachedToLandVehicle && IsRidingMC` (word1 bit37).
- **Land car** = `IsAttachedToLandVehicle && !IsRidingMC`.
- **Boat/jetski** = `IsAttachedToSeaVehicle`; boat-vs-jetski is the one class RTTI does **not** split at
  the `CSeaVehicle` base. The exact 7-way engine enum is `NVehicle::EVehicleType` (`ECAR=0, EHELICOPTER=1,
  EBOAT=2, EAIRPLANE=3, EMOTORCYCLE=4, ESUB=5, ETRAIN=6`), stored on the vehicle as
  `CVehicle::m_VehicleType` and returned by `CVehicle::GetVehicleType` (inlined everywhere in this build —
  the field offset is not yet pinned). Note this enum still folds jetski into `EBOAT`; the game keys the
  jetski mapping band off a jetski-specific type elsewhere. **TODO:** pin the `m_VehicleType` offset and
  the jetski discriminator if the boat/jetski action sets need to differ; until then treat sea vehicles
  as one class.
- **Seat matters for input, not class**: `CVehicle::m_LocalPlayerSeat` (`NVehicle::ESeat`, `EDriverSeat=0`,
  `ENoSeatIdentifier=0xFFFFFFFF`) is what the game's own context predicates test (`m_LocalPlayerSeat ==
  EDriverSeat`) to decide whether driver controls apply — a passenger/gunner gets a different mapping even
  in the same vehicle. `CVehicle::GetSeat` is `0x140F24FB0`.

### 5.4 Grapple traversal states (`GHS_*`)

The grappling-hook state machine is the finest grapple-traversal signal, orthogonal to the character
animation flags. `CCharacter::GetGrapplingHook` (`0x140760830`) returns the hook shared-ptr; the raw hook
pointer is cached at **`CCharacter + 0xA40`** (`m_Inventory.m_GrapplingHook.px`), and its state is a
`u32` at **`hook + 0x234`** (`CGrapplingHook::m_State`):

```
enum EGrapplingHookState { GHS_INITIALIZING=0, GHS_INACTIVE=1, GHS_REELING_IN=2, GHS_REELED_ATTACHED=3,
                           GHS_REELED_HANG=4, GHS_REELED_UPSIDEDOWN=5, GHS_REELED_STUNT=6,
                           GHS_CUSTOM_ACTIVE_WIRE=7 };
```

So `m_State ∈ {3,4,5,6}` is "reeled onto something" (the `IsReeledIn` predicate), `GHS_REELED_HANG`
is hanging, `GHS_REELING_IN` (2) is mid-reel. The character-level `IsReelingIn` / `IsGrappleHanging` /
`IsReeledIn` above compose this hook state with the animation state and are the recommended readers;
the raw `hook->m_State` is the fallback if the hook pointer is present but the character predicate is
ambiguous during a transition.

### 5.5 Game-UI-up / gameplay-input-suspended

The engine suspends gameplay input through two distinct mechanisms; a robust UI action-set trigger reads
both:

1. **Hard pause (pause menu).** `CGameStateRun` propagates an update-context `m_Paused` flag into
   `CPhysicsSystem::m_Pause` (`Base::CSingle<CPhysicsSystem>::Instance->m_Pause = m_Paused`) and
   `CClock::Pause(true)` (the game-clock game-pause bool, `docs/engine/hands-and-roomscale.md` §4.6b). The
   physics-system singleton is `qword_142EDC120` (§4.2); its `m_Pause` byte is nonzero for the whole real
   pause. This is the cleanest per-tick "gameplay is frozen" read. **Caveat:** a real pause also freezes
   the camera seam (§4.6b) — the VR pause must *not* enter this state, so when the mod owns the freeze it
   reads its own state, and `m_Pause` is the detector only for the *game's* pause menu.
2. **Slow-mo / radial pause.** `CGamePauseUtility` (a `Base::CSingle` singleton) drives the
   pause-until-action flow (weapon wheel / prompt). `m_state` is its first field (offset 0);
   `m_state == 2` (`E_STATE_PAUSED`) is up. `SetState` is `0x1409BA270` (xref its `this` to recover the
   singleton `Instance` address).
3. **Input focus (map, commlink — no physics pause).** Opening the map or commlink does *not* hard-pause;
   the game instead switches input focus to the UI action map. `CSteeringUI` holds a static UI action map
   (`CSteeringUI::GetActionMap` → `m_ActionMapUI`) that becomes the focused consumer while a UI screen is
   up, and the `CButtonMapping` `FIRST_UI_MAPPING..END_UI_MAPPINGS` band (the game's own `END_UI_MAPPINGS`
   boundary) is what applies. In practice the mod already owns its floating-panel UI and the virtual
   cursor, so it *knows* when its own panel/menu is up without an engine read; the engine-side UI-focus
   read (is `CSteeringUI` the active steering) is the signal for the game's *own* screens (map/commlink/
   pause) that the mod did not open. **TODO:** if the game's map/commlink must independently trigger the
   `ui` action set, pin the "UI steering active" global (`CSteeringUI::m_SteeringUI` presence plus a
   focus check); the pause flag alone does not cover the non-pausing screens.

### 5.6 Recommended mode-derivation recipe

Per sim tick, read `character = GetLocalPlayerCharacter()` once (null → no mode / keep last), snapshot the
three state-bitflag words once (`§5.1`), and resolve the action-set mode by the **first** matching check —
order matters because states overlap (a reeled grapple is also airborne; a wingsuit dive is also falling):

1. **UI up** → `ui` action set. `CPhysicsSystem::m_Pause != 0` (`qword_142EDC120`) **or**
   `CGamePauseUtility.m_state == 2` **or** the mod's own panel is open **or** (for game screens) the UI
   steering is focused. Highest priority: a menu overrides everything beneath it.
2. **Teleport this tick** → hold/recenter, do not switch mode. The `+0x2B08 & 4` flag or the distance-delta fallback (§4.3) —
   suppress mode transitions for the settling frame.
3. **Seated in a vehicle** → `vehicle` action set + class. `IsInVehicleAttachState` (covers the enter/exit
   animation too). Class via §5.3: helicopter → plane → motorcycle → car → sea, using
   `IsAttachedToHelicopter` / `IsAttachedToAirVehicle` / `IsRidingMC` / `IsAttachedToLandVehicle` /
   `IsAttachedToSeaVehicle`. Seat (`m_LocalPlayerSeat == EDriverSeat`) selects driver vs. passenger/gunner
   sub-mapping.
4. **Grapple traversal** → keep gameplay (`onfoot`/`airborne`) but flag the reel/hang sub-state for the
   arm-IK and comfort handling. `IsReelingIn` (word1 bit44) / `IsGrappleHanging` (`0x14079E290`) /
   `IsReeledIn` (`0x14077ED10`) / `IsPreReelingIn` (word1 bit27) / `IsPreHang` (word2 bit7). The
   `GHS_*` hook state (`hook+0x234`) is the fine discriminator.
5. **Wingsuit** → `airborne` action set (wingsuit variant). `IsUsingWingsuit` (word0 bit0, `m_InstanceData`
   gated).
6. **Parachute** → `airborne` action set (parachute variant). `IsUsingParachute` (word0 bit35), or
   `IsInAttachedToParachute` (word1 bit26) for the vehicle-parachute case.
7. **Airborne (free)** → `airborne`. `IsFreefalling` / `IsFalling` / `IsFastfalling` (word0 bits 5/4/33).
8. **Swimming** → `onfoot` (swim variant if wanted). `IsSwimming` / `IsUnderwaterSwimming` (word1 bits
   56/57).
9. **On foot (default)** → `onfoot`. Nothing above matched; the `EvaluateCharacterOrientation` counter the
   sim already watches remains a good corroborating idle/on-foot heartbeat (it advances every on-foot tick
   including idle, §4.1), but the state-word read is now the primary source and resolves the modes the
   counter cannot.

The whole recipe is one pointer chase plus a fixed set of bit tests and a `GetVehiclePtr` (only when a
vehicle bit/state indicates a vehicle), so it is comfortably a per-tick cost.

### 5.7 Edge cases

- **Transitions hijack mid-animation.** Vehicle enter/exit, grapple wind-up (`IsPreReelingIn`/`IsPreHang`),
  and reel release all pass through animation states where the *coarse* bit is not yet set but the body is
  already committed. `IsInVehicleAttachState` deliberately spans the transition; for grapple, the pre-*
  predicates catch the wind-up. Prefer the state-machine-hash predicates (§5.2) over the raw bit during a
  known transition, and debounce mode switches by a frame or two so a one-tick flicker (e.g. a blend
  frame) does not thrash the action set.
- **Overlapping states.** Reel/hang is simultaneously "airborne"; wingsuit and parachute are both
  "falling"; a vehicle-parachute is both attach and parachute. The priority order in §5.6 is the
  disambiguator — vehicle before grapple before wingsuit before parachute before generic airborne.
- **Swimming.** `IsSwimming` is set independently of the on-foot locomotion counter; the counter alone
  (today's detector) reads a swimmer as `Other`. The explicit swim bits fix that.
- **Spawn / stream-in.** The `m_InstanceData`-gated bits (wingsuit) read false until the animation
  instance exists; treat a null `m_ControlParameters` or `m_InstanceData` as "no reliable mode, hold last"
  for the first frames.
- **Teleport spike.** Gate mode transitions on the §4.3 teleport signals so a fast-travel or
  mission warp does not momentarily misclassify the frame the character is being re-based.

### 5.8 Confidence

| Signal | Confidence | Basis |
|---|---|---|
| On foot (state words + orientation counter) | **High** | word reads verified in release; counter already proven in `sim.rs` |
| Vehicle presence + coarse class (land/air/sea/heli) | **High** | RTTI predicates release-addressed and decompiled (`IsAttachedTo*`) |
| Vehicle fine class (car/mc, plane/heli) | **High** | `IsRidingMC` bit + `IsAttachedToHelicopter` cleanly split their pairs |
| Boat vs. jetski | **Low** | not split at the `CSeaVehicle` RTTI base; needs the `m_VehicleType`/jetski discriminator (TODO) |
| Wingsuit active | **High** | `IsUsingWingsuit` word0 bit0 verified in release |
| Parachute open | **High** | `IsUsingParachute` word0 bit35 verified; `IsInAttachedToParachute` covers the vehicle case |
| Reel/grapple traversal (`GHS_*`) | **High** | hook `m_State` offset (`+0x234`) and the character predicates verified |
| Hard pause (pause menu) | **High** | `CPhysicsSystem::m_Pause`/`CClock` pause path decompiled (§4.6b) |
| Radial/slow-mo pause | **Medium** | `CGamePauseUtility.m_state == 2` established; singleton `Instance` address still to xref |
| Map/commlink UI focus (non-pausing) | **Medium** | mechanism understood (`CSteeringUI` focus + UI mapping band); no single clean per-tick global pinned yet |
