# The weapon aim and fire pipeline

Mapping the weapon-side aim/fire path well enough to give each wielded weapon its own aim ray — the right hand firing a gun one way while the left grapples another, or two dual-wielded guns firing at two targets. The camera-getter split and the body's native turn reactions to the aim direction are covered below in "The camera getters" and "Body-turn reactions to the aim direction"; `docs/mod/head-and-body.md`'s "The aim seam" covers the mod's design for how it overrides those getters and suppresses the turn reactions. This doc picks up from the camera matrix, through the single stored aim target, into the per-weapon fire construction.

All addresses are release-IDB (`JustCause3.exe.i64`, No-Denuvo) RVAs; the IDB carries full mangled symbols, so these were read directly, not mapped from the dump.

## The short version

Everything the player fires is aimed at **one point**. `CPlayerAimControl` casts a single ray from the camera transform and stores one target position per *purpose* (weapon, grapple, melee, grenade, sticky, snap) — not per weapon and not per hand. Each frame `CPlayer::UpdatePostCamera` reads the weapon slot's point and stamps it onto the character as `CCharacter::m_AimTargetPositionWeapons`. That single point fans out unchanged to every weapon the character is holding, where each weapon stores it as its own `m_AimTargetPosition` and, at fire time, builds its shot as *muzzle → that point*. Dual-wielded guns, wingsuit guns, and mounted guns all receive the identical point.

So the origin is already per-weapon (each weapon's own muzzle bone), but the direction all converges on one shared aim target. Giving each weapon an independent ray is a matter of substituting a per-weapon target point at one of three well-defined seams — the machinery to carry a distinct point per weapon already exists; it is simply fed the same value everywhere today.

## The aim target: `CPlayerAimControl`

One `CPlayerAimControl` hangs off the player's character (`CPlayer::m_CharacterSP`'s aim-control sub-object). It is a target *cache*, indexed by `CTarget::ETargetType`, six slots wide:

    TARGET_WEAPONS = 0   TARGET_STICKY_AIM = 1   TARGET_GRAPPLE = 2
    TARGET_MELEE   = 3   TARGET_GRENADE    = 4   TARGET_SNAP   = 5

Every array on the struct is `[6]`: `m_Target[6]`, `m_AimPos[6]`, `m_DirectPositions[6]`, `m_MaxAimDistance[6]`, and so on. There is exactly one weapon aim position — `m_AimPos[0]` — and its accessor is candidly named `GetAverageWeaponTargetPosition` (`0x140_2D..` in the dump; it simply `return m_AimPos[0]`), a historical name for what is a single point.

The per-frame update is `CPlayerAimControl::UpdatePostSim` (`0x140_CF5_C50`), which runs, in order:

- `UpdateInput` (`0x140_CE9_210`) — folds stick input, dead-zones.
- `UpdateRange` (`0x140_CA8_8C0`).
- `UpdateDirectAim` (`0x140_CE5_350`) — **the raycast**.
- `UpdateAllTargets` (`0x140_CE7_690`) — **target selection / aim assist**.
- `UpdateLastValidGrappleTarget`, `UpdateCrosshairLook`.

### The raycast (`UpdateDirectAim`, `0x140_CE5_350`)

A single ray. The origin comes from `GetRaycastStartPosition` (`0x140_C2B_610`): it calls `CGameCameraManager::GetCameraMatrix` (`0x140_75C_7C0` — see "The camera getters" below) and steps back 0.1 along camera-forward. The direction is camera-forward, adjusted by `GetAdjustedCameraMatrix` (`0x140_C3E_510`), which for the *current weapon* (`GetCurrentWeapon(TARGET_WEAPONS)`, `0x140_C65_E10`) adds a small ballistic pitch (`FOV × weapon tuning`) — a per-weapon-*type* offset, not a per-hand one. `CPhysicsSystem::CastRay` (`0x140_286_740`) runs it; `CheckDirectTargets` (`0x140_CE4_BB0`) writes the hit into `m_DirectPositions[0]` / `m_AimPos[0]`.

Because the origin is `GetCameraMatrix` and the mod overrides that getter, **the current aim ray already follows the HMD gaze** — this is exactly why the VR mod aims everything by looking. The whole raycast is single-threaded through the one camera transform.

### The camera getters

`GameCameraManager` splits its camera getters by phase:

- `GetInputMatrix` (`0x140_75C_7A0`) reads `m_NextRenderContext` — the render-phase context the mod patches, so movement-direction mapping follows the headpose.
- `GetCameraMatrix` (`0x140_75C_7C0`) reads `m_NextCameraContext`, which the *sim-phase* camera update rewrites from the internal camera **after** the mod's render-phase patch — so with the look input already consumed by that patch, the internal camera's yaw is frozen at its injection-time direction. Every sim-side aim consumer goes through this getter: `CPlayerAimControl` (the raycast above, plus `GetAdjustedCameraMatrix` and the target visibility casts), the weapon aim-target queries, and the melee and grapple tasks (see `docs/engine/grapple-pipeline.md`). The mod hooks `GetCameraMatrix` post-call and overwrites the output with the render camera's headpose transform, which is why the aim ray above already follows the HMD gaze.
- `GetAlternateAimMatrix` (`0x140_75C_830`) reads `m_NextRenderContext.m_AlternateAimTransform`, which the render-phase context patch already covers.

### Body-turn reactions to the aim direction

The `GetCameraMatrix` override has a side effect on the body: `NStateTask_LocoUtil::GetAimMoveAngle` (`0x140_831_880`) measures the XZ angle from the move direction to the player's aim target — which now follows the head when the mod's override is active — and the aim-relative act dispatchers (`QueueAimRelativeOnSpotActions`, `QueueRotateOnSpots`, `QueueStopTurns`, and friends) queue turn acts from it, so the game itself turns the body toward the aim direction whenever those acts run. The turn acts are only half of it: in the aim-relative family the game also passes `track_face_dir` to the orientation executor with its own blackboard face dir (again derived from the aim reference), so the executor's continuous yaw turns the body toward the aim direction independently of the turn acts. `docs/mod/head-and-body.md`'s "The aim seam" covers how the mod suppresses both reactions for the decoupled-idle head case (the `GetAimMoveAngle` zero-return shim, and `force_face_camera` forcing `track_face_dir` off).

### Aim assist (`UpdateAllTargets`, `0x140_CE7_690`)

Assist is applied *on the aim target*, upstream of any weapon. `UpdateAllTargets` scores candidate game objects with per-purpose fitness functions (`SetTargetWeaponsFitness`, `SetTargetSnapFitness`, `SetTargetStickyFitness`, …) driven by angle-to-camera-forward and distance, picks `m_Target[0]`, and the sticky/snap machinery nudges `m_AimPos[0]` toward the selected target. Separately, `NAutoAimToTarget_Update` (`0x140_809_C60`, already in `aim/aim.pyxis`) is the character-state task that rotates the aim/camera direction toward a locked target. Both are computed **once, from the single camera-derived aim** — magnetism is a property of the one gaze ray, not of any weapon.

Consequence for per-hand aiming: if we substitute a controller ray for a weapon *downstream* of `CPlayerAimControl` (at the character or weapon seam), that weapon gets **no** aim assist — assist lives entirely on the camera-coupled path and does not "follow" a per-weapon override. Per-ray assist would have to be re-derived, or the assist path itself re-pointed (see Seams).

## From aim control to the character: `CPlayer::UpdatePostCamera`

`CPlayer::UpdatePostCamera` (`0x140_CF7_BB0`) is the hand-off, each frame after the camera update:

1. `CPlayerAimControl::UpdatePostSim(...)` — recompute the target cache (above).
2. Read `m_AimPos[0]` into a local `weapon_target_pos`.
3. `CCharacter::UpdateWeaponAiming(character, &weapon_target_pos)` (`0x140_760_2C0`).
4. `CCharacter::UpdateGrenadeAiming(...)` — the grenade point (`m_AimPos[4]`).
5. For a driver, `CVehicle::SetAimTargetPosition(...)`.

`UpdateWeaponAiming` is a five-line setter. It writes the point into three floats and raises a one-frame flag:

    m_AimTargetPositionWeapons                 @ CCharacter + 0x26BC   (CVector3f)
    m_AimTargetPositionGrenade                 @ CCharacter + 0x26C8   (CVector3f)
    m_AimTargetPositionWeaponWasSetThisFrame   @ CCharacter + 0x2714   (bool)

The flag matters: if nothing set the weapon target this frame, `CCharacter::UpdateWeapons_Serial` falls back to a point 25 m along the character's *body* forward (`m_WorldMatrixT1` forward). AI and remote players reach the same field through other callers (`CSniperAimer`, the AI action `AiSetAimTarget`); the player path is the `UpdatePostCamera` one above.

**This single field — `m_AimTargetPositionWeapons` — is the narrowest global choke point for the player's weapon aim direction.** Everything below reads it.

## The fan-out to weapons: `CInventory`

`CCharacter::UpdateWeapons_Parallel` (`0x140_760_660`) passes `m_AimTargetPositionWeapons` and `m_AimTargetPositionGrenade` down as two `CVector3f`s to `CInventory::Update_Parallel` (`0x140_942_920`), which forwards them to `CInventory::UpdateWeapons_Parallel` (`0x140_91B_BB0`). That function drives each active weapon with the **same** `aim_position_weapons` argument:

- mounted-gun weapon (`GetWeaponInInventory(2)`) — `aim_position_weapons`;
- every wingsuit weapon (`m_Wingsuit->m_WingsuitWeapons[]`) — `aim_position_weapons`;
- the wielding weapon (`GetWieldingWeapon(...)`) — `aim_position_weapons`;
- the selected grenade — `aim_position_grenade`.

Each call is `weapon->UpdateFromUserPostSim_Parallel(dt, aim_position, inputs)` (`CWeaponBase::UpdateFromUserPostSim_Parallel`, `0x140_9A3_C50`). Inside, the weapon validates the point and stores it as its own field via the (release-inlined) `SetWeaponAimTargetPosition` write:

    m_AimTargetPosition   @ CWeaponBase + 0x3FC   (CVector3f)

So **every weapon now holds a private copy of the one shared point.** The copy is real per-weapon state — it is simply written from a single source.

## Dual-wield

Dual-wield is a weapon *slot*, `E_SLOT_DUAL_WIELD = 0` (the first `CInventory::EWeaponSlot`), distinct from `E_SLOT_TWO_HANDED = 1`, `E_SLOT_HEAVY`, etc. A dual-wield equip is a single `CWeaponBase` instance occupying that slot whose animation and fire pattern drive both hands (JC3's akimbo pistols are one weapon object with two muzzle fire-points in its `CWeaponData::m_InternalFirePositions`, alternated by `MarkNextWeaponComponentForFire`), not two independent `CWeaponBase`s. The inventory's `GetWieldingWeapon` returns that one instance; it receives the one `aim_position_weapons`. Both barrels therefore fire toward the identical target — there is no second aim slot, and no per-hand target state on the weapon.

(True two-gun independence would need either two `CWeaponBase` instances in two slots each driven with its own point, or a dual-wield weapon extended to carry two `m_AimTargetPosition`s and a `GetShotMatrix` that selects per fire-point. Today neither exists.)

## Fire direction, end to end (`GetShotMatrix`, `0x140_985_AA0`)

When a weapon fires, the launch transform is `CWeaponBase::GetShotMatrix` (`0x140_985_AA0`), called from the fire functions — `CWeaponBase::Fire_Projectile` (`0x140_991_FF0`), `CBulletWeaponBase::Fire_Projectile` (`0x140_927_550`), and their siblings. JC3 has no separate hitscan path for small arms: "bullet" weapons spawn fast projectiles through `Fire_Projectile`, so the muzzle → target construction below governs everything (`CBulletWeaponBase::GetTuning` at `0x140_914_7D0` decides explosive/water behaviour; `NWeaponUtil::Trajectory_GetAngleToHitPosition` arcs grenades/missiles). `UpdateDirectAim`'s raycast is only for the *aim target / reticle*; it is not the projectile's ray.

`GetShotMatrix` builds the shot as:

- **Origin** = `GetFireFromPosition` (virtual, `CWeaponBase::GetFireFromPosition` `0x140_966_940`): the weapon's world matrix `× m_InternalFirePositions[current]` — i.e. the **muzzle bone** of *this* weapon's current fire-point. Per-weapon, per-barrel. (Grip transforms for hand attachment are the separate `GetGripPosition`, `0x140_966_840` — see `docs/engine/hands-and-roomscale.md`.)
- **Aim point** = `m_AimTargetPosition` (`+0x3FC`), i.e. the stored shared target. For a remote player it is instead read off the network component.
- **Direction** = `CMatrix4f::CreateOrientation(from = muzzle, at = aim point, up)` — the shot points from *this weapon's muzzle* toward the *shared* target.
- **Fallbacks/modifiers**: if the aim point is within ~0.3 m of the muzzle, or the weapon is in `E_PROJECTILE_FIRE_DIRECTION_BARREL` mode (`m_ProjectileFireDirection` / `m_ProjectileTempFireDirection`, `+0x168`/`+0x16C` region), or `force_barrel_direction`, it uses the barrel-forward direction instead of the aim point (10 m along the weapon's own forward). `m_ProjectileOffsetFromAimPos` (`+0x1A4`) applies a spread ring, and scatter (`RotationYawPitchRoll` from tuning) is layered on unless suppressed.

The barrel-direction mode is worth noting: `SetProjectileFireDirectionMode` / `ResetProjectileFireDirectionMode` already let *some* weapon per-instance state choose "fire down my own barrel" over "fire at the aim point." That is the closest thing to a native per-weapon direction switch.

## The reticle

The HUD reticle is **singular** and comes from the same single aim point. `CHUDUI::UpdateGrappleReticle` projects `CPlayerAimControl`'s smoothed `m_AimPos` to screen as the first world-to-screen call of its frame, through `UIManager::Convert3DCoordsDefault`. The mod already hooks that (`payload/src/hooks/ui.rs`, `convert_3d_coords_default`) to reproject the reticle onto the floating panel and to record the aim depth (`payload/src/hud/aim.rs`, `record()` on the frame's first call — the later calls are the wire-attachment and grip-radius samples). There is one Scaleform reticle clip (`MCI_reticles`) and one recorded depth.

Two reticles would mean two projected world points and two on-panel sprites. The game's own reticle clip can only be one; a second reticle would have to be a mod-drawn element (the pattern already exists — `payload/src/hud/cursor.rs` draws the mod's own circle-dot quad for the UI mouse), positioned from the second weapon's aim point and given its own recorded depth for stereo vergence.

## Seams for per-weapon aim rays, ranked by invasiveness

The task: give each wielded weapon its own aim ray (origin already per-weapon; the win is an independent *direction*, i.e. an independent target point). Ranked least to most invasive:

**1. Per-weapon target write — detour `CWeaponBase::UpdateFromUserPostSim_Parallel` (`0x140_9A3_C50`), or the inlined `m_AimTargetPosition` field (`+0x3FC`).**
The lowest-blast-radius option. Each weapon already stores its own target; substitute a controller-derived point per `CWeaponBase` instance right before/after the per-weapon update, keyed on which weapon it is (e.g. right-hand vs left-hand slot). `GetShotMatrix` then naturally builds muzzle → your point for that weapon only. This is a small pre-call detour that overwrites one `CVector3f` per weapon. Origin is untouched (real muzzle bone). Downsides: no aim assist on the substituted weapons (assist never runs per-weapon); you must map weapon instance → hand/controller yourself; and the barrel-direction fallback triggers if your point lands too near the muzzle, so keep controller rays projected out to a sensible distance.

**2. Character-level split — detour `CCharacter::UpdateWeaponAiming` (`0x140_760_2C0`) / the `m_AimTargetPositionWeapons` field (`+0x26BC`).**
One point coarser: the single choke point every weapon reads. A detour here can set the *primary* weapon's target to the right-hand controller ray while leaving the left hand to a second mechanism. But because this one field fans out to *all* weapons unchanged, splitting two hands still requires reaching down to seam 1 for the second weapon — this seam alone can only move all weapons together. Best used to point the whole weapon set at a controller ray (single-gun VR aiming) rather than to split two guns.

**3. Re-derive assist per ray — additionally re-point the `CPlayerAimControl` inputs, or run a second lightweight target query per controller ray.**
Needed only if per-weapon aim assist / sticky-aim must follow each hand. `CPlayerAimControl` is structurally single-camera (one `GetCameraMatrix` origin, one `m_AimPos[0]`); there is no second weapon aim slot to borrow. Options: (a) accept no assist on VR-aimed weapons (recommended first cut — motion-controller aiming is direct and players expect no magnetism); (b) drive the existing raycast/assist from the controller ray instead of the camera for the primary weapon by overriding `GetRaycastStartPosition`/`GetAdjustedCameraMatrix` inputs, which gives assist to *one* weapon only; (c) build a parallel per-controller fitness query mirroring `UpdateAllTargets`, the most work. This is the invasive tier and should be deferred.

Recommended path: **seam 1 for independence** (each weapon gets its controller ray by writing its own `m_AimTargetPosition`), optionally **seam 2** as the simpler single-gun case (whole weapon set follows one controller). Leave assist (seam 3) off initially.

## Risks and coupling

- **The aim-flags coupling (the fps-movement lesson).** `Character::m_AimFlags` (`m_AimingWeapon` / `m_AimingGrapple`, `character/character.pyxis`, `+0x2_0F1`) and the aim timer gate whether the weapon is *raised* at all, whether the reticle shows, whether the body faces the aim direction, and whether assist runs. Substituting a fire *direction* does not by itself raise the weapon or enable firing — `CInventory::UpdateWeapons_Parallel` only drives a weapon when the animation allows shooting and the aim state is live. Per-hand aiming that wants the off-hand raised independently will collide with this single-aim-state model (there is one `m_AimingWeapon`, not one per hand), exactly the coupling `docs/mod/head-and-body.md` and the `fps-movement-aim-coupling` memory warn about. Treat weapon-raise/enable as a separate problem from aim-direction.
- **The body turns toward the aim point** (see "Body-turn reactions to the aim direction" above): `NStateTask_LocoUtil::GetAimMoveAngle` (`0x140_831_880`) measures the body's move angle against the *aim target*; the mod already neutralises this for the decoupled-idle head case. A per-weapon target that diverges hard from the head will re-excite the body-turn machinery through the primary weapon's point — validate that the character does not spin toward an off-axis controller ray.
- **The `WasSetThisFrame` fallback.** If a detour ever skips the write (`+0x2714` stays 0), the serial path aims 25 m down body-forward — a substitution seam must set the flag or route through `UpdateWeaponAiming` so the fallback never fires.
- **Barrel-direction fallback and near-muzzle points.** `GetShotMatrix` silently swaps to barrel-forward when the target is ~0.3 m from the muzzle; keep controller rays projected to a real distance so a near-hand target doesn't collapse to "fire down the barrel."
- **Network aim override.** For non-local controllers `GetShotMatrix` reads the aim point off the network component, not `m_AimTargetPosition`; the substitution seams are local-player only, which is the intended scope.

## RE notes — release addresses

Aim control (`CPlayerAimControl`):

    GetRaycastStartPosition            0x140_C2B_610
    GetAdjustedCameraMatrix            0x140_C3E_510
    UpdateDirectAim                    0x140_CE5_350
    CheckDirectTargets                 0x140_CE4_BB0
    UpdateAllTargets                   0x140_CE7_690   (aim assist / target select)
    UpdateRange                        0x140_CA8_8C0
    UpdateInput                        0x140_CE9_210
    UpdateCrosshairLook                0x140_CE8_BE0
    UpdatePostSim                      0x140_CF5_C50   (orchestrator)
    GetCurrentWeapon                   0x140_C65_E10
    GetRaycastMinDistanceStartPosition 0x140_C65_B10
    GetTargetBodyPosition              0x140_CDA_570
    NAutoAimToTarget_Update            0x140_809_C60   (already in aim/aim.pyxis)

Camera getters (see "The camera getters" above):

    GetCameraMatrix                    0x140_75C_7C0   (mod-overridden; aim origin)
    GetInputMatrix                     0x140_75C_7A0
    GetAlternateAimMatrix              0x140_75C_830

Player / character:

    CPlayer::UpdatePostCamera          0x140_CF7_BB0   (aim-control → character hand-off)
    CCharacter::UpdateWeaponAiming     0x140_760_2C0   (writes m_AimTargetPositionWeapons)
    NStateTask_LocoUtil::GetAimMoveAngle 0x140_831_880 (body-turn reaction to the aim direction)
    CCharacter::UpdateWeapons_Parallel 0x140_760_660
      m_AimTargetPositionWeapons               @ +0x26BC
      m_AimTargetPositionGrenade               @ +0x26C8
      m_AimTargetPositionWeaponWasSetThisFrame @ +0x2714

Inventory:

    CInventory::Update_Parallel        0x140_942_920
    CInventory::UpdateWeapons_Parallel 0x140_91B_BB0   (fan-out; same point to all weapons)

Weapon:

    CWeaponBase::GetShotMatrix                    0x140_985_AA0   (muzzle → m_AimTargetPosition)
    CWeaponBase::GetFireFromPosition (virtual)    0x140_966_940   (muzzle origin)
    CWeaponBase::GetGripPosition                  0x140_966_840   (hand grip; see hands doc)
    CWeaponBase::UpdateFromUserPostSim_Parallel   0x140_9A3_C50   (per-weapon target write)
    CWeaponBase::Fire_Projectile                  0x140_991_FF0
    CBulletWeaponBase::Fire_Projectile            0x140_927_550
    CBulletWeaponBase::GetTuning                  0x140_914_7D0
      m_AimTargetPosition   @ CWeaponBase + 0x3FC

`CTarget::ETargetType` slots: `TARGET_WEAPONS=0`, `TARGET_STICKY_AIM=1`, `TARGET_GRAPPLE=2`, `TARGET_MELEE=3`, `TARGET_GRENADE=4`, `TARGET_SNAP=5`. `CInventory::EWeaponSlot`: `E_SLOT_DUAL_WIELD=0`, `E_SLOT_TWO_HANDED=1`, `E_SLOT_HEAVY=2`, `E_SLOT_HAND_GRENADE=3`, … `E_NUM_WEAPON_SLOTS=8`.

Interface points with adjacent docs: the grapple aim target is slot `TARGET_GRAPPLE=2` (`GetActiveGrappleTargetPosition` → `m_AimPos[2]`), fed by the same `CPlayerAimControl` raycast and grapple-fitness scoring — grapple targeting internals belong to `docs/engine/grapple-pipeline.md`. Weapon *grip*/hand attachment (`GetGripPosition`, muzzle bones) belongs to `docs/engine/hands-and-roomscale.md`; this doc treats the muzzle only as the fire origin.
