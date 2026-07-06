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

`CButtonMapping::EMapping` partitions mappings by mode with `END_*` markers: generic, on-foot plus context-specific on-foot, generic vehicle, land (car/motorcycle), sea (boat/jetski), air (heli/plane), UI, weaponized wingsuit, and mech. On foot only `MAPPING_FIRE_RIGHT` exists — there is no fire-left; dual-wield alternates barrels internally off one input, and `FIRE_LEFT` is consulted only in vehicle/mounted contexts. The mapping-to-action table itself is data-built and not yet recovered; `CPlayerActionObserver` keys button-hint prompts off it. The default bindings ship as `settings/keymap_gamepad.bin` and `keymap_keyboard.bin` (RTPC containers, extractable with the Gibbed tools per the RE skill; see `docs/mod/controllers-and-roomscale.md` for the extracted gamepad layout).
