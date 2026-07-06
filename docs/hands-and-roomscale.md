# Hands, arms, and roomscale

Scoping VR motion controllers for JC3VRS: virtual hands that hold weapons, both arms aimable at
independent targets (right gun one way, left grapple another), and roomscale locomotion where
physically walking moves the in-game character. This is a reverse-engineering recon of the three
systems that would carry those features — the weapon-attachment (prop) chain, the shipped per-arm
aim IK, and the character root/collision-capsule movement path — with feasibility verdicts and the
interface points with the aim and grapple pipelines.

All addresses are release-build RVAs (`JustCause3.exe`, 2026 no-Denuvo IDB), read from the release
decompile; the 2016 symbol dump was the locator only. This is design/RE notes, not shipped code.

Related: `docs/humanik.md` (the solver, effector ids, the injection seam), `docs/skeleton.md` (the
Joint API and pose pipeline), `docs/head-and-body.md` (the "Boneworks alignment" topology this
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
  `CPlayerAimControl`, see `docs/head-and-body.md` "The aim seam"). If controller-relative firing is
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
itself, it competes with the mod's head override and the body-IK head target from `docs/humanik.md`.
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
`UpdatePassFinalizePose_Parallel` (`docs/humanik.md`), so their targets and the mod's head target
coexist on the same solve (the pass step is the max; effectors are keyed by id).

**Driving both arms from controller rays is feasible and low-risk**, using the exact pattern the mod
already uses for the head effector (`payload/src/hooks/character.rs`,
`AddEffectorTargetRotationVector` on `PASS_MAIN`):

- Queue an `AddEffectorTargetRotation` for the right-hand/right-arm effectors from the right
  controller ray, and for the reel-in arm effector from the left controller ray, each with its
  `m_TargetReachR`. Rotation effectors on distinct effector ids do not collide.
- **Hand *position* effectors** can place the wrists at controller positions outright:
  `GetEffectorIdFromBoneIndex(m_HIK, wristBoneIndex)` yields effector **3 (left wrist)** / **4 (right
  wrist)** (see the effector table in `docs/humanik.md`), then
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

**Roomscale can ride the displacement path so collision is respected for free.** Two ride points:

1. **Additive wanted velocity** — the least invasive: add `roomscaleDeltaXZ / dt` to the wanted
   velocity that the on-foot locomotion task passes to `SetWantedVelocity` (hook `SetWantedVelocity`
   for the local player, add the roomscale term), so the physics proxy walks the extra distance and
   resolves collision. The capsule follows automatically because the proxy *is* the capsule.
2. **Code-driven displacement direction** — set `m_CodeDrivenDisplacement` and publish the roomscale
   direction to blackboard `0x7DF24A88`, letting `EvaluateCharacterDisplacement` produce the wanted
   velocity. Heavier (interacts with the game's own code-driven displacement users) and mainly a fit
   if roomscale should compose with authored displacement.

Ride point 1 is the recommendation: it is the same seam the game itself uses to move the proxy, it
is collision-correct by construction, and it is a small per-frame add rather than a state-machine
change. The residual XZ error between the player's real position and the (collision-clamped) capsule
is the "positional tracking through geometry" pitfall from `docs/head-and-body.md` — mitigate with a
fade on deep penetration, not a hard freeze.

### Vehicles — roomscale must disable (seat-lock)

In a vehicle the character is **attached**: `CCharacter::m_Attachable` (`CAttachable`),
`m_attachType` (`CCharacter::AttachType`), and `m_attachedObject` hold the parent binding; the
`AttachTo` virtual (vtable slot `+320`, wrapped by `SetYAlignedAttachTo` **0x14079D540**) sets it,
and while attached the character's transform is parented to the vehicle seat rather than driven by
the character proxy. `m_NumFramesSinceTeleport` tracks post-warp settling. Roomscale locomotion must
be gated off whenever the character is seat-attached (`m_attachType != NONE` / the in-vehicle state):
the body is fixed to the seat, the head stays free (this is the "easy case" from
`docs/head-and-body.md` — no body-yaw decoupling in vehicles), and adding wanted velocity to a
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
  `SetJoint`, and the body-IK head target (`docs/humanik.md`) all touch the head. Exact suppression
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
  `docs/head-and-body.md`).
- **Capsule size in roomscale.** Leaning/ducking physically moves the head but the proxy capsule is
  a fixed shape (`m_PendingProxyState`); whether crouch/lean should reshape the capsule or only
  offset the head is unresolved.
