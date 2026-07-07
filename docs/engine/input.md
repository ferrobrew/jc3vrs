# Input

Just Cause 3's local-player input runs through a single action-effector map plus a separate movie-facing mouse pipeline for the Scaleform UI. This is the engine's own read/write surface for both — the effector accessors, the action-ID table, the write API, an anti-tamper protection to avoid, the mouse/UI pipeline, and the semantic button-mapping layer above the raw actions — as ground truth for whatever mod code taps or drives it (see `docs/mod/input.md`).

## Action effectors

The engine exposes input through action effectors: `LocalPlayerActionMap::GetActionEffector` returns an `SEffectorElement` per action, and there is a `Freeze` path. The camera look axes are `LOOK_LEFT` / `LOOK_RIGHT` / `LOOK_UP` / `LOOK_DOWN` (net `yaw = right − left`, `pitch = up − down`), and the move axes are `MOVE_LEFT` / `MOVE_RIGHT` / `MOVE_FORWARD` / `MOVE_BACKWARD` (IDs 28–31). All of these resolve through the `Action` enum (see below). The gameplay buttons (fire, jump, grapple, reload, enter-vehicle, …) are likewise enumerated.

## The action IDs

The action-name-to-ID table (`action_name_table` at `0x142D99370`) is a fixed array of 255 name pointers indexed by ID, built once at startup — the numbering is byte-stable across builds, so the IDs are **hardcoded** rather than discovered live. All 255 are captured as the `Action` enum (`input/input_action_map.pyxis`), from `PAUSE = 0` through `GUI_USE_BUTTON = 0xFE`, and the effector accessors and the write API take it directly. (Runtime key/axis bindings load separately via `LoadActionMapFromFile`, but the action-ID numbering itself is static.) Live logging inside `GetActionEffector` was the original discovery route and is now moot since the enum exists.

## The write API

The write API is on `LocalPlayerActionMap` (its instance pointer is a global; the def models it as a singleton). Drive an action by ID: `ForceSetPressed(actionId, value)` for analog/held inputs (sticks, triggers, held buttons), and `ForceSetClicked(actionId)` for a one-frame press edge. Both guard against the null-effector sentinel that invalid IDs resolve to. The low-level effector mutators (`Click`, `Press`, `Freeze`, `ForceClick`) are also exposed on `InputDeviceEffector` if a specific effector needs driving directly.

**Timing.** Inject *after* `InputDeviceManager::Update(dt)` — the per-frame device poll — or the poll overwrites whatever you wrote. `Update` runs `UpdateForceClicks` after polling, which is the `m_ForceClick` mechanism: a click latched via `ForceSetClicked`/`ForceClick` sets the effector's `m_ForceClick` flag (`InputDeviceEffector+0xF`) so the press edge survives that poll instead of being cleared by it. So the robust pattern is to set held/analog values right after `Update`, and to use the force-click path for momentary buttons so they aren't lost to poll timing. (`InputDeviceManager::m_Enabled` gates the whole poll and doubles as a freeze handle.)

## Anti-tamper protection

`InputToOrbitModifier::CalculateInputDeltaAngles` carries an anti-tamper codemarker — do not hook it or anything inside it. `ProcessCameraContext` itself is clean.

## The mouse and UI pipeline (issue #9)

The game's UI mouse is fed by a single choke point, and it is position-from-Windows, deltas-from-DirectInput:

- **Position**: `WndProc` on `WM_MOUSEMOVE` calls `CUIManager::SetMousePos(x, y)` (window-client pixels, the only writer of `UIManager::m_MouseX/m_MouseY`), which immediately runs `CUIManager::SendMouseEvents`. `SendMouseEvents` also runs once per frame from `CUIManager::PreUpdate`.
- **Conversion**: `SendMouseEvents` maps client pixels to movie-viewport pixels by subtracting the centering offset `(m_CachedViewport − m_MovieScale) / 2`, then feeds `GFx::MouseEvent`s to `MovieImpl::HandleEvent` (vtable slot 35). A move event is only emitted on frames where the DirectInput mouse reported a non-zero delta; clicks come from the steering action map's `MOUSE1`/`MOUSE2` effector edges, the wheel from the DirectInput z axis. With a gamepad in use it parks the Scaleform mouse at `(−1000, −1000)`.
- **Cursor sprite**: the cursor is doubled — an OS `HCURSOR` (`CGraphicsEngine::SetCursor`, driven by `COverlayUI`'s show/hide refcount via `CUIManager::MousePointerVisibility`) and the movie's own `MCI_cursor` clip, repositioned in stage coordinates through `COverlayUI::SetMouseCursorPosition` on every move.

Two further engine seams bear on that pipeline:

- **The engine's reset path**: `CUIManager::RestoreAfterReset` (device reset, alt-tab reacquire) resizes the movie's viewport from the device without the back buffer necessarily changing size.
- **The map's own conversion**: `CCommMapUI::OnManageInput` — the only caller of `CUIManager::GetMovieSpaceMouseCursor` — converts `GetMousePos` window-client pixels to stage space through `m_MouseDeltaX/Y`/`m_MouseScaleFac`, which only `ComputeMovieSizeOnViewSize` writes.

For ground-truth debugging, `MovieImpl.MouseStates[0].LastPosition` (stage twips, `MovieImpl+0x2248`) holds the movie's post-`Advance` mouse position — the exact point the hit test used.

## The semantic button layer

`CButtonMapping::EMapping` partitions mappings by mode with `END_*` markers: generic, on-foot plus context-specific on-foot, generic vehicle, land (car/motorcycle), sea (boat/jetski), air (heli/plane), UI, weaponized wingsuit, and mech. On foot only `MAPPING_FIRE_RIGHT` exists — there is no fire-left; dual-wield alternates barrels internally off one input, and `FIRE_LEFT` is consulted only in vehicle/mounted contexts. The mapping-to-action table is **code-built** (not a data file), recovered below. `CPlayerActionObserver` keys button-hint prompts off it. The default *key/axis* bindings ship separately as `settings/keymap_gamepad.bin` and `keymap_keyboard.bin` (RTPC containers, extractable with the Gibbed tools per the RE skill; see `docs/mod/controllers-and-roomscale.md` for the extracted gamepad layout); those bind physical inputs to the action IDs, while the table below binds each semantic mapping to the action ID(s) it consults.

## The mapping → action table

Recovered from `CButtonMapping::PopulateMappings` (release `0x140C3BBA0`), which hardcodes every entry — there is no data file; `CButtonMapping::Init` calls `PopulateMappings` then `RefreshAllActionsForUi`. Each `EMapping` slot has an `SMappingInfo` holding a `CSteering::EAction m_Action1` (and optional `m_Action2` for chords), an `EButtonInteraction m_Interaction`, a `m_DescriptionKey` (the `buttonhint_*` localization key), optional `m_RequiredAbility` / `m_RequiredUpgrade` hashes, and the `m_Customizable` / `m_ShowInUIMapping` / `m_PCOnly` / `m_ControllerOnly` flags.

The action IDs are the same numbering as the `Action` enum in `jc3gi/src/input/input_action_map.rs` (`CSteering::EAction` ≡ that action space; the 2016 dump's decompiler renders some values as OR-expressions of an unrelated `CGame::EGameAction`, e.g. `A_SCREENSHOT|0xA0` = `0xD|0xA0` = `0xAD` = 173 = `FIRE_GRAPPLE` — those are display artifacts, the stored value is the integer). Verified end-to-end against the release `PopulateMappings`; the action IDs, description keys, ability/upgrade gates, and flags all match the dump.

**Layout drift (verified):** the 2016 dump's `SMappingInfo` is 72 bytes (`0x48`; its two `CHashString`s each carry a `boost::shared_ptr<string>`), but the **release** `SMappingInfo` is **28 bytes** — 7 dwords: `m_Interaction`, `m_Action1`, `m_Action2`, `m_DescriptionKey`, `m_RequiredUpgrade` (bare hash), `m_RequiredAbility` (bare hash), and a packed-flag dword (`Customizable`, `ShowInUIMapping`, `PCOnly`, `ControllerOnly`). The mapping→action semantics are identical across both.

Interaction legend: **tap** = `BINT_SINGLE_BUTTON_TAP`, **hold** = `BINT_SINGLE_BUTTON_HOLD`, **release** = `BINT_SINGLE_BUTTON_RELEASE`, **chord** = `BINT_TWO_BUTTON_HOLD_AND_TAP` (`m_Action1` held + `m_Action2` tapped). Every `EMapping` value has a populated entry **except** the `END_*` section markers, which stay at the constructor default (`m_Action1 = 255`, invalid) — so the table is complete.

### On-foot (generic + `FIRST_ON_FOOT_MAPPING`..`LAST_ON_FOOT_MAPPING` = `0x03`..`0x1C`)

| Mapping | Interaction | Action ID(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_PAUSE` (0x00) | tap | `GUI_PAUSE` (83) | generic |
| `MAPPING_COMMLINK` (0x01) | tap | `GUI_PDA` (111) | generic |
| `MAPPING_FIRE_GRAPPLING_HOOK` (0x03) | tap | `FIRE_GRAPPLE` (173) | ability `grappling_hook` |
| `MAPPING_RETRACT_TETHERS` (0x04) | tap | `RETRACT_GRAPPLE` (194) | ability `retract_tether` |
| `MAPPING_RELEASE_TETHERS_NON_CHORDED` (0x05) | tap | `RELEASE_GRAPPLE` (195) | PC only |
| `MAPPING_FIRE_RIGHT` (0x06) | tap | `FIRE_RIGHT` (11) | the sole on-foot fire input |
| `MAPPING_THROW_GRENADE` (0x07) | tap | `THROW_GRENADE` (13) | ability `grenades` |
| `MAPPING_DETONATE_EXPLOSIVE_GAMEPAD` (0x08) | hold | `DETONATE_EXPLOSIVE` (168) | ability `planted_explosives`; controller only |
| `MAPPING_DETONATE_EXPLOSIVE_PC` (0x09) | tap | `DETONATE_EXPLOSIVE_TAP` (166) | ability `planted_explosives`; PC only |
| `MAPPING_RELOAD` (0x0A) | tap | `RELOAD` (14) | |
| `MAPPING_JUMP` (0x0B) | tap | `JUMP` (33) | |
| `MAPPING_OPEN_WINGSUIT` (0x0C) | tap | `OPEN_WINGSUIT` (169) | ability `wingsuit` |
| `MAPPING_OPEN_PARACHUTE` (0x0D) | tap | `OPEN_PARACHUTE` (171) | ability `parachute` |
| `MAPPING_HAMMER` (0x0E) | tap | `PUSH_GRAPPLE` (174) | ability `hammer`; **melee reuses the grapple-push action ID**, `buttonhint_hammer` |
| `MAPPING_PRECISION_AIM` (0x0F) | tap | `PRECISION_AIM` (176) | upgrade `precision_aim` |
| `MAPPING_CANCEL` (0x10) | tap | `CANCEL` (248) | |
| `MAPPING_USE_ITEM` (0x11) | tap | `USE_ITEM` (36) | |
| `MAPPING_SELECT_DUAL_WIELD` (0x12) | tap | `SELECT_DUEL_WIELD` (234) | |
| `MAPPING_SELECT_TWO_HANDED` (0x13) | tap | `SELECT_TWO_HANDED` (235) | |
| `MAPPING_SELECT_TWO_HANDED_SPECIAL` (0x14) | tap | `SELECT_TWO_HANDED_SPECIAL` (236) | |
| `MAPPING_SELECT_EXPLOSIVE` (0x15) | tap | `SELECT_EXPLOSIVES` (183) | ability `planted_explosives` |
| `MAPPING_PREV_WEAPON` (0x16) | tap | `PREV_WEAPON` (16) | PC only |
| `MAPPING_NEXT_WEAPON` (0x17) | tap | `NEXT_WEAPON` (15) | PC only |
| `MAPPING_MOVE_FORWARD` (0x18) | hold | `MOVE_FORWARD` (28) | |
| `MAPPING_MOVE_BACKWARD` (0x19) | hold | `MOVE_BACKWARD` (29) | |
| `MAPPING_MOVE_LEFT` (0x1A) | hold | `MOVE_LEFT` (30) | |
| `MAPPING_MOVE_RIGHT` (0x1B) | hold | `MOVE_RIGHT` (31) | |
| `MAPPING_WALK` (0x1C) | hold | `WALK` (32) | PC only |

### Context-specific on-foot (`0x1E`..`0x2B`)

| Mapping | Interaction | Action ID(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_RELEASE_TETHERS_WITH_RETRACT` (0x1E) | tap | `RELEASE_GRAPPLE` (195) | controller only |
| `MAPPING_DETACH_MOUNTED_GUN` (0x1F) | tap | `RELOAD` (14) | |
| `MAPPING_CANCEL_REEL_IN` (0x20) | tap | `CANCEL` (248) | reel-in context |
| `MAPPING_SLINGSHOT_TO_REEL_IN` (0x21) | tap | `OPEN_PARACHUTE` (171) | reel-in context |
| `MAPPING_PLANT_EXPLOSIVE` (0x22) | tap | `PLANT_EXPLOSIVE` (167) | |
| `MAPPING_HANG_JUMP` (0x23) | tap | `JUMP` (33) | reel-in context |
| `MAPPING_SNIPER_ZOOM` (0x24) | tap | `PRECISION_AIM` (176) | **no upgrade gate** (unlike `MAPPING_PRECISION_AIM`); shares the precision-aim action ID |
| `MAPPING_SKIP_CUTSCENE` (0x25) | hold | `SKIP_CUTSCENE` (138) | |
| `MAPPING_LOOK_AT` (0x26) | hold | `LOOK_AT` (137) | |
| `MAPPING_AIR_BRAKE` (0x27) | hold | `CANCEL` (248) | upgrade `wingsuit_air_brake` |
| `MAPPING_SLINGSUIT_TO_REEL_IN` (0x28) | tap | `OPEN_WINGSUIT` (169) | upgrade `slingsuit_to_reel` |
| `MAPPING_HOLSTER_WEAPON` (0x29) | hold | `RELOAD` (14) | **holster = hold-reload** |
| `MAPPING_REEL_BOOST` (0x2A) | hold | `RETRACT_GRAPPLE` (194) | reel-in context (hold-retract) |
| `MAPPING_KEEP_RAGDOLLING` (0x2B) | hold | `CANCEL` (248) | hidden in UI mapping |

### Generic vehicle (`0x2D`..`0x32`)

| Mapping | Interaction | Action ID(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_LOOK_BACK_CAM` (0x2D) | hold | `LOOK_BACK_CAM` (5) | |
| `MAPPING_RECENTER_CAM` (0x2E) | tap | `VEHICLE_CAM` (6) | |
| `MAPPING_EXIT_VEHICLE` (0x2F) | tap | `EXIT_VEHICLE` (43) | hidden in UI mapping |
| `MAPPING_ATTACHED_TO_PARACHUTE` (0x30) | hold | `OPEN_PARACHUTE` (171) | |
| `MAPPING_TOGGLE_MAGNET` (0x31) | tap | `FIRE_VEHICLE_WEAPON_SECONDARY` (12) | hidden in UI mapping |
| `MAPPING_VEHICLE_RELEASE_GRAPPLE` (0x32) | tap | `VEHICLE_RELEASE_GRAPPLE` (175) | |

### Land vehicle — generic (`0x34`..`0x39`), car (`0x3B`..`0x40`), motorcycle (`0x42`..`0x48`)

| Mapping | Interaction | Action ID(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_HANDBRAKE` (0x34) | tap | `HANDBRAKE` (41) | |
| `MAPPING_ACCELERATE_LAND_VEHICLE` (0x35) | tap | `ACCELERATE` (37) | |
| `MAPPING_REVERSE_LAND_VEHICLE` (0x36) | tap | `REVERSE` (38) | |
| `MAPPING_NITROUS_LAND_VEHICLE` (0x37) | hold | `USE_VEHICLE_MOD` (130) | upgrade `land_vehicle_nitrous_unlock` |
| `MAPPING_TURBO_JUMP_LAND_VEHICLE` (0x38) | tap | `USE_VEHICLE_MOD` (130) | upgrade `land_vehicle_turbo_jump_unlock` |
| `MAPPING_SOUND_HORN_SIREN_LAND_VEHICLE` (0x39) | tap | `SOUND_HORN_SIREN` (42) | |
| `MAPPING_TURN_LEFT` (0x3B) | tap | `TURN_LEFT` (39) | car |
| `MAPPING_TURN_RIGHT` (0x3C) | tap | `TURN_RIGHT` (40) | car |
| `MAPPING_ENTER_GUNNER_SEAT` (0x3D) | hold | `ENTER_VEHICLE` (35) | hidden in UI mapping |
| `MAPPING_FIRE_VEHICLE_WEAPON_PRIMARY_CAR` (0x3E) | tap | `FIRE_VEHICLE_WEAPON_PRIMARY` (9) | |
| `MAPPING_FIRE_VEHICLE_WEAPON_SECONDARY_CAR` (0x3F) | tap | `FIRE_VEHICLE_WEAPON_SECONDARY` (12) | |
| `MAPPING_STUNT_JUMP_CAR` (0x40) | tap | `STUNT_JUMP` (110) | |
| `MAPPING_FIRE_WEAPON_MOTORCYCLE` (0x42) | tap | `MC_FIRE` (128) | personal weapon |
| `MAPPING_RELOAD_WEAPON_MOTORCYCLE` (0x43) | tap | `MC_RELOAD` (129) | |
| `MAPPING_HOLSTER_WEAPON_MOTORCYCLE` (0x44) | hold | `MC_RELOAD` (129) | holster = hold-reload |
| `MAPPING_TILT_FORWARD_MOTORCYCLE` (0x45) | tap | `BIKE_TILT_FORWARD` (44) | |
| `MAPPING_TILT_BACKWARD_MOTORCYCLE` (0x46) | tap | `BIKE_TILT_BACKWARD` (45) | |
| `MAPPING_LEAN_LEFT_MOTORCYCLE` (0x47) | tap | `BIKE_LEAN_LEFT` (68) | |
| `MAPPING_LEAN_RIGHT_MOTORCYCLE` (0x48) | tap | `BIKE_LEAN_RIGHT` (69) | |

### Sea vehicle — generic (`0x4A`..`0x4E`), boat (`0x50`..`0x51`), jetski (`0x53`..`0x55`)

| Mapping | Interaction | Action ID(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_NITROUS_SEA_VEHICLE` (0x4A) | hold | `USE_VEHICLE_MOD` (130) | upgrade `sea_vehicle_nitrous_unlock` |
| `MAPPING_TURBO_JUMP_SEA_VEHICLE` (0x4B) | tap | `USE_VEHICLE_MOD` (130) | upgrade `sea_vehicle_turbo_jump_unlock` |
| `MAPPING_TURN_LEFT_SEA_VEHICLE` (0x4C) | tap | `ACCELERATE` (37) | **actually forward** — `buttonhint_forward_sea_vehicle` |
| `MAPPING_TURN_RIGHT_SEA_VEHICLE` (0x4D) | tap | `REVERSE` (38) | **actually backward** — `buttonhint_backward_sea_vehicle` |
| `MAPPING_SOUND_HORN_SIREN_SEA_VEHICLE` (0x4E) | tap | `SOUND_HORN_SIREN` (42) | |
| `MAPPING_FIRE_VEHICLE_WEAPON_PRIMARY_BOAT` (0x50) | tap | `FIRE_VEHICLE_WEAPON_PRIMARY` (9) | |
| `MAPPING_STUNT_JUMP_BOAT` (0x51) | tap | `STUNT_JUMP` (110) | |
| `MAPPING_FIRE_WEAPON_JETSKI` (0x53) | tap | `MC_FIRE` (128) | personal weapon |
| `MAPPING_RELOAD_WEAPON_JETSKI` (0x54) | tap | `MC_RELOAD` (129) | |
| `MAPPING_HOLSTER_WEAPON_JETSKI` (0x55) | hold | `MC_RELOAD` (129) | holster = hold-reload |

### Air vehicle — generic (`0x57`..`0x58`), helicopter (`0x5A`..`0x63`), plane (`0x65`..`0x6E`)

| Mapping | Interaction | Action ID(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_FIRE_VEHICLE_WEAPON_PRIMARY_AIR_VEHICLE` (0x57) | tap | `FIRE_VEHICLE_WEAPON_PRIMARY` (9) | |
| `MAPPING_FIRE_VEHICLE_WEAPON_SECONDARY_AIR_VEHICLE` (0x58) | tap | `FIRE_VEHICLE_WEAPON_SECONDARY` (12) | |
| `MAPPING_HELI_INC_ALTITUDE` (0x5A) | tap | `HELI_INC_ALTITUDE` (52) | |
| `MAPPING_HELI_DEC_ALTITUDE` (0x5B) | tap | `HELI_DEC_ALTITUDE` (53) | |
| `MAPPING_HELI_ROLL_RIGHT` (0x5C) | tap | `HELI_ROLL_RIGHT` (51) | |
| `MAPPING_HELI_ROLL_LEFT` (0x5D) | tap | `HELI_ROLL_LEFT` (50) | |
| `MAPPING_HELI_TURN_LEFT` (0x5E) | tap | `HELI_TURN_LEFT` (48) | |
| `MAPPING_HELI_TURN_RIGHT` (0x5F) | tap | `HELI_TURN_RIGHT` (49) | |
| `MAPPING_HELI_FORWARD` (0x60) | tap | `HELI_FORWARD` (46) | |
| `MAPPING_HELI_BACKWARD` (0x61) | tap | `HELI_BACKWARD` (47) | |
| `MAPPING_STUNT_JUMP_HELICOPTER` (0x62) | tap | `STUNT_JUMP` (110) | |
| `MAPPING_NITROUS_HELICOPTER` (0x63) | hold | `USE_VEHICLE_MOD` (130) | upgrade `air_vehicle_nitrous_unlock` |
| `MAPPING_PLANE_TURN_RIGHT` (0x65) | tap | `PLANE_TURN_RIGHT` (58) | |
| `MAPPING_PLANE_TURN_LEFT` (0x66) | tap | `PLANE_TURN_LEFT` (57) | |
| `MAPPING_PLANE_ROLL_RIGHT` (0x67) | tap | `PLANE_ROLL_RIGHT` (60) | |
| `MAPPING_PLANE_ROLL_LEFT` (0x68) | tap | `PLANE_ROLL_LEFT` (59) | |
| `MAPPING_PLANE_PITCH_UP` (0x69) | tap | `PLANE_PITCH_UP` (55) | |
| `MAPPING_PLANE_PITCH_DOWN` (0x6A) | tap | `PLANE_PITCH_DOWN` (56) | |
| `MAPPING_PLANE_INC_THRUST` (0x6B) | tap | `PLANE_INC_TRUST` (61) | |
| `MAPPING_PLANE_DEC_THRUST` (0x6C) | tap | `PLANE_DEC_TRUST` (62) | |
| `MAPPING_STUNT_JUMP_PLANE` (0x6D) | tap | `STUNT_JUMP` (110) | |
| `MAPPING_NITROUS_PLANE` (0x6E) | hold | `PLANE_INC_TRUST` (61) | upgrade `air_vehicle_nitrous_unlock` |

### UI (`0x70`..`0x77`)

All UI mappings are tap and `m_Customizable = false`.

| Mapping | Action ID(s) |
|---|---|
| `MAPPING_UI_OK` (0x70) | `GUI_OK` (74) |
| `MAPPING_UI_CANCEL` (0x71) | `GUI_CANCEL` (75) |
| `MAPPING_UI_TAB_NEXT` (0x72) | `GUI_TAB_NEXT` (196) |
| `MAPPING_UI_TAB_PREV` (0x73) | `GUI_TAB_PREV` (197) |
| `MAPPING_UI_FILTER` (0x74) | `GUI_FILTER` (251) |
| `MAPPING_UI_RECENTER` (0x75) | `GUI_RECENTER` (252) |
| `MAPPING_UI_PAGE_NEXT` (0x76) | `GUI_PAGE_NEXT` (112) |
| `MAPPING_UI_PAGE_PREV` (0x77) | `GUI_PAGE_PREV` (113) |

### Weaponized wingsuit (`0x79`..`0x7E`)

| Mapping | Interaction | Action ID(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_WEAPONIZED_WINGSUIT_TAKEOFF` (0x79) | hold | `WINGSUIT_TAKEOFF` (142) | |
| `MAPPING_WEAPONIZED_WINGSUIT_BOOST` (0x7A) | tap | `WINGSUIT_BOOST` (146) | |
| `MAPPING_WEAPONIZED_WINGSUIT_BRAKE` (0x7B) | tap | `WINGSUIT_AIRBRAKE` (143) | |
| `MAPPING_WEAPONIZED_WINGSUIT_EVADE` (0x7C) | chord | `WINGSUIT_EVADE` (144) held + `MOVE_ALL` (145) tapped | |
| `MAPPING_WEAPONIZED_WINGSUIT_PRIMARY_WEAPON` (0x7D) | tap | `FIRE_WINGSUIT_WEAPON_MAIN` (170) | |
| `MAPPING_WEAPONIZED_WINGSUIT_SECONDARY_WEAPON` (0x7E) | tap | `FIRE_WINGSUIT_WEAPON_SECONDARY` (172) | |

### Mech (`0x80`..`0x89`) and unsectioned (`0x8B`..`0x8C`)

Mech move maps reuse plane/turn action IDs. `MAPPING_ACTIVATE_BAVARIUM_SHIELD` and `MAPPING_CHALLENGE_RESET_VEHICLE` sit past `END_MECH_MAPPINGS` with no `FIRST/LAST` section of their own.

| Mapping | Interaction | Action ID(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_MECH_MOVE_FORWARD` (0x80) | hold | `PLANE_PITCH_DOWN` (56) | reuses plane action ID |
| `MAPPING_MECH_MOVE_BACKWARD` (0x81) | hold | `PLANE_PITCH_UP` (55) | reuses plane action ID |
| `MAPPING_MECH_MOVE_LEFT` (0x82) | hold | `TURN_LEFT` (39) | |
| `MAPPING_MECH_MOVE_RIGHT` (0x83) | hold | `TURN_RIGHT` (40) | |
| `MAPPING_MECH_JUMP` (0x84) | tap | `MECH_JUMP` (121) | |
| `MAPPING_MECH_PUNCH` (0x85) | tap | `MECH_PUNCH` (105) | |
| `MAPPING_MECH_GRAVITY_GUN_ATTRACT` (0x86) | hold | `MECH_FIRE_GRAVITY_WEAPON_PRIMARY` (106) | |
| `MAPPING_MECH_GRAVITY_GUN_DROP` (0x87) | tap | `MECH_FIRE_GRAVITY_WEAPON_SECONDARY` (108) | |
| `MAPPING_MECH_GRAVITY_GUN_THROW` (0x88) | release | `MECH_FIRE_GRAVITY_WEAPON_PRIMARY` (106) | fires on button release |
| `MAPPING_MECH_FIRE_RIGHT_HAND_WEAPON` (0x89) | tap | `MECH_FIRE_RIGHT_HAND_WEAPON` (107) | |
| `MAPPING_ACTIVATE_BAVARIUM_SHIELD` (0x8B) | tap | `SOUND_HORN_SIREN` (42) | reuses horn action ID |
| `MAPPING_CHALLENGE_RESET_VEHICLE` (0x8C) | hold | `CHALLENGE_RESET_VEHICLE` (34) | |

### Gating (`IsMappingActive`) and consumption

`CButtonMapping::IsMappingActive(m, player)` (dump `0x1410C2C50`; release address not yet anchored — the release symbol is stripped, but the same logic is inlined into `PopulateMappings`'s neighbours) gates a mapping on two conditions, ANDed:

1. If `m_RequiredAbility.m_Hash != 0` and a player is present, the mapping is inactive unless `CAbilitiesHandler::IsEnabled(player, ability)` is true (e.g. grenades, grappling hook, wingsuit, parachute, planted explosives, hammer, retract-tether).
2. If `m_RequiredUpgrade.m_Hash != 0`, the mapping is inactive unless `CProfileManager::GetUpgradeInfo(upgrade)` reports both `m_IsPurchased` and `m_IsEnabled` (e.g. precision aim, the per-class nitrous/turbo-jump unlocks, wingsuit air brake, slingsuit-to-reel).

Otherwise the mapping is active. Ability/upgrade hashes are stored per entry; the mappings without either gate are always active.

The button-hint / observer layer consumes the table through two range queries, both taking a `[from, to_inclusive]` `EMapping` sub-range (the `FIRST_*`/`LAST_*` section bounds selected by the player's current mode) and an `only_active` flag that applies `IsMappingActive`:

- `GetMappingInfosForKey(player, key, from, to, only_active, force_keyboard, force_gamepad)` — walks the range and collects every `SMappingInfo` whose `m_Action1` **or** `m_Action2` resolves through `CUIBase::GetKeyID` to the given physical key (skipping the `255` sentinel). This answers "what does this button do right now".
- `GetMappingInfosForAction(player, action, from, to, only_active)` — the inverse: collects every mapping in the range whose `m_Action1` or `m_Action2` equals the given `CSteering::EAction`. This is how a button-hint prompt for an action finds its glyph.

`RefreshAllActionsForUi` builds `m_AllActionsForUi` (a `forward_list<CSteering::EAction>`) for the bindings/UI-mapping screen.

**Notable reuses to watch when driving VR input** (each is a single action ID doing double duty, gated only by context/mode): on-foot **holster is hold-`RELOAD`** and **melee/`MAPPING_HAMMER` is `PUSH_GRAPPLE`** (174); **`MAPPING_SNIPER_ZOOM` is `PRECISION_AIM`** (176) with no upgrade gate, while `MAPPING_PRECISION_AIM` is the same action ID gated on the `precision_aim` upgrade; nitrous/turbo-jump across every vehicle class are the same `USE_VEHICLE_MOD` (130) separated only by upgrade hash and tap-vs-hold; and the sea-vehicle "turn left/right" mappings are really forward/reverse (`ACCELERATE`/`REVERSE`).
