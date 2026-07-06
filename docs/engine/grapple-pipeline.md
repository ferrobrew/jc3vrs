# The grapple pipeline

Just Cause 3's signature mechanic: fire a hook at a surface or object to zip toward it (traversal), or tether two things together (attach), with reel-in. The VR goal being scoped is a two-handed split — the **left** motion controller aims and fires the grapple at whatever it points at, while the **right** hand independently aims and fires a weapon (`docs/engine/aim-pipeline.md`). This doc maps the grapple targeting and tether machinery well enough to identify the seam for re-sourcing the grapple's aim ray from an arbitrary transform.

All addresses are release-IDB (`JustCause3.exe`, imagebase `0x140000000`) unless marked *(dump)*, meaning a dump-build address still to be confirmed against the release binary. The release IDB carries MSVC symbols, so most functions resolved directly by name.

## The one-line summary

The grapple does **not** cast its own ray. It reads the same shared aim state the guns read — the single per-tick raycast in `CPlayerAimControl::UpdateDirectAim` — bucketed by a target-type index. Weapons are index 0, melee index 1, **grapple is index 2**. Both the ray origin and direction come from `CGameCameraManager::GetCameraMatrix` (`0x140_75C_7C0`), the same getter the mod already post-overrides with the headpose (see "The camera getters" above and `docs/mod/head-and-body.md` for the mod-side override). So overriding `GetCameraMatrix` moves the gun aim *and* the grapple aim together. Splitting them is not a caller-keyed problem — it is an **index** problem, and the clean intervention is to overwrite the grapple slot (index 2) after the shared raycast has run.

## 1. Targeting: how the target point and entity are chosen

There is no grapple-specific raycast. `CPlayerAimControl` owns aim for the local player and runs one physics ray per sim tick that services every aim consumer.

- `CPlayerAimControl::UpdateDirectAim` (`0x140_CE5_350`) is the per-tick driver. It builds the ray from the camera:
  - **Direction** from `CPlayerAimControl::GetAdjustedCameraMatrix` (`0x140_C3E_510`, static), which reads `GetCameraMatrix` (or `GetAlternateAimMatrix` when the alternate-aim transform is active) and optionally tilts it by a per-weapon pitch factor scaled by FOV. Forward is the matrix's `-Z` column.
  - **Origin** from `CPlayerAimControl::GetRaycastMinDistanceStartPosition` (`0x140_C65_B10`), a weapon-barrel/head-blended start also derived from that camera matrix; the simpler `CPlayerAimControl::GetRaycastStartPosition` (`0x140_C2B_610`) just steps `0.1` back along the camera forward.
  - It then calls `CPhysicsSystem::CastRay` once and hands the single hit to `CPlayerAimControl::CheckDirectTargets` (`0x140_CE4_BB0`).
- `CheckDirectTargets` loops over the target types (weapon, grapple; melee is skipped here) and fills, per type, the parallel arrays on `CPlayerAimControl`: `m_DirectTargets[]`, `m_DirectHits[]`, `m_DirectPositions[]`, `m_AimPos[]`, `m_IsInRange[]`, `m_MaxAimDistance[]`. For the grapple it also maintains the "last valid grapple" cache — `m_LastHitGrappleTargetGO`, `m_HasLastValidGrapple`, and `m_DirectGrapplePosIsGrapplable` — so a momentarily-lost target can persist.

Where the chosen grapple result is read:

- `CPlayerAimControl::GetActiveGrappleTargetPosition` (`0x140_96E_750`) returns `&m_AimPos[2]`.
- `CPlayerAimControl::GetCurrentGrappleTargetGO` (`0x140_C55_4B0`) / `GetCurrentGrappleTargetWeakGO` (`0x140_C2B_5C0`) return the cached grapple game object, gated on `m_HasLastValidGrapple && m_IsInRange[2]`.
- `CPlayerAimControl::IsLastValidGrappleAimPosStillValid` (`0x140_C3E_5F0`) re-validates the cached grapple aim by re-reading `GetCameraMatrix` — one of the few grapple-*specific* callers of the camera getter.

So the answer to "does it use `GetCameraMatrix` or its own raycast": it uses `GetCameraMatrix`, twice (origin and direction), through the shared `UpdateDirectAim` ray, and buckets the result into the grapple slot (index 2). The chosen target lives in `CPlayerAimControl::m_AimPos[2]` (world position) plus `m_Target[2]` / `m_LastHitGrappleTargetGO` (entity).

### The grapple's own read of that state

When the player fires, the hook latches the aim state into a cached target rather than re-reading it live:

- `CGrapplingHook::SetUpTarget` (`0x140_939_CF0`, static) is the capture point. Signature `(CGrapplingHook*, CWireEnd::SHookTargetInfo& out, CTarget::ETargetType type)`. It reads `m_AimControl` at `CGrapplingHook+0x158`, then `m_AimPos[type]` (byte offset `0x14C` into `CPlayerAimControl`, stride 12) and `m_Target[type]`, transforms the world aim position into the target object's local space, and writes a `CWireEnd::SHookTargetInfo`. When there is no direct `CTarget` it falls back to `GetCurrentGrappleTargetGO` **only for `type == 2`** (grapple). `SetupTarget` has two overloads — `CWireEnd::SHookTargetInfo::SetupTarget(CTarget*, CVector3f local)` (`0x140_926_C00`) and `(CGameObject*, CVector3f local)` (`0x140_926_E50`).
- `CGrapplingHook::GetLastGrappleTargetPos` (`0x140_93A_4B0`) returns that cached target's current world position. Downstream fire and IK read this, not live `CPlayerAimControl`.

This matters for the seam: because the grapple's use of the aim state is keyed by the index `2`, it is cleanly separable from the weapon's index-0 use without knowing the caller.

## 2. Fire and attach: not instant — a short hook flight

Between "target chosen" and "hook attached" there is an animation-gated wire *extension*, i.e. a genuine (short) projectile flight of the hook from the hand toward the cached target. The pipeline:

1. `CGrapplingHook::TryFireHook` (`0x140_943_2E0`) computes the fire direction from the cached `SHookTargetInfo` and calls the animation fire.
2. `CGrapplingHook::DoActFire` (`0x140_8FE_D80`, static). Signature `(const CVector3f& direction, boost::shared_ptr<CCharacter>&, bool dual_tether, bool reel)`. It takes a **direction**, not a position, and only selects a directional fire animation sector (angle → `ACT_GRAPPLE_FIRE_*`, or `ACT_GRAPPLE_TETHER_FIRE_*` when dual-tether) and `QueueAct`s it. No wire is created here; the callbacks (`FireHookQueueActCallback`, `FireDualTetherQueueActCallback`, `FireHookGrappleReelQueueActCallback`) only notify. The act is re-issued each tick until `CCharacter::IsGrappleFiring` goes true.
3. The fire animation emits a track message; on the "release the hook now" frame the hook is spawned in `CGrapplingHook::OnPostCamUpdate` (`0x140_94E_D60`): it reads the hand/shoulder position, creates the wire with `CGrapplingHook::CreateNewWire` (`0x140_923_BC0`), pins the near end to the device, and starts the far end extending with `CWireEnd::SetExtend` (`0x140_93D_700`, `(const CVector3f& start, float speed, SHookTargetInfo&, bool ghost)`). `SetExtend` puts the far end into an EXTENDING state at a tuning speed aimed at the cached target — this is the flight.
4. On reaching/hitting, `CWireEnd::Attach` *(dump `0x140CA1A50`)* builds an attachment proxy (`CGrapplePointProxy`) and moves the end to ATTACHED. A wire end attaches to a **proxy**, not a raw coordinate — `CWireEnd::AttachToProxy` *(dump `0x140C297F0`)*, `CWireEnd::AttachToCharacterBone(shared_ptr<CCharacter>&, int bone, const CMatrix4f& localOffset)` *(dump `0x140C350F0`)*. A `CGrapplingHookWire` holds two `CWireEnd`s: `[0]` near/device, `[1]` far/hook. So a tether is entity/bone/world-proxy per end, not a two-point pair of bare coordinates.

## 3. Traversal versus tether versus retract

The three modes are distinguished by input and by `CGrapplingHook::EGrapplingHookState` (`GHS_INACTIVE`, `GHS_REELING_IN`, `GHS_REELED_ATTACHED`, `GHS_REELED_HANG`, `GHS_REELED_UPSIDEDOWN`, `GHS_REELED_STUNT`, `GHS_CUSTOM_ACTIVE_WIRE`; `SetState` is the mutator). Input is dispatched from `CGrapplingHook::OnPreSimUpdate` (`0x140_949_F70`) and `OnPostCamUpdate`, reading a `CActionMap`. The relevant `Action` IDs (from `input/input_action_map.pyxis`, and see `docs/engine/input.md`):

- **`FIRE_GRAPPLE` (`0xAD`)** — the mode selector, tap versus hold against the tuning hold-time:
  - **tap** with a wire already attached → **reel-to-target / zip**: `SetState(GHS_REELING_IN)`, `QueueAct(ACT_PRE_REEL)`, reel completes into a `GHS_REELED_*` state via the reel tasks (`NStateTask_ReelIn`, `StartReelAttach` / `StartReelHangAtPosition` / `StartReelUpsideDown`).
  - **hold** → **dual tether**: fires a *second* wire whose far end dual-attaches, connecting two things (`OnDualTetherAttach`). Multi-tether count is upgrade-gated (`GetMaxDualTethers`), and the oldest tether is broken when over budget (`BreakOldestDualTetherIfNeeded`).
- **`RETRACT_GRAPPLE` (`0xC2`)** — retract/pull. `CGrapplingHook::RetractActiveWire` (`0x140_8F7_A10`) destroys the constraint and detaches the near end. `CGrapplingHook::HandleGrapplePull` (`0x140_8F7_C20`, thin in release) is the "yank a light object toward you" path: slerp a pull direction and apply an impulse, then retract.
- **`RELEASE_GRAPPLE` (`0xC3`)** — release tethers: `ReleaseAllDualTethers`, `DetachAllDualTetheredWiresAttachedToGameObject`.
- **`PUSH_GRAPPLE` (`0xAE`)**, and the reeled-in actions `REELED_IN_JUMP_ACTION` (`0x87`) / `REELED_IN_RELEASE_ACTION` (`0x88`) drive the reeled states.

The mod keeps all of this semantics untouched; it only wants to re-source the ray that feeds the grapple slot. The tap/hold/release dispatch, the states, and the act queueing stay native.

## 4. The seam: re-sourcing the grapple ray from the left controller

**The core finding.** The grapple and the guns share one raycast. That single ray reads `GetCameraMatrix` once (via `GetAdjustedCameraMatrix` and `GetRaycastMinDistanceStartPosition`) and `CheckDirectTargets` buckets the one hit into *both* the weapon slot (index 0) and the grapple slot (index 2). Consequences:

- The existing `GetCameraMatrix` post-override (headpose) already reaches the grapple — but it moves the weapon aim with it, so it cannot split left-hand grapple from right-hand gun.
- **Caller-keying `GetCameraMatrix` by return address does not work.** You *could* tell the grapple-specific callers apart (`IsLastValidGrappleAimPosStillValid`, `NStateTask_InAirGrappleFireMotionTask::OnEnter` at `0x140_810_B60`, both call the getter directly), but the load-bearing read is inside `UpdateDirectAim`'s shared raycast, whose single `GetCameraMatrix` call serves weapon and grapple at once and cannot be attributed to one hand. Reject return-address keying.

Ranked by invasiveness:

1. **Overwrite the grapple slot after the shared raycast (recommended).** Post-hook `CPlayerAimControl::UpdateDirectAim` (`0x140_CE5_350`) for the local player. After the original runs, cast a grapple ray from the left-controller transform and overwrite **only** index 2 of the aim arrays: `m_AimPos[2]`, `m_DirectPositions[2]`, `m_DirectTargets[2]`, `m_DirectHits[2]`, `m_IsInRange[2]`, plus the grapple cache (`m_LastHitGrappleTargetGO`, `m_HasLastValidGrapple`, `m_DirectGrapplePosIsGrapplable`). The weapon slot (index 0) is left camera/right-hand-sourced. This is the **same pattern as the existing `GetCameraMatrix` post-call override** (overwrite a getter's output for a consumer), but scoped by target-type *index* rather than by function — so every grapple consumer inherits it for free: `SetUpTarget`/fire, `GetActiveGrappleTargetPosition`, the reticle (§5), `IsLastValidGrappleAimPosStillValid`, and the fire arm IK via `GetLastGrappleTargetPos`, while the gun is untouched. The cost is reproducing the grapple half of target selection: for surface zip a plain physics ray hit written into `m_DirectPositions[2]`/`m_AimPos[2]` with `m_DirectHits[2]` suffices; for entity attach and dual-tether targeting the game's grapple fitness/target scoring must be re-run against the left-controller ray to populate `m_Target[2]`/`m_LastHitGrappleTargetGO` faithfully. Field offsets to confirm on the release struct: `m_AimPos` at byte `0x14C` (stride 12), the target/hit/range arrays adjacent (see `CheckDirectTargets`).

2. **Overwrite at fire capture only (fire-correct, reticle-wrong).** Hook `CGrapplingHook::SetUpTarget` (`0x140_939_CF0`) for the local player and overwrite the produced `SHookTargetInfo` (or the `m_AimPos[2]`/`m_Target[2]` it reads at `CGrapplingHook+0x158`) from a left-controller raycast. This corrects the actual hook target and thus `GetLastGrappleTargetPos` and the fire arm IK, but **not** the on-screen grapple reticle or `IsLastValidGrappleAimPosStillValid`, which read `m_AimPos[2]` directly — so the marker would show the camera target while the hook flies at the controller target. Use only if the reticle mismatch is acceptable, or combine with a reticle-only patch (§5).

3. **Move the whole grapple aim via the existing camera override, time-sliced.** Not recommended; there is no per-frame way to make one `GetCameraMatrix` call return two different matrices for the two hands within the shared raycast.

Recommendation: option 1. It is the minimal per-consumer intervention, mirrors the established post-getter-override idiom, and keeps the game's tap/hold/tether/retract semantics and all downstream consumers native.

## 5. Related seams

### Arm IK during grapple (interface point for `docs/engine/hands-and-roomscale.md`)

Two distinct IK tasks with two distinct target sources — the hands agent should note which is which:

- `NGrapplingFireArmIK::Update` (`0x140_814_C80`) — the **fire** arm aim. Its target is `CGrapplingHook::GetLastGrappleTargetPos` (`0x140_93A_4B0`), i.e. the cached fire target, the same point the hook flies to. It computes `aim_dir = target − left_shoulder`, projects a wanted left-hand position at arm length, and writes it to the blackboard (hand-position and weight keys). Not camera-sourced, not the live wire endpoint. Under option 1 above this follows the left controller automatically.
- `NReelInAimIK::Update` (`0x140_841_D70`) → `NReelInAimIK::UpdateAimEffector` (`0x140_838_A80`) — the **reel-in** arm aim, gated by a blend-weight blackboard key. Its aim target is the **live** `CPlayerAimControl` aim position (read at `m_AimControl + 0x14C`, i.e. `m_AimPos`), fed to `CHumanIK::AddEffectorTargetRotation` at the upper-body solve step; a secondary chest/spine effector uses a reel target stored on the `CGrapplingHook`. Because this reads live `m_AimPos` (not the grapple index specifically — confirm which index at that offset), the hands agent should verify whether the reel aim should follow the left controller, the head, or stay on the camera during reel-in.

### The grapple reticle

The grapple has its **own** reticle, separate from the weapon reticle:

- `CHUDUI::UpdateGrappleReticle` (`0x140_E29_410`) projects the grapple aim world position — `m_AimControl->m_AimPos[2]` (lerped from `m_LastAimPos[2]`) — to screen via `CUIManager::Convert3DCoords`. When the aim is using a direct position it pins to stage centre instead. Visibility is gated by `CGrapplingHook::ShouldShowAttachedUI`.
- `CHUDUI::UpdateWeaponReticle` *(dump `0x1412AC340`)* only *reads* `ShouldShowAttachedUI` to suppress itself while grappling.
- Wire tension and attach-type overlays are pushed separately by `CGrapplingHook::PushWireDataToUI` *(dump `0x140C3F900`)*.

Because the reticle projects from `m_AimPos[2]`, option-1's slot overwrite makes the marker track the left controller with no extra work. Option 2 would need a matching reticle patch here.
