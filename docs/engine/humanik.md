# HumanIK: driving the upper body from an external target

JC3 wraps Autodesk HumanIK in `NAnimationSystem::CHumanIK` (bindings: `jc3gi::animation::ik::HumanIK`). Each `Character` embeds one solver and drives it every frame inside the pose-finalization pass. This documents the solver's layout, its per-frame lifecycle, the seam where an external caller must inject effector targets so they solve the same frame, the effector-id model, and the exact parameter/coordinate-space semantics — everything needed to drive the player's spine and head from an externally supplied head target.

All addresses are release-build RVAs (`JustCause3.exe`, 2026 no-Denuvo IDB), verified against the release decompile; the 2016 symbol dump was the locator only.

## Where it lives

`Character::m_HIK` is at offset **0x10E0 (4320)** within `CCharacter` (verified: every HumanIK call in `UpdateSecondaryHandIKPass`/`UpdatePassFinalizePose_Parallel` passes `this + 4320`). `CHumanIK` itself is **0x6A0 (1696)** bytes. The release layout matches the dump once you account for this build's `std::vector` being 32 bytes (the three MSVC pointers plus a trailing allocator slot), which is how the pyxis `Vector<T>` is already modelled.

Confirmed field offsets (release):

| Offset | Field | Notes |
|---|---|---|
| 0x00–0x18 | `m_HIKCharacter` / `…State` / `m_HIKEffectorSetState` / `m_HIKPropertySetState` | opaque Autodesk HIK handles |
| 0x20 | `m_PassInfo[2]` | one `SPassInfo` (0x48 each) per pass; `[0]`=MAIN, `[1]`=SECONDARY |
| 0xB0 | `m_CurrentPass` | i32; set by `SetActiveIKPass` (`*(a1+176)=pass`) |
| 0xB8 | `m_HIKNodeAndBonePairs` | `Vector` (32 B) |
| 0xD8 | `m_UsedHIKNodeIds` | `int*`, `-2`-terminated |
| 0xE0 | `m_TQS` | `Vector` (32 B) |
| 0x100 | `m_EffectorIds` | `THashTable<int,uint,1,ushort>`, 0x20 B: `m_HashTable`@0, `m_ChainPool`@8, `m_HashTableLength`(u16)@0x10 |
| 0x120 | `m_TargetPull[44]` | interpolation destinations |
| 0x1D0 | `m_TargetResist[44]` | |
| 0x280 | `m_TargetReachT[44]` | translation-reach target weight (written directly after queuing a position target) |
| 0x330 | `m_TargetReachR[44]` | rotation-reach target weight (written after a rotation target) |
| 0x3E0 | `m_Pull[44]` | current values, driven toward the targets |
| 0x490 | `m_Resist[44]` | |
| 0x540 | `m_ReachT[44]` | |
| 0x5F0 | `m_ReachR[44]` | |

Each `SPassInfo` (0x48) is `{ m_SolveStep: SolveStep @0; Vector<positions> @8; Vector<rotations> @0x28 }`.

## Per-frame lifecycle

Everything runs in the SIM phase inside `Character::UpdatePassFinalizePose_Parallel` (**0x1407F9B10**), after the animation graph finalizes the local pose and before the model-space pose is computed. Gated by the global HIK enable (`byte_142D621C8`, `CCharacter::m_EnableHIK`) and the character not being in reduced LOD (`(*(this+10124) & 2) == 0`).

```
UpdatePassFinalizePose_Parallel(context):                        [0x1407F9B10]
  ... UpdateGroundInfo / UpdateAttachmentTransforms ...
  if m_EnableHIK && not-reduced-LOD:
    # ---- MAIN pass (body IK: aim, reach) ----
    if HasTargets(m_HIK, PASS_MAIN):                             [0x1403C96B0]  <-- THE GATE
        SetActiveIKPass(m_HIK, PASS_MAIN)                        [0x1403BD1A0]
        DriveAllCurrentEffectorControlValues(m_HIK, dt)         [0x1403EC970]
        # UpdateIKForTargets — inlined in release:
        CharacterToIKState(m_HIK, pose)                          [0x1403F4390]
        UpdateEffectorsFromTargets(m_HIK, dt)                    [0x1403F4530]
        Solve(m_HIK)                                            [0x1403F4920]
        IKToCharacterState(m_HIK, pose, updateAllBones)          [0x1403F49D0]  -> writes solved pose back
        UpdatePropTransforms(...)
    ResetSolveStep(m_HIK, PASS_MAIN)                            [0x1403BD270]
    if ClearTargets(m_HIK, PASS_MAIN): ResetProperties(m_HIK)    [0x1404020F0 / 0x1403BD260]
    # ---- SECONDARY pass (hand / grip IK) ----
    UpdateSecondaryHandIKPass(this, dt, &ik_has_run)             [0x1407EF690]
      -> SetActiveIKPass(PASS_SECONDARY); AddEffectorTargetPosition(..., PASS_SECONDARY);
         (solve); ClearTargets(PASS_SECONDARY); ResetSolveStep(PASS_SECONDARY)
    ResetSolveStep(m_HIK, PASS_SECONDARY)
  if skip-subframe: UpdateGraphicsMatrix; SyncPoses; T0 = T1
  UpdateGraphicsMatrix; UpdateModelVisibility; UpdateMyselfTarget
  CInventory::UpdateInventoryPostFinalizePose
  CalculateModelSpacePose(poseProducer)              <-- model-space pose finalized here
  ...
  Character::UpdatePropEffects(this, dt)             [0x1407C2380]  <-- LAST; the mod's existing hook
```

Key ordering facts:
- The MAIN solve is **gated by `HasTargets(PASS_MAIN)`**. No targets ⇒ the entire body solve is skipped that frame.
- `ClearTargets(PASS_MAIN)` runs **after** the solve, so targets queued before the gate survive to be consumed the same frame; then they are dropped/blended out for the next.
- The mod's current seam, `UpdatePropEffects`, is the **very last** call in this function — after both IK passes *and* after `CalculateModelSpacePose`. That is why the shipped head override there works as a direct `SetJoint`, but it is **too late for the HumanIK route**.

## The injection seam

To drive the upper body via HumanIK, an external caller must call `AddEffectorTargetPosition(m_HIK, …, PASS_MAIN, …)` **before the `HasTargets(PASS_MAIN)` gate** at `0x1407F9C84`.

Recommended seam: **hook `Character::UpdatePassFinalizePose_Parallel` (0x1407F9B10), pre-call** — queue the MAIN-pass head/effector targets at entry, then invoke the trampoline. The whole HIK block is well inside the function; nothing clears PASS_MAIN before the gate, so entry-queued targets reach the gate and are solved that frame. This matches how the game's own aim IK behaves (it queues MAIN targets even earlier, during animation-graph evaluation); queuing at function entry is simply the latest safe point.

- Do **not** use the existing `UpdatePropEffects` hook for HumanIK targets — it runs after the solve and after `CalculateModelSpacePose`.
- The pose read by `CharacterToIKState` is the one present when the solve runs (mid-function), i.e. the freshly animated pose — correct for a model-space target.
- Respect the same gates the engine does: only expect a solve when `m_EnableHIK` (`byte_142D621C8`) is set and the character is not in reduced LOD.

An alternative to a distinct hook is to keep using the `UpdatePropEffects` hook for the `SetJoint`-based head override (as today) and add a *separate* pre-call hook on `UpdatePassFinalizePose_Parallel` purely for HumanIK target queuing. The two are independent.

## Effector-id model

`GetEffectorIdFromBoneIndex(m_HIK, boneIndex)` (**0x1403E2BF0**) maps a **skeleton bone index** to a HumanIK effector id in `0..44`, or returns `-1` if the bone has no effector mapping. The bone index is in the *same integer space* the character's bone matrices/joints use (`AnimationController::GetBoneIndex`/`GetJoint`, and the value `Character::GetSafeBoneMatrix` resolves a `SafeBoneIndex` to). In `UpdateSecondaryHandIKPass` the hand bone indices come straight out of `Character::m_SafeBoneIndices` and are handed to `GetEffectorIdFromBoneIndex` unchanged — so `Character::GetSafeIndex(SafeBoneIndex::HEAD)` (already used by the mod) yields exactly the index to feed here.

The mapping (`GetEffectorIdMapping`, node→effector) fixes the important ids:

| Effector id | Body part |
|---|---|
| 0 | Hips |
| 1 / 2 | Left / Right ankle |
| 3 / 4 | Left / Right wrist |
| 5 / 6 | Left / Right knee |
| 7 / 8 | Left / Right elbow |
| 9 | Waist |
| 10 | Chest end (`GetChestEndEffectorId`, constant) |
| 11 / 12 | Left / Right foot |
| 13 / 14 | Left / Right shoulder |
| **15** | **Head** |
| 16 / 17 | Left / Right hip |

So the head effector is **15** (provided the characterization uses a head node, which the humanoid rig does). The 44-slot control arrays (`m_TargetReachT` etc.) are indexed by this effector id.

## Parameter and coordinate-space recipe

`AddEffectorTargetPosition` (**0x140408860**), prototype verified against the debug PDB and the release call sites:

```
AddEffectorTargetPosition(
    m_HIK,
    effector,                    # i32, e.g. 15 for head
    pos,                         # *const Vector3, CHARACTER-MODEL SPACE
    solve_step,                  # SolveStep
    pass,                        # Pass (PASS_MAIN to drive the body from an external target)
    effector_interpolation,      # bool
    effector_interpolation_rate, # f32
    effector_blend_out,          # bool
    effector_blend_out_rate)     # f32
```

**Coordinate space is character-model space**, not world. `UpdateSecondaryHandIKPass` builds `ws_to_ms = inverse(m_WorldMatrixT1)` and multiplies the world-space blackboard target through it *before* the call. Feed `inverse(characterWorld) · desiredHeadWorld` — the same transform the mod already uses to place the head joint.

After queuing a positional target, the engine also writes the reach weight: `m_HIK.m_TargetReachT[effector] = weight` (1.0 = full reach). Skipping this leaves the reach at zero and the target has no effect.

Defaults the game itself uses:

- **Hand pass (position), world-space target →** `solve_step` per-hand, `pass=PASS_SECONDARY`, `interpolation=false`, `interp_rate=3.0`, `blend_out=true`, `blend_out_rate=1.5`, then `m_TargetReachT[eff]=weight`.
- **Aim IK (rotation) on the body →** `NRightArmAimIK::UpdateAimEffector` (0x140838EC0) calls `AddEffectorTargetRotationVector(m_HIK, eff, angle, &axis, HIK_SOLVE_UPPER_BODY /*7*/, PASS_MAIN /*0*/, interp=false, 0, blend_out=false, 0)` and writes `m_TargetReachR[eff]`. This is the concrete precedent for **driving the upper body on PASS_MAIN**: use **`SolveStep::UPPER_BODY` (7)** (or `SPINE_HEAD_ONLY` (2) for spine+head only).

`SolveStep` maps to the Autodesk solver bitmask in `Solve`: `SPINE_ONLY`→0x4000, `SPINE_HEAD_ONLY`→0x6000, `ARMS`→0x60, `UPPER_BODY`→0x6679, `SPINE_HEAD_LOWER_BODY`→0xE180, `FULL_BODY_NO_PULL`→0xE1F8, `FULL_BODY`→0xFFF9. A pass's effective step is the **max** of its queued targets' steps (with an arm-combining special case), so a single head target at `UPPER_BODY` pulls the whole upper body.

There are two `AddEffectorTargetRotation` overloads: the axis-enum variant `AddEffectorTargetRotation` (**0x140408960**) and the explicit-axis-vector variant bound as `AddEffectorTargetRotationVector` (**0x140408BB0**).

### Recipe to drive the head

1. Pre-call hook on `UpdatePassFinalizePose_Parallel`.
2. `eff = GetEffectorIdFromBoneIndex(m_HIK, GetSafeIndex(HEAD))` (expect 15).
3. `posModel = inverse(characterWorldT1) · desiredHeadWorld`.
4. `AddEffectorTargetPosition(m_HIK, eff, &posModel, SolveStep::UPPER_BODY, Pass::MAIN, false, 3.0, true, 1.5)`.
5. `m_HIK.m_TargetReachT[eff] = 1.0`.
6. Optionally a rotation target for head orientation via `AddEffectorTargetRotationVector(..., Pass::MAIN)` + `m_TargetReachR[eff]`.

The engine clears and resets the pass after solving, so re-queue every frame.

## Open risks

- **Fighting the aim IK.** The aim/reach IK also queues PASS_MAIN targets (chest/arm rotation). If both the mod and the aim IK add targets in the same frame, both are solved; the pass step is the max, and per-effector targets are keyed by effector id (a second add to the same effector *updates* the first). Driving the head (effector 15) should not collide with the arm effectors, but interaction with any chest/spine target the game adds is untested.
- **Weight source.** `DriveAllCurrentEffectorControlValues` runs *before* the solve inside the pass; it eases `m_ReachT` toward `m_TargetReachT`. With `interpolation=false` the value snaps; with interpolation it ramps over several frames, so the body eases into the target rather than snapping. Choose per the desired feel.
- **`updateAllBones`.** `IKToCharacterState` takes `CCharacter::m_IkToCharacterUpdateAllBones`; whether the solved head propagates cleanly to the existing `SetJoint`-based head override (also in the same frame, later) needs in-game validation — the two overrides touch the same bones.
- **Model-space vs. the existing head override.** The shipped head override composes onto the *animated* head orientation in `UpdatePropEffects` (after the solve). If HumanIK also moves the head, the two must be reconciled (either let HumanIK own the head and drop the `SetJoint`, or keep `SetJoint` for orientation and use HumanIK only for spine bend).
- **`hkaPose` pointer.** `CharacterToIKState`/`IKToCharacterState` take the character's `hkaPose*`; they are invoked by the engine inside the pass, so an external caller only needs to *queue targets*, not run the solve — do not call `Solve` directly.
- **`UpdateIKForTargets` is inlined** in the release build (no standalone symbol); the ordering above is reconstructed from the inlined body in `UpdatePassFinalizePose_Parallel` and the dump's discrete function.
