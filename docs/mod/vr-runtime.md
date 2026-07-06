# VR runtime: OpenXR, per-eye projection, per-eye resolution

The OpenXR runtime is built. The mod brings up a real OpenXR session against the game's own D3D11 device, drives the render camera from the located HMD pose, builds a per-eye off-axis projection from the headset's field of view, renders each eye at the HMD-recommended resolution, blits the two eye captures into a stereo swapchain, and submits a projection layer to the compositor — while the game's own desktop present stays suppressed and one eye is mirrored back into the game window. This doc describes what runs, in the order it runs, and gathers the headset-only checks the desktop build could not settle into a single playtest checklist at the end.

The implementation lives in `payload/src/vr/` (`mod.rs` owns the session and the frame API; `frame.rs` bridges a frame to the render hooks; `projection.rs` builds the off-axis matrices; `blit.rs` copies the eyes into the swapchain; `resolution.rs` drives the per-eye resize; `mirror.rs` presents the desktop mirror; `config.rs` is the config surface) plus the frame-loop wiring in `payload/src/hooks/game.rs` and the VR headpose source in `payload/src/headpose/xr.rs`. It is the runtime half that turns the desktop stereo prototype (rendering §11) into something a headset displays; the head/body scheme, the HUD, and controllers hang off it.

## The runtime loop

The whole loop runs on the game's main thread, inside the `game_update_render` hook (`payload/src/hooks/game.rs`), in this order:

- `vr::update()` pumps the OpenXR event queue once, drives bring-up/retry/teardown per config, and returns whether a session is running. The headpose source is set to `Vr` while a session is running and `Sim` otherwise, so the flatscreen mouse-look sim yields to the HMD pose for the frame.
- `vr::apply_native_resolution()` populates the engine's deferred display-mode state so the first eye's `Draw` prologue resizes the scene targets to the per-eye resolution. This must sit before the eye loop and before `frame_begin` (which holds the runtime lock for the frame).
- `vr::frame_begin()` runs `xrWaitFrame` → `xrBeginFrame` → `xrLocateViews`, returning a `FrameContext` that holds the runtime lock for the frame. `xrWaitFrame` is what paces the app against the compositor, replacing vsync (which stays suppressed via `BLOCK_FLIP`, rendering §7).
- The original `UpdateRender` runs (this frame's animation, so the head-bone anchor is current), then `vr::begin_render_frame` publishes the HMD headpose and the per-eye render parameters from the located views — but only when the runtime asked to render; otherwise the parameters are cleared and the camera hook falls back to flatscreen stereo for the non-submitted keep-alive draws.
- The existing double-`Draw` stereo path renders both eyes. A running session forces the stereo double-`Draw` on regardless of the flatscreen stereo toggle (the swapchain has a slice per eye), and blocks the flip for both eyes. The `SetupRenderCamera` hook (hooks::camera, rendering §2) applies each eye's off-axis projection and world offset from the render parameters.
- `vr::present_and_submit()` blits each captured eye into its swapchain slice and ends the XR frame (a world projection layer when rendered, an empty frame otherwise), consuming the frame context and releasing the runtime lock.
- `vr::present_mirror()` draws one eye into the game window, letterboxed, and presents the game swapchain — the only present this frame.

The per-eye render parameters do not flow through the frame's runtime lock: `frame_begin` holds that lock for the whole frame, and the camera hook runs during the eye draws, so the parameters are handed to it through a separate, independently locked slot (`vr::render_params`, in `frame.rs`). Nothing on the game thread may re-enter the runtime between `frame_begin` and the submit.

## Session lifecycle, degradation, and retry

Bring-up is the full chain: load the OpenXR loader (dynamically, `openxr_loader.dll` beside the payload DLL or `vr.loader_path`; the static loader route does not cross-build under xwin), create the instance with `XR_KHR_D3D11_enable`, acquire the HMD system, create the session against the graphics engine's existing `ID3D11Device`, and create the LOCAL reference space. The stereo swapchain (a single 2-slice texture array) is created lazily on the first rendered frame.

Any failure at any stage logs on the `vr` target and leaves the mod in flatscreen stereo; `vr::update` retries the whole bring-up every `vr.retry_interval_secs` while `vr.enabled`. This is the graceful-degradation contract: with no runtime installed the mod plays normally in flatscreen and simply keeps retrying. Turning `vr.enabled` off, a session transition to `EXITING`/`LOSS_PENDING`, instance loss, or a lifecycle shutdown all tear the runtime down in order (swapchain → session → instance) so the OpenXR instance never outlives the DLL across an uninject → reinject cycle.

## The cockpit pose model

The runtime tracks a **recenter baseline** (`vr::Baseline`): a position and a yaw-only orientation, captured from the latest located head pose. `recenter()` snapshots the current head pose into the baseline; F7 and the VR tab's Recenter button both route here (via `headpose::recenter`, which re-bases both the VR baseline and the mouse sim so one action recenters whichever source is live). The baseline is yaw-only on purpose — pitch and roll are the player's real head tilt and must not be zeroed, only the heading is re-based, matching the vehicle-recenter design in `docs/mod/head-and-body.md`.

Each frame, `frame_begin` locates the two eye views in LOCAL space and re-bases each into the baseline frame (`baseline⁻¹ · pose`, yaw-only inverse). `begin_render_frame` (`frame.rs`) then reduces the two eyes to a center pose (position averaged, the left eye's orientation), composes it into world space through `headpose::xr::compose` — the cockpit-frame pose is rotated by the body frame and placed on the animated head-bone anchor, scaled by `vr.world_scale` — and publishes it as the headpose. The per-eye world offset handed to the camera is the true per-eye delta from `locate_views`, rotated into world space by the body frame, replacing the flatscreen build's synthetic ±IPD/2 lateral offset.

The VR source publishes through `headpose::set_pose_no_interp`, writing the same pose to both sides of the engine's `T0 → T1` interpolation pair (**prev = cur**). The engine interpolates its camera by the sub-frame fraction `dtf` to smooth its fixed-rate sim tick; the VR source samples a fresh pose at the predicted display time every rendered frame, so there is no tick cadence to smooth and any residual interpolation would only lag the head behind the HMD (the "no smoothing on the HMD→camera path" pitfall in `docs/mod/head-and-body.md`).

## Per-eye camera and projection, and the two convention tweakables

`projection.rs` turns the four `XrFovf` half-angles into an off-axis (asymmetric-frustum) projection in the engine's row-major, row-vector layout (rendering §2.6), in two depth conventions, both host-unit-tested. The camera hook writes the projection into `m_Projection` on the render camera; the frame loop supplies the world-space eye offset alongside it.

The off-axis matrix is built element-for-element the way the engine's `CMatrix4f::PerspectiveOffCenter` builds `Camera::m_Projection` — verified against the release build (rendering §2.9) — and fed the engine's own default near/far (`0.1` / `38400`, the `Camera` constructor values). The `38400` far is the tell that matters: the game renders a finite-far reverse-Z frustum out to ~38 km, so an earlier `4000` far default would have clipped the horizon at 4 km; the mod now matches the engine's frustum.

The depth convention is settled but kept switchable as a headset escape hatch:

- **`vr.projection_convention`** (`EnginePreReverseZ`, default and verified-correct, vs `ManualReverseZ`). The preferred path writes a standard (non-reverse-Z) off-axis projection *before* `SetupRenderCamera`, so the engine applies its own reverse-Z remap and TAA jitter to it exactly once, matching every other camera (rendering §2.7). This is now confirmed against the decompile (rendering §2.9): `SetupRenderCamera` *consumes* whatever is in `m_Projection`, remapping it in place with `z' = w − z` — it never rebuilds from FOV/near/far — so the pre-call write reaches the GPU. The `ManualReverseZ` fallback (write an already-reverse-Z'd projection *after* `SetupRenderCamera` and rebuild the view-projections manually) is retained only as a runtime escape hatch in case the depth still reads wrong in-headset for a reason the desktop could not surface. The §2.7 wedge bug (a thin valid depth band, black elsewhere) would be the tell.
- **`vr.blit_srgb_gamma`** (`Linearize`, default, vs `Passthrough`). The captured eye texture is a `CopyResource` of `m_BackBufferLinear` as `R8G8B8A8_UNORM` (non-sRGB) holding display-referred bytes; the negotiated OpenXR swapchain is `_SRGB`, whose render-target view applies a hardware linear→sRGB encode on write. To reproduce the original bytes the blit shader linearizes the sampled color first, so the hardware re-encode cancels it out. `Passthrough` is for a genuinely-linear source or a non-sRGB swapchain. (The desktop mirror is a separate path with no gamma conversion — it writes into the game's own non-sRGB back buffer, which applies no encode, so passthrough there is a correctness conclusion, not a knob; see `mirror.rs`.)

Before trusting a real HMD *rotation*, clear the coordinate-frame gate: the `coord_frame` diagnostic (`RUST_LOG=coord_frame=debug`, added for this work) logs the render camera's `m_TransformF` basis rows and the travel-direction dot products, so one walk-forward session confirms JC3's world frame (almost certainly right-handed Y-up, but unverified). See `docs/mod/head-and-body.md`'s RE notes.

## Per-eye native resolution

While a session runs and `vr.native_resolution` is on (default), each eye renders at the runtime's recommended per-eye resolution × `vr.resolution_scale`, the same size the swapchain uses (one shared `scaled_eye_size`, so the blit is a straight scale-1 pass). The engine has no dynamic-resolution path (rendering §9): every render target is sized from `device->m_DeviceInfo.m_DisplayWidth`/`m_DisplayHeight` through `CreateRenderSetups`.

Rather than call `ApplyResize` directly, `resolution.rs` drives the engine's **own deferred display-mode state** — it writes the pending dimensions into `m_WindowWidth`/`m_WindowHeight` and sets `m_HasNewWindowSettings`, exactly as a windowed/settings resize does. The engine's `HandleModeChange`, serviced once per frame in the `Draw` prologue, then calls `ApplyResize` at the frame boundary the engine chose (previous dispatch drained, this frame not yet dispatched), so the idle-context assumption `ApplyResize` needs holds by construction (`docs/engine/render-setups-reinit.md` §2/§6). `ApplyResize` also resizes the DXGI swapchain buffers and sets the camera aspect ratio, and never touches the Win32 window (§4/§7); presenting is suppressed in VR, so the desktop effect is nil.

The pre-VR display size is captured before the first resize and restored both when the session ends and on uninject: a lifecycle cleanup requests the deferred restore while the hooks are still live, so the delayed hook uninstall (`lib.rs` `shutdown_startup`, its 100 ms grace windows) leaves the `Draw` prologue time to service it and the game is left exactly as found. Every serviced resize is verified (the device reports the requested size, and the Win32 window rect is unchanged); a timeout (`SERVICE_TIMEOUT_FRAMES`), a wrong size, or a changed window rect disables native resolution at runtime and restores the original size, and the mod continues at desktop resolution.

## The desktop mirror

While a session runs the compositor owns the HMD present and the engine's own present is blocked for both eyes, so the game window would freeze on a stale frame. `mirror.rs` presents the game's own swapchain itself, once per frame, unsynced (`SyncInterval = 0`): a vsynced mirror on a 60 Hz monitor would throttle the whole loop, including the 90 Hz HMD submit, down to the monitor's refresh. It draws the configured eye (`vr.mirror_eye`) into the game back buffer, letterboxed to the window aspect — the buffer is near-square at the per-eye resolution while the window keeps its 16:9 client rect, so a viewport inside the buffer pre-compensates DXGI's buffer→window stretch (unit-tested in `mirror.rs`). The egui debug overlay composites onto the mirror before the present so it stays visible on the desktop. Any draw/present fault disables `vr.mirror` at runtime and never wedges the loop; the window then holds its last frame.

## Recenter and the VR debug tab

Recenter is bound to **F7** (`payload/src/hooks/wndproc.rs`), edge-detected like the other function-key toggles (F5 uninject, F6 egui capture, F10 stereo capture, F11 shadow-PCF A/B) — F7 was the free key in that block. The same action is a Recenter button in two places in the debug overlay: the Camera tab's Headpose section and the dedicated **VR tab** (`payload/src/ui/vr.rs`). The VR tab shows the session state, the runtime name, the effective per-eye resolution, and the live headpose source, and exposes the `vr.enabled`, `vr.native_resolution`, `vr.mirror`, and `body_ik.enabled` toggles live.

## Config reference

All under `vr.*` (`payload/src/vr/config.rs`):

- `enabled` — master switch; off tears any live runtime down and stays in flatscreen stereo.
- `resolution_scale` — per-eye swapchain resolution scale on the runtime's recommendation (`1.0` = recommended).
- `retry_interval_secs` — bring-up retry cadence after a failure.
- `world_scale` — metres of head/IPD motion per engine unit (`1.0` = 1:1).
- `loader_path` — override for the OpenXR loader DLL.
- `near_clip` / `far_clip` — the per-eye projection clip planes, in metres. Default to the engine's own `Camera` constructor values (`0.1` / `38400`, rendering §2.9) so the frustum matches the game and the ~38 km horizon does not clip.
- `projection_convention` — `EnginePreReverseZ` (default) or `ManualReverseZ` (see above).
- `blit_srgb_gamma` — `Linearize` (default) or `Passthrough` (see above).
- `native_resolution` — render each eye at the HMD-recommended resolution (default on; auto-disables on a resize fault).
- `mirror` / `mirror_eye` — desktop mirror on/off and which eye (default on, left).

## Playtest checklist

Everything below needs a headset in the loop; the desktop build could not settle these, and each maps to a runtime knob or a diagnostic so a wrong guess is recoverable without a rebuild. Consolidated from the module doc comments and the branch's commit notes.

**Bring-up and lifecycle.**

- With a runtime present and a headset connected, confirm the `vr` log shows the loader loading, the runtime name and version, the D3D11 graphics requirements, the negotiated swapchain format, the swapchain creation line, and the `session state -> READY` transition, then a steady `VR frame-loop health` line every ~5 s.
- Confirm graceful degradation with no runtime available: the mod plays in flatscreen, logs the bring-up failure, and retries on the `retry_interval_secs` cadence (no panics, no crash).
- Toggle `vr.enabled` off and on in the VR tab mid-session: the runtime tears down cleanly (flatscreen resumes, resolution restores) and brings back up on re-enable.
- Uninject (F5) and reinject: the game stays running and stable, the display resolution is restored to the pre-VR size, and a fresh inject brings the runtime back up cleanly.

**Coordinate frame and pose.**

- Run the `coord_frame` diagnostic (`RUST_LOG=coord_frame=debug`) and walk forward to confirm the world frame before trusting HMD rotation (blocker 3, `docs/mod/head-and-body.md`).
- Confirm head rotation and roomscale translation track 1:1 with no perceptible lag or smoothing.
- Confirm Recenter (F7 or the VR tab button) re-bases heading only, leaving pitch and roll (real head tilt) intact.
- Confirm the stereo separation reads as depth (not a flat or hyperstereo image) and that near/far objects fuse — the true per-eye `locate_views` delta, not the synthetic IPD.

**Projection and color.**

- Confirm the world fills the frustum with no §2.7 depth wedge (a thin valid band with black elsewhere). If it wedges, flip `vr.projection_convention` to `ManualReverseZ`.
- Confirm colors and gamma look correct in the headset. If washed out or crushed, flip `vr.blit_srgb_gamma` to `Passthrough`.

**Resolution and mirror.**

- Confirm each eye renders sharp at the recommended resolution and that the `native resolution: engine resize serviced` line reports the expected per-eye size and a sane aspect ratio, with no runtime auto-disable.
- Confirm the desktop mirror shows the configured eye letterboxed at the correct aspect, that the egui overlay is visible on it, and that the mirror present never throttles the HMD frame rate.

**Body IK.**

- With `body_ik.enabled`, confirm the spine and neck bend toward where the player looks without fighting the head-bone override or the game's aim IK (`docs/engine/humanik.md` open risks); tune the reach weights in the VR/Camera tab.
