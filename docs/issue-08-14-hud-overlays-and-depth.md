# Scaleform HUD overlays and depth in VR

Research covering issue #8 (full-screen HUD overlays that break on a floating
panel) and issue #14 (dynamic HUD depth/scale based on scene geometry). The two
share the same underlying Scaleform architecture and the same per-element
separation question, so they're documented together.

## The problems

### #8: Full-screen overlays

Full-screen overlays like the player-damage red flash, directional damage
indicators, drowning effect, and impact warnings are Scaleform GFx elements
baked into the HUD texture. When the HUD is projected as a floating panel in
3D space, these overlays cover the entire panel — which looks wrong, since
they were designed to cover the entire flat screen. This is specifically about
**Scaleform-layer** overlays, not the post-FX passes (which are already handled
by `skip_player_damage` / `skip_fade` config toggles).

### #14: Dynamic panel distance

The floating HUD panel sits at a fixed distance from the camera (3 m
default). When geometry is closer than the panel (vehicle interiors, indoor
corridors, near walls), the HUD overlaps that geometry and creates conflicting
depth cues. The constant-apparent-size scaling is already implemented (the
panel resizes with distance to maintain angular size). What remains is
deciding the `distance` from scene depth, and ideally rendering individual
elements at different depths.

## The Scaleform HUD architecture

### One movie, one texture, one Invoke

The HUD is driven by `CHUDUI`, a singleton that inherits from `CUIMenu` →
`CUIBase`. The rendering pipeline:

1. **`CUIManager::StartRender`** (vtable +0x08) — kicks off the async UI render
   fragment on a separate thread.
2. **`CUIManager::SyncRender`** (vtable +0x10) — barrier, waits for the UI
   render thread.
3. **`CUIManager::Submit`** (vtable +0x18) — locks the master context, flushes
   UI draws via `m_RenderHAL->Submit()`.
4. **`CUIManager::Render`** — the actual render: walks the Scaleform render
   tree (`TreeRoot`), calls `HAL::Draw` for each display handle, drawing all
   GFx elements into the bound render target.

`CUIBase` has an `Invoke` virtual function (vtable slot):

```c
// CUIBase.h
bool (__fastcall *Invoke)(CUIBase *this, const char *method_name,
    const Scaleform::GFx::Value *args, unsigned int num_args,
    Scaleform::GFx::Value *result, float timeout);
```

Every HUD update is a call to `this->Invoke(this, "MethodName", args, ...)`,
which internally calls `Scaleform::GFx::Movie::Invoke` on the HUD movie
(`CUIManager::m_Movie`). The method name string is an ActionScript method on
the GFx movie's root timeline. All elements are rendered into the single HUD
texture by the movie's own rendering pipeline — there is no per-element render
target separation.

### Shipped .gfx files

The game ships separate `.gfx` files (found via the Gibbed file lists):

- `ui/root.gfx` — the main loader movie (loaded by `CUIManager` at init).
- `ui/hud.gfx` — HUD elements (imported into `root.gfx`).
- `ui/overlay.gfx` — overlay elements (imported into `root.gfx`).
- `ui/shared_lib.gfx` — shared library (fonts, common symbols).
- `ui/dyn_root.gfx` — used by `CRenderToTextureUI` for in-world screens.

`CUIManager` loads `ui/root.gfx` as a single `MovieDef` at init:

```c
Movie = Scaleform::GFx::Loader::CreateMovie(loader, "ui/root.gfx", 0x42, 0);
this->m_MovieDef = Movie;
// ... later:
m_MovieDef->CreateInstance(m_MovieDef, ...);  // creates the single m_Movie
```

All `CUIBase` subclasses (CHUDUI, CRomUI, CCommLinkUI, etc.) get their
`m_Movie` set to this same instance in `CUIBase::OnInit`:

```c
void CUIBase::OnInit(CUIBase *this) {
    this->m_Movie = CUIManager::Instance->m_Movie;
}
```

The separate `.gfx` files (`hud.gfx`, `overlay.gfx`) are imported into
`root.gfx` at the Scaleform authoring level — they become `MovieClip` symbols
in the root movie's library. `CUIBase::Activate` calls `root.Activate` /
`root.ActivateDyn` with a movie clip name string, which tells the root movie's
ActionScript to show/attach the named clip from its library. All elements live
within the same movie instance and render into the same texture.

### The CRenderToTextureUI pattern

The engine does have a mechanism for truly separate movies:
`CRenderToTextureUI`. Each `CRenderToTextureUI` loads its own `MovieDef` from
`ui/dyn_root.gfx`:

```c
Movie = Scaleform::GFx::Loader::CreateMovie(loader, "ui/dyn_root.gfx", 0x42, 0);
m_MovieDef->CreateInstance(m_MovieDef, ...);  // creates its own m_Movie
```

It creates its own `Movie` instance, has its own `CUIBase` code interface
(`m_CodeInterface`), renders into its own render target, and is registered
with `CUIManager::AddRenderToTextureUI` for rendering in
`RenderOffScreenTextures`. This is the engine's own pattern for separate UI
movies at different depths — used for in-world Scaleform screens.

### The HUD update loop

`CHUDUI::Update` (called from `CUIManager`'s update cycle) drives all HUD
elements per frame. The main loop (dump: `CHUDUI.cpp`, line ~28755):

```c
CHUDUI::UpdateDirectionalDamageIndicators(this, v6);
CHUDUI::UpdateOxygen(this, v6);
CHUDUI::UpdateWeaponReticle(this, ...);
CHUDUI::UpdateGrappleReticle(this, ...);
CHUDUI::UpdateVehicle(this, ...);
CHUDUI::UpdateWingsuit(this, ...);
CHUDUI::UpdateHealth(this, ...);
CHUDUI::UpdateDetector(this, ...);
CHUDUI::UpdatePOIs(this, ...);
CHUDUI::UpdateMissileWarning(this);
```

This is gated by `m_State` (an `EHUDState` enum): when the state is
`HUD_HIDE_ALL` (0x2) or `HUD_HIDE_ALL_NON_OVERLAY` (0x5), most updates are
skipped. `HUD_HIDE_ALL_NON_OVERLAY` is the state used during pause menus and
comm-link — it hides the main HUD but keeps overlay elements (damage
indicators, warnings) visible.

## The Scaleform overlay elements (#8)

The following AS3 method names are the full-screen / overlay elements that
misbehave on a floating panel. Each is invoked via `CHUDUI::Invoke`:

| AS3 method | CHUDUI function | What it does | Full-screen? |
|---|---|---|---|
| `UpdateCharacterDmgIndicators` | `UpdateDirectionalDamageIndicators` | Directional damage arrows | Partial |
| `UpdateMechDmgIndicators` | `UpdateDirectionalDamageIndicators` | Same, for mechs | Partial |
| `OnOmniDamage` | `OnOmniDamage` | Screen-wide damage flash (`m_GrabScreenTimer = 2.0`) | **Yes** |
| `OnPlayerDogeDamage` | `OnPlayerDogeDamage` | Damage type indicator (bullet, explosion, fire) | Partial |
| `OnPlayerDidDamage` | `OnPlayerDidDamage` | Hit marker | No (small) |
| `UpdateHealth` | `UpdateHealth` | Health bar update (drives near-death) | No |
| `ShowDrowning` / `HideDrowning` | `UpdateOxygen` | Full-screen water tint when underwater | **Yes** |
| `ShowWarning` / `HideWarning` | `ShowWarning` / `HideWarning` | Warning message overlay | Partial |
| `UpdateMissileImpactWarning` | `UpdateMissileWarning` | Missile direction warning | Partial |
| `ShowSniperOverlay` / `HideSniperOverlay` | `ShowSniperOverlay` / `HideSniperOverlay` | Sniper scope (full-screen vignette + crosshair) | **Yes** |

### The damage indicator data flow

When the player takes damage, `CCharacter` (dump line ~39779) calls
`CHUDUI::OnPlayerDogeDamage` / `CHUDUI::OnOmniDamage` /
`CHUDUI::OnPlayerDidDamage` directly. Simultaneously,
`CPlayerHealthEffects::OnDamage` (line 404) creates or updates
`SDmgIndicator` entries in `m_CharacterDmgIndicators` /
`m_VehicleDmgIndicators` vectors:

```c
struct SDmgIndicator {
    boost::weak_ptr<CGameObject> m_SourceGo;  // who damaged us
    float m_AccumulatedDmg;                     // total damage from this source
    float m_Time;                               // time since last hit
    bool m_ShieldHit;                           // was it a shield hit?
    unsigned __int16 m_Id;                      // unique ID
};
```

`CHUDUI::UpdateDirectionalDamageIndicators` (line 10043) reads these vectors,
calls `PopulateDirectionalDamageIndicators` to build a GFx array, then calls
`Invoke("UpdateCharacterDmgIndicators", ...)` / `Invoke("UpdateMechDmgIndicators",
...)` to push the array to the Scaleform movie.

### The `s_disable_health_ui` flag

There is a console variable `s_disable_health_ui` (a global `bool` in
`_data.cpp`). When set, it causes `UpdateHealth` and `OnOmniDamage` to skip
their Scaleform Invoke calls, and `UpdateDirectionalDamageIndicators` to skip
the indicator update. This is the engine's own mechanism for suppressing the
health/damage UI — used during cutscenes and debug.

## Approaches to suppressing overlays (#8)

### A: Hook `CUIBase::Invoke` and filter method names

`CUIBase::Invoke` is a virtual function on the vtable. The mod can hook it and
selectively suppress specific AS3 method calls by name — return `true`
(pretend success) for suppressed methods. Must check `this` to identify the
HUD caller (via `CHUDUI::Instance` singleton).

**Advantages:** Clean, centralized, granular per-element, non-destructive.
**Disadvantages:** Shared across all UI menus (needs caller check); per-call
string comparison overhead (minor — ~20-30 calls per frame).

### B: Set `s_disable_health_ui` and supplement

Suppresses `UpdateHealth`, `OnOmniDamage`, and
`UpdateDirectionalDamageIndicators`. Doesn't cover drowning, sniper overlay,
or warnings. Suppresses all health UI including the health bar. Supplement
with Approach A for the remaining elements.

### C: Intercept the data, not the rendering

Hook `CPlayerHealthEffects::OnDamage` to capture damage source position, type,
and amount. Use this to render alternative VR feedback (world-space directional
quads at real depth, OpenXR haptics, panel-edge vignette). Suppress the
original Scaleform overlays via Approach A. Most work, best VR experience.

## Approaches to dynamic panel distance (#14)

### A: Near/far presets with smoothed transitions (recommended)

Two discrete distance settings with exponential damping (~0.5 s halflife):
- **Far (~3 m):** on-foot/flying.
- **Near (~1 m):** vehicles, indoors, close geometry.

**Trigger options:**
- `IsInDrivingVehicleState` (cheap, already available) for vehicle detection.
- Depth buffer probing: a small compute shader reading
  `GraphicsEngine::m_MainDepthTexture` at the HUD quad footprint, computing
  the median/trimmed-mean depth. The depth texture is `D32FS8` (reverse-Z,
  near = 1.0, far = 0.0). Already accessible via the FSR path.
- Interior detection via game-state hooks.

Hysteresis between near/far thresholds prevents oscillation. The panel resizes
automatically (constant apparent size is already implemented).

### B: Per-element depth (stretch goal)

In an ideal world, each UI element renders at its own depth. See the
multi-pass approach below.

## Can elements be separated for different depths?

### World-to-screen: Get2DInfo and Convert3DCoords

World-anchored markers (objective markers, enemy pips, distance labels) are
projected onto the panel via `CUIManager::Get2DInfo` / `Convert3DCoords`,
which take the VP as a **parameter**. Feeding a per-eye VP relocates each
marker. Callsites found in `CHUDUI.cpp`, `CPOI.cpp`, `CROMTrigger.cpp`,
`CMissionTrigger.cpp`, `CUIMenu.cpp`, `NLandVehicle_Hidden.cpp`.

This only affects world-anchored markers — static HUD elements (health, ammo,
minimap) have no world position and are always baked into the panel texture.

### Multi-pass: toggle element visibility, re-render to separate textures (most promising)

The GFx API supports per-clip visibility toggling and `HAL::Draw` can be called
multiple times to different render targets:

- **`Scaleform::GFx::Movie::SetVariable("root.clip._visible", false)`** —
  toggles a `MovieClip`'s `_visible` property. Takes effect immediately on
  the display list; no `Advance` needed (it's a `DisplayObject` property,
  not an ActionScript timeline action).
- **`DisplayInfo::SetVisible(bool)`** — structured alternative on
  `GFx::Value::ObjectInterface`.
- **`m_RenderHAL->SetRenderTarget(target)`** — redirects the next `HAL::Draw`
  to a different render target. `CUIManager` already uses this to bind
  `m_pDisplayRT`.
- **`HAL::Draw(renderEntry)`** — renders the current display tree. Can be
  called multiple times — `CUIManager::Render` itself calls it per
  `DisplayHandle`, and `RenderOffScreenTextures` calls it for each
  `CRenderToTextureUI`.

**The sequence:**

1. Game's `CHUDUI::Update` runs normally — pushes all data, calls `Advance`.
2. Hook `CUIManager::Render` (or `Submit`). Before the normal render:
   - Hide overlay clips via `SetVariable("root.<clip>._visible", false)`.
   - `SetRenderTarget(texture_A)` → `HAL::Draw` — static HUD into texture A
     (at the panel distance).
3. Then:
   - Show overlays, hide static elements.
   - `SetRenderTarget(texture_B)` → `HAL::Draw` — overlays into texture B
     (at a different depth or as world-space quads).
4. Restore all visibility to normal.

**Advantages:**
- No new `.gfx` files needed — uses the existing movie.
- No double-driving data — the game's `CHUDUI::Update` runs once; the mod
  just toggles visibility between render passes.
- Full per-element depth control — each texture can be displayed at a
  different depth.

**Challenges:**
- **Clip path discovery:** The mod needs to know the AS3 clip path strings
  (e.g., `"root.hud_mc.damage_indicators"`). These are authored in the `.gfx`
  file, not documented in the C++ code — the C++ side only knows method names
  (`"UpdateCharacterDmgIndicators"`, `"ShowDrowning"`), not the internal clip
  hierarchy. Discovery options:
  - Extract and decompile the `.gfx` files (via JPEXS FFDec or the Gibbed
    tools).
  - Runtime probing via `GetVariable` on known/likely paths.
  - Hook `CUIBase::Invoke` and trace which clips are accessed.
- **Thread safety:** The extra passes must happen on the UI render thread,
  inside the `CUIManager::Render` / `Submit` window.
- **Performance:** Each `HAL::Draw` renders the full display tree. Two passes
  ≈ double the UI render cost. The UI render is cheap relative to the scene
  render (a few hundred 2D draw calls), so likely acceptable.

**Verdict:** Medium difficulty, no new content needed. The most practical path
to per-element-depth rendering without Scaleform authoring tools.

### Other approaches (less promising)

- **Second `CreateInstance` of `root.gfx`** — produces a blank-slate movie
  that needs full re-initialization and data mirroring. Renders the whole
  HUD, not a subset. `MovieDef::CreateInstance` can be called multiple times
  on the same `MovieDef` (the `ResourceWeakLib::BindResourceKey` cache returns
  the same `MovieDef`), but the second `Movie` starts from frame 0 with an
  empty display list.
- **Load `hud.gfx`/`overlay.gfx` standalone** — the `LoadDisableImports`
  flag (`0x100000`) skips import resolution. But these files may be symbol
  libraries (not standalone movies) — loading them standalone may produce a
  movie with no visible content. Uncertain without inspecting the `.gfx` file
  contents.
- **Author custom `.gfx` files** — requires Scaleform authoring tools (Flash
  CS + Scaleform extension) not publicly available for JC3's version.

### Summary

| Approach | Difficulty | New `.gfx`? | Per-element depth? |
|---|---|---|---|
| **Multi-pass: toggle visibility + re-render** | **Medium** | **No** | **Full — separate textures per pass** |
| Suppress + mod-drawn quads | Medium | No | Full — mod draws at any depth |
| Near/far panel distance presets | Medium | No | Panel-level only |
| Intercept `Get2DInfo` for world markers | Medium | No | Partial — markers only |
| Second `CreateInstance` of `root.gfx` | Medium-high | No | No — renders whole HUD |
| Load `hud.gfx`/`overlay.gfx` standalone | Medium-high | No (shipped files) | Partial — if standalone |
| Author custom `.gfx` files | High | Yes | Full — per-group movies |

## Recommended approach

1. **For #8 (overlay suppression):** Hook `CUIBase::Invoke` and filter
   full-screen overlay method names (`OnOmniDamage`, `ShowDrowning`,
   `ShowSniperOverlay`). Optionally set `s_disable_health_ui = true` for
   damage indicators. Keep the hit marker (`OnPlayerDidDamage`).

2. **For #14 (dynamic distance):** Implement near/far presets with
   `IsInDrivingVehicleState` trigger + optional depth-buffer probing. Smooth
   with exponential damping. The panel resizes automatically.

3. **For per-element depth (stretch goal, both issues):** The multi-pass
   approach — hook `CUIManager::Render`, toggle `_visible` per clip between
   `HAL::Draw` passes to separate textures. Requires clip-path discovery
   (extract the `.gfx` files with JPEXS FFDec or the Gibbed tools — a
   one-time RE effort).

4. **For alternative feedback (#8):** Hook `CPlayerHealthEffects::OnDamage`
   for directional damage data. Render directional indicators as world-space
   quads at real depth. Use OpenXR haptics for damage feedback. Draw a subtle
   red vignette on the panel edges for low health.

## Key addresses and symbols

| Symbol | Notes |
|---|---|
| `CUIBase::Invoke` | Virtual function (vtable slot). Hook for overlay suppression. |
| `CUIBase::OnInit` | Sets `m_Movie = CUIManager::Instance->m_Movie` — all UI shares one movie. |
| `CUIBase::SetMovie` | Can reassign a `CUIBase`'s movie to a different instance. |
| `CRenderToTextureUI` | The engine's pattern for separate movies — loads `ui/dyn_root.gfx`. |
| `Scaleform::GFx::Loader::CreateMovie` | Creates a `MovieDef` from a `.gfx` file path. |
| `Scaleform::GFx::Loader::LoadDisableImports` | Flag `0x100000` — skip import resolution. |
| `MovieDef::CreateInstance` | Creates a `Movie` instance from a `MovieDef`. |
| `Scaleform::GFx::Movie::SetVariable` | Sets an AS3 variable — use for `_visible` toggling. |
| `Scaleform::GFx::Movie::GetVariable` | Reads an AS3 variable — use for clip-path probing. |
| `Scaleform::GFx::Movie::Invoke` | Calls an AS3 method on the movie's root timeline. |
| `m_RenderHAL->SetRenderTarget` | Redirects the next `HAL::Draw` to a different target. |
| `CUIManager::Get2DInfo` / `Convert3DCoords` | World-to-screen with VP as parameter — per-element marker reprojection. |
| `CUIManager::ClampToScreen` | Edge-clamping for markers. |
| `CHUDUI::Instance` | Singleton — identify the HUD caller in the Invoke hook. |
| `CHUDUI::Update` | Main HUD update loop — drives all elements per frame. |
| `CHUDUI::UpdateDirectionalDamageIndicators` | Builds and pushes damage indicators to Scaleform. |
| `CHUDUI::OnOmniDamage` | Triggers the full-screen damage flash. |
| `CHUDUI::ShowSniperOverlay` / `HideSniperOverlay` | Sniper scope overlay. |
| `CHUDUI::ShowDrowning` / `HideDrowning` | Drowning overlay. |
| `s_disable_health_ui` | Global bool (`_data.cpp`). Suppresses health/damage UI. |
| `CPlayerHealthEffects::OnDamage` | Where damage indicators are created — hook for alternative feedback. |
| `CPlayerHealthEffects::Update` | Where near-death state and effect level are computed. |
| `CCharacter::IsInDrivingVehicleState` | `0x140_77E_AF0` — vehicle state detection for distance presets. |
| `GraphicsEngine::m_MainDepthTexture` | `D32FS8` reverse-Z depth — for depth-buffer probing. |
| `CUIManager` singleton | `Base::CSingle<CUIManager>::Instance`. |
