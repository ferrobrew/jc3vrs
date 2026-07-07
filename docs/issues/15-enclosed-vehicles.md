# Issue #15: Enclosed vehicles — no external visibility in first-person VR

Research into the enclosed-vehicle problem and viable approaches to give the
player external visibility when inside a vehicle with no windows.

**Status: open research.** Nothing here has shipped — there is no
enclosed-vehicle handling in the payload yet. The approaches below are
candidates, and the "leading candidate" section names the current front-runner,
not a committed design.

## The problem

Some JC3 vehicles — the CS Baltdjur (armoured truck), tanks, and other
fully-enclosed vehicles — have no windows to see through. In the native
third-person camera this is fine, but in first-person VR the camera is inside
the vehicle mesh with no view of the road, obstacles, or enemies.

## The vehicle camera path

### IsInDrivingVehicleState

`CCharacter::IsInDrivingVehicleState` (release `0x140_77E_AF0`) checks the
player's animation state machine: it returns true if the current state hash
matches one of the driver-vehicle states (`S_IDLE_DRIVER_VEHICLE`,
`S_FORWARD_TO_REVERSE_DRIVER_VEHICLE`, `S_REVERSE_TO_FORWARD_DRIVER_VEHICLE`,
`S_REVERSE_DRIVER_VEHICLE`), or if the `m_UserVM.m_CurrentStateBitFlags[1] &
0x2000000000` flag is set.

### PushRenderContext and the vehicle bypass

`CGameCameraManager::PushRenderContext` (dump address
`0x140_A88_730`) is where the gameplay camera's world transform is pushed to
the engine camera. It has an `IsInDrivingVehicleState` branch:

```c
LocalPlayerCharacter = CCharacter::GetLocalPlayerCharacter();
if ( !LocalPlayerCharacter || !CCharacter::IsInDrivingVehicleState(LocalPlayerCharacter) )
{
    // On-foot: apply jitter filter (s_use_jitter_filter)
    // ... jitter filter logic ...
    goto LABEL_15;
}
// Vehicle: skip jitter filter, go directly to InitTransform
if ( s_use_jitter_filter_when_driving )
    goto LABEL_22;  // apply jitter filter
LABEL_14:
p_m_LastRenderTransform = &mat;
LABEL_15:
NGraphicsEngine::CCameraManager::InitTransform(..., p_m_LastRenderTransform);
NGraphicsEngine::CCameraManager::InitFOV(..., m_FOV);
```

In the vehicle path, the jitter filter is skipped (unless
`s_use_jitter_filter_when_driving` is set), and the raw camera matrix from the
`CGenericVehicleModifier` is passed directly to `InitTransform`. This means
the vehicle camera's world position comes from the vehicle modifier's
position/look-at calculation — which places the camera *behind* the vehicle
in third-person.

### The vehicle camera modifier

`CGenericVehicleModifier` (dump: `CGenericVehicleModifier.cpp`) is the camera
modifier for all generic vehicles. It has three camera states
(`EVehicleCameraState`):

```
eDRIVING_CAMERA    = 0  (cars, trucks, tanks)
ePLANE_CAMERA      = 1
eHELICOPTER_CAMERA = 2
```

The modifier holds the camera's position (`m_Position`), look-at offset
(`m_LookAtOffset`), follow distance (`m_FollowDistance`), and a look stick
for free-look. The camera position is computed relative to the vehicle's
matrix, with springs for lag, velocity-based offsets, and collision. This is a
**third-person follow camera** — it sits behind and above the vehicle.

The mod's camera hook overrides the camera position to the head bone position
(via `camera_update_render` and `camera_tree_update_render_contexts`), which
places the camera inside the vehicle at the driver's head — correct for
open vehicles, but inside the mesh for enclosed ones.

## Vehicle type and model data

### EVehicleType

`NVehicle::EVehicleType` (`NVehicle/EVehicleType.h`) classifies vehicles:

```
ECAR        = 0
EHELICOPTER = 1
EBOAT       = 2
EAIRPLANE   = 3
EMOTORCYCLE = 4
ESUB        = 5
ETRAIN      = 6
```

This is stored on `CVehicle::m_VehicleType` (offset 39 in `CVehicle.h`). It
distinguishes broad categories but does not indicate whether a vehicle has an
enclosed interior — a convertible car and an armoured truck are both `ECAR`.

### Vehicle model identification

`CVehicle` has:
- `m_NameHash` (`SObjectNameHash`) — a hash of the vehicle's model name.
- `m_Name` (`std::string`) — the vehicle's name string.
- `m_NameHashString` (`CHashString`) — a hash-string wrapper.
- `m_ResourceCache` (`SResourceCache *`) — the vehicle's resource cache, which
  holds the ADF data (model, textures, parts).

The vehicle's mesh/interior geometry is loaded from the resource cache as
part of the ADF (Avalanche Data Format) system. There is no per-vehicle
"enclosed" flag in the struct — the interior is defined by the mesh geometry,
not metadata.

### Detecting enclosed vehicles

Since there is no engine flag for "enclosed interior," detection must be
either:

1. **Curated list:** A hardcoded list of vehicle name hashes known to be
   enclosed (CS Baltdjur, tanks, etc.). This is simple and reliable but requires
   manual maintenance. The vehicle name is available via `CVehicle::m_Name`.

2. **Runtime depth probe:** Cast a ray from the camera position (driver's head)
   in several directions. If all rays hit geometry within a short distance
   (< 0.5 m), the vehicle is enclosed. This is robust but requires physics
   raycast access and runs every frame.

3. **Mesh analysis:** Check the vehicle's model for transparent materials
   (windows). If no transparent geometry exists near the driver seat, the
   vehicle is enclosed. This requires traversing the model instance hierarchy.

The curated list is the most practical for an initial implementation.

## Viable approaches

### Option A: In-vehicle render target (external camera periscope)

Render the scene from an external camera (e.g., on the vehicle roof or front)
into a texture, then display that texture on a surface inside the cockpit.

#### The ExternalRenderCamera mechanism

The engine's `CRenderPass` has an `ExternalRenderCamera` field (confirmed in
dump: `NGraphicsEngine.cpp`, line 139948):

```c
ExternalRenderCamera = this->ExternalRenderCamera;
if ( ExternalRenderCamera )
{
    NGraphicsEngine::CCamera::SetupRenderCamera(ExternalRenderCamera, 0);
    NGraphicsEngine::CRenderPass::SetRenderContextCamera(&this->m_RenderContext, this->ExternalRenderCamera);
}
else
{
    RenderCamera = CCameraManager::GetRenderCamera(...);
    NGraphicsEngine::CRenderPass::SetRenderContextCamera(&this->m_RenderContext, RenderCamera);
}
```

This means a `CRenderPass` can use a completely different camera than the main
render camera. The `ExternalRenderCamera` is a full `CCamera` object that gets
`SetupRenderCamera` called on it (reverse-Z + jitter + VP rebuild). This is
the mechanism the reflection passes use to render from different viewpoints.

However, this is per-pass, not a full scene re-render. To render the *entire
scene* from a different camera position, we would need to either:

1. **Run a third dispatch** (in addition to the two stereo eye dispatches)
   with the render camera positioned externally. This is the "third eye"
   approach — roughly 1.5× the render cost (3 draws instead of 2). The
   existing stereo infrastructure (save/restore frame counters, EffectInfo
   slots, etc.) would need to be extended for the third dispatch.

2. **Use the reflection proxy system.** The engine already has infrastructure
   for rendering from different viewpoints: the `RP_REFLECTIVE_WATER_PLANES`,
   `RP_SCREEN_SPACE_REFLECTIONS`, and `RP_REFLECTION_*` passes. These render
   depth/normal/gloss from a proxy camera into off-screen textures. However,
   these are not full scene colour renders — they are depth/normal captures for
   SSR and planar reflections. They would not produce a usable "external view"
   texture.

3. **Render to a texture via a custom render setup.** Create a separate render
   target + render setup, set the external camera, and run
   `DrawRenderPassRange` for the full pass range (`0x2F`–`0x96`). This is
   essentially a third dispatch but scoped to the render pass range, not the
   whole `HandleDrawThreadTask`. The cost is a full scene render.

#### Display surface

The external view texture would be displayed on a quad inside the cockpit —
at the windshield position or as a "digital display" overlay. This quad would
be drawn in the stereo render, either as a world-space quad (positioned at the
windshield) or as a screen-space overlay (at a fixed position in the HUD).

#### Cost analysis

A third full scene render at 90 Hz is expensive. At reduced resolution
(half-res, ~960×1080 per eye → ~480×540 for the external view), the cost is
roughly 0.25× of one eye — bringing the total to ~2.25× the single-eye cost.
This may be acceptable on high-end hardware but is a concern.

#### Verdict

Technically feasible via the `ExternalRenderCamera` + third dispatch, but
expensive. The display surface is straightforward (a world-space quad). The
main engineering work is extending the stereo dispatch infrastructure for a
third pass and managing the per-dispatch state (frame counters, EffectInfo,
shadow parity, etc.).

### Option B: Fall back to third-person exterior view

When the player enters an enclosed vehicle, switch to a third-person exterior
view. The player sees the vehicle and surroundings from behind/above.

#### Using the game's native TPS camera

The game's native vehicle camera (`CGenericVehicleModifier`) already places
the camera behind the vehicle. The mod's camera hook overrides this to place
the camera at the head bone. To fall back to TPS for enclosed vehicles, the
mod would simply *skip* the camera override for enclosed vehicles — letting
the engine's `CGenericVehicleModifier` position the camera externally.

**Problem:** The native TPS camera is screen-locked (not 6-DoF). It uses
the jitter filter, springs, and collision — all designed for a flat screen.
In VR, a screen-locked camera is uncomfortable (no head tracking, the view
doesn't respond to head movement). The player would feel "stuck" to the
screen.

**Improvement:** Apply the existing camera hook (head-bone position) but at
an *external* position — e.g., behind and above the vehicle, derived from the
vehicle's matrix. This gives the player head tracking (the camera position
responds to HMD movement) while showing the exterior. The position would be:

```
external_cam_pos = vehicle_matrix * vec3(0, 3.0, -6.0)  // 3m up, 6m back
```

This is a "floating camera" — positioned in 3D space relative to the vehicle,
but with head tracking applied on top. The mod's per-eye parallax
infrastructure would work as-is.

#### Transition smoothing

The transition between first-person (open vehicles) and third-person
(enclosed vehicles) should be smoothed. Options:
- **Fade to black, then back:** simplest, but jarring.
- **Dolly + blend:** animate the camera from the head position to the
  external position over ~0.3–0.5 s, with a fade. The mod's lazy-follow
  infrastructure (from the HUD panel) could be reused.
- **Cut with a brief vignette:** instant cut, but with a 0.2 s darkening
  vignette to mask the discontinuity.

The trigger should be debounced — don't switch if the player is entering/exiting
rapidly. A 0.5 s hold before switching prevents flicker.

#### Verdict

Simpler and cheaper than Option A. The main work is detecting enclosed
vehicles, skipping the camera override, and smoothing the transition. The
floating camera approach gives head tracking without the cost of a third
render.

### Option C: Hybrid — external camera with in-vehicle overlay

Combine both: render the external view (Option A) but display it as a
full-screen overlay (not a cockpit quad) when the player looks forward.
This is essentially a "camera monitor" — the player's entire forward view
is the external camera feed, with the cockpit visible when they look left,
right, or down.

This requires the third dispatch (Option A's cost) but gives a more immersive
feel than a small cockpit screen. The player is still "inside" the vehicle
(their head tracking works for looking around the cabin), but the forward view
is the external feed.

**Verdict:** Most immersive but most expensive. Depends on Option A's
infrastructure.

## Summary of findings

| Question | Answer |
|---|---|
| Is there a per-vehicle "enclosed" flag? | No. Detection requires a curated list or runtime geometry probing. |
| What vehicle types exist? | `ECAR, EHELICOPTER, EBOAT, EAIRPLANE, EMOTORCYCLE, ESUB, ETRAIN` — broad categories, no interior info. |
| Can a render pass use a different camera? | Yes — `CRenderPass::ExternalRenderCamera` feeds a separate `CCamera` to `SetRenderContextCamera`. |
| Can we render the full scene from an external camera? | Yes, via a third dispatch with the render camera repositioned. ~1.5× render cost. |
| Can we use the game's native TPS camera? | Yes — skip the mod's camera override for enclosed vehicles. But it's screen-locked (no head tracking). |
| Can we make a floating external camera? | Yes — position the camera behind/above the vehicle, with the mod's per-eye parallax on top. |

## Leading candidate (not yet implemented)

None of the options below have been built. The current front-runner for an
initial implementation is Option B (floating third-person camera) for enclosed
vehicles, with a curated list of enclosed vehicle name hashes: skip the camera
override, position the camera externally relative to the vehicle matrix, apply
head tracking and per-eye parallax, and smooth the transition. It is the
cheapest option and reuses the existing camera hook.

Option A (external camera render target) is a possible later enhancement for
players who want to stay in first-person, but it requires the third-dispatch
infrastructure and is a much larger engineering effort.

## Key release addresses

`PushRenderContext`, `CRenderPass::SetRenderContextCamera`, and
`CShadowManager::CommitRenderPassSettings` now live in pyxis-defs; consult the
bindings for their addresses. The remaining symbol is not yet defined there:

| Symbol | Release address |
|---|---|
| `CCharacter::IsInDrivingVehicleState` | `0x140_77E_AF0` |

`CVehicle` (with `m_VehicleType`, `m_NameHash`, and `m_Name`) is not modelled
in pyxis-defs yet; its layout is described in the prose above.
