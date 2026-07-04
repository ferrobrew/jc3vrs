# Scaleform HUD overlays and depth in VR

Research covering issue #8 (full-screen HUD overlays that break on a floating
panel) and issue #14 (dynamic HUD depth/scale based on scene geometry). The two
share the same underlying Scaleform architecture and the same per-element
separation question, so they're documented together.

**Status: implemented on the `hud-depth-split` branch, pending in-game
verification.** The multi-pass split renders the HUD in three visibility
passes (static / markers / center) into separate textures composited at
per-layer depths, with a per-marker depth warp on the marker layer, an
aim-driven depth for the center layer, and overlay suppression for #8 — see
`docs/hud.md` ("Depth layers") for the shipped design. This document keeps the
underlying reverse-engineering and records what still needs an in-game pass:

1. The display-tree dump (HUD tab, Scaleform section) must confirm the runtime
   clip paths — the authored `MCI_*` names and the attachment prefix under the
   root movie (`hud.split_path_prefix`). `SetVariable` failures are logged
   once per path.
2. The MovieRoot vtable guard (`Movie::VFTABLE`, `0x1426216B0`) must match the
   live object; a mismatch logs and disables every Scaleform-side operation.
3. The capture-per-pass mechanism, the deferred-render-lock reentrancy, and
   the per-layer visuals themselves.

One correction to the original analysis below: `HAL::Draw` renders a
*captured* display-tree snapshot, so toggling `_visible` between draws does
nothing without a fresh `Movie::Capture` per pass (with capture-thread
ownership borrowed, as `RenderOffScreenTextures` does). The implemented
sequence per pass is: set visibility, capture, rebind the render buffer's
views, call the original `Render` — all under `m_DeferredRenderLock`, which
`PreRender` also holds across `Advance`+`Capture`, making the split race-free
against the update thread.

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

The floating HUD panel sits at a distance from the camera set by a manual
slider (3 m default, 0.3–10 m range). When geometry is closer than the panel
(vehicle interiors, indoor corridors, near walls), the HUD overlaps that
geometry and creates conflicting depth cues. The constant-apparent-size scaling
is implemented (the panel resizes with distance to maintain angular size), so
the distance can be changed freely without the HUD growing or shrinking. What
remains is deciding the `distance` from scene depth automatically, and ideally
rendering individual elements at different depths.

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

## Candidate approaches to suppressing overlays (#8)

None of these is implemented; they are the candidate paths for closing out #8.

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

## Candidate approaches to dynamic panel distance (#14)

Distance is currently a manual slider. The options below for deriving it
automatically are future work.

### A: Near/far presets with smoothed transitions

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

### World-to-screen: Get2DInfo and Convert3DCoords (shipped)

World-anchored markers (objective markers, enemy pips, distance labels) are
projected via `CUIManager::Get2DInfo` / `Convert3DCoords`, which take the VP as
a **parameter**. This is now implemented for the floating panel: `Get2DInfo` is
hooked (`payload/src/hooks/ui.rs`) and its VP and camera matrix are swapped for
the panel's orientation (`hud::compute_panel_vp`), with `m_CachedViewportRatio`
retargeted to the panel aspect so `Convert3DCoords`' aspect correction lands on
the panel surface rather than the screen plane. See `docs/hud.md` for the full
mechanism. The default (unhooked) callsites carry the render camera's VP;
callsites are found in `CHUDUI.cpp`, `CPOI.cpp`, `CROMTrigger.cpp`,
`CMissionTrigger.cpp`, `CUIMenu.cpp`, `NLandVehicle_Hidden.cpp`.

This reprojects markers onto the panel as a whole — it does not separate
individual markers to their real world depths (see the multi-pass approach
below, which remains a stretch goal). It only affects world-anchored markers;
static HUD elements (health, ammo, minimap) have no world position and are
always baked into the panel texture.

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
| **Multi-pass: toggle visibility + re-render** | **Medium-high** | **No** | **Full — separate textures per pass** |
| Suppress + mod-drawn quads | Medium | No | Full — mod draws at any depth |
| Near/far panel distance presets | Medium | No | Panel-level only |
| Intercept `Get2DInfo` for world markers | Medium | No | Partial — markers only |
| Second `CreateInstance` of `root.gfx` | Medium-high | No | No — renders whole HUD |
| Load `hud.gfx`/`overlay.gfx` standalone | Medium-high | No (shipped files) | Partial — if standalone |
| Author custom `.gfx` files | High | Yes | Full — per-group movies |

## GFX file inspection findings

We extracted the shipped `.gfx` files from the game archives using a Python
script (the Gibbed tools require .NET/Wine, which isn't available; the `.tab`/
`.arc` format is simple — Jenkins-hashed entries pointing at offsets in the
`.arc`). The files use the **CFX** magic (zlib-compressed Scaleform GFX,
essentially SWF with Scaleform extensions).

A Rust tool (`tools/scaleform-gfx-examine`) was built to parse and dump the
GFX tag structure, SymbolClass mappings, DoABC string pools, and DefineSprite
depth tables. Full ABC body parsing (instances, classes, method bodies) was
not completed — Scaleform's GFX uses a non-standard GenericName multiname
encoding that desynchronizes the AVM2 parser. The string pool and multiname
names are extracted successfully, covering the most useful data.

### `root.gfx` — the UI movie registry

`root.gfx` contains `CSharedLibUI`, the base class that manages the UI movie
registry. Each UI movie is registered as a class-to-filename pair:

| AS3 class | Movie file | Purpose |
|---|---|---|
| `COverlayUI` | `overlay` | Overlay elements |
| `CHUDUI` | `hud` | HUD |
| `CMainUI` | `main` | Main menu |
| `CPauseUI` | `pause` | Pause menu |
| `CIntroUI` | `intro` | Intro |
| `CTitleUI` | `title` | Title |
| `CTutorialsUI` | `tutorials` | Tutorials |
| `CCreditsUI` | `credits` | Credits |
| `CLobbyUI` | `lobby` | Lobby |
| `CCommLinkUI` | `comm_link` | Comm link |
| `CCommCollectiblesUI` | `comm_collectibles` | Collectibles |
| `CCommCommunityUI` | `comm_community` | Community |
| `CCommMapUI` | `comm_map` | Map |
| `CCommSkillUI` | `comm_skill` | Skill |
| `CCommStatsUI` | `comm_stats` | Stats |
| `CCommBragsFeatsUI` | `comm_brags_feats` | Brags/feats |
| `CCommStoreUI` | `comm_store` | Store |
| `CRewardUI` | `reward` | Rewards |
| `CROMUI` | — | ROM (challenges) |
| `CSharedLibUI` | `shared_lib` | Shared library |

It also exposes the full set of `ExternalInterface.call` method names the
engine invokes on the root movie: `activate`, `deactivate`, `:tween_pos`,
`:tween_alpha_visibility`, `:delay_call`, `:get_localized_string`,
`:set_setting_value`, `:get_setting_value`, `:get_sku`, `:hash`, etc.

### `hud.gfx` — SymbolClass and element inventory

The HUD movie contains 1649 tags: 546 DefineSprites, 357 DefineShapes, 159
DefineEditText fields, one DoABC block (395 KB), one SymbolClass, and 58
DefineFont4 entries. The SymbolClass maps tag IDs to AS3 class names:

**Full-screen overlay elements (to suppress, #8):**

| Tag ID | AS3 class | C++ Invoke method | What it does |
|---|---|---|---|
| 985 | `hud_code.health.omni_damage` | `OnOmniDamage` | Screen-wide damage flash |
| 984 | `hud_code.health.hud_health_damage_manager` | — | Manages health paint layers |
| 979 | `hud_code.health.hud_health_paint_container` | — | Container for paint layers |
| 978 | `hud_code.health.hud_health_paint` | `UpdateHealth` | Near-death screen-edge paint (4 corners × 5 layers) |
| 986 | `hud_fla.drown_4` | `ShowDrowning`/`HideDrowning` | Full-screen drowning overlay |
| 265 | `hud_code.CharacterHitIndicator` | `UpdateCharacterDmgIndicators` | Directional damage arrows |
| 53 | `VehicleHitIndicator` | `UpdateMechDmgIndicators` | Vehicle damage arrows |
| 1041 | `hud_fla.inflict_damage_57` | `OnPlayerDogeDamage` | Damage type indicator |
| 417 | `bomb_warning` | `ShowWarning`/`HideWarning` | Warning message |
| 1496 | `hud_fla.warning_426` | `ShowWarning`/`HideWarning` | Warning visual container |
| — | (dynamic) | `ShowSniperOverlay`/`HideSniperOverlay` | Sniper scope vignette |

The sniper overlay has no static SymbolClass entry in `hud.gfx` — it's likely
created dynamically by ActionScript or lives in `overlay.gfx` (which is in the
patch archive, not yet extracted due to a hash mismatch).

**Elements to keep on the panel (#14):**

| Tag ID | AS3 class | What it does |
|---|---|---|
| 0 | `hud` | Root HUD container |
| 494 | `hud_code.life_bar_group` | Health bar group |
| 785 | `hud_code.nitro_meter` | Nitro/boost meter |
| 739 | `hud_code.bavarium_meter` | Bavarium meter |
| 610 | `hud_code.jump_meter` | Jump meter |
| 252 | `DualTether` | Tether UI |
| 235 | `hud_code.poi` | Points of interest |
| 601 | `hud_code.live_leaderboard.live_leaderboard` | Leaderboard |
| 552 | `hud_code.hud_challenge_timer` | Challenge timer |
| 834 | `hud_code.c_chaos_award` | Chaos award |
| 859 | `hud_code.notifications.hud_notification_manager` | Notifications |
| 415 | `hud_code.timer_big` | Big timer |

### Per-element depth separation: a new concern

The multi-pass approach (toggle `_visible` per clip between `HAL::Draw` passes)
assumes we can address each element by a clip path string
(e.g., `root.hud_mc.health_paint_container`). The GFX inspection reveals a
structural obstacle:

- There are only **10 PlaceObject2 tags** on the root timeline but **546
  DefineSprites**. Most elements are not statically placed — they're created
  and parented dynamically by ActionScript code (`addChild`, `addChildAt`).
- The SymbolClass gives us class names, and DefineSprite tables give us static
  depth placement *within* each sprite, but the *runtime* parent-child
  relationships are established by AS3 code we can't fully decompile (the
  GenericName parsing issue blocks full ABC body recovery).
- Even if we trace the static sprite hierarchy, the runtime clip paths may
  differ from the static tag structure. Elements like `omni_damage` might be
  added to a container that's itself dynamically created and named.

**Implication:** Clip-path discovery from static `.gfx` analysis alone is
insufficient for the multi-pass approach. Runtime probing (`GetVariable` on
candidate paths) or hooking `addChild`/`addChildAt` calls would be needed to
map the runtime display tree. This raises the difficulty of the multi-pass
approach from "medium" to "medium-high".

However, **coarse suppression** (filtering Invoke method names to prevent
overlay elements from being driven) does not need clip paths at all — it works
purely at the C++ → AS3 call boundary.

## Candidate approaches, summarized

None of the below is implemented. They are the candidate directions for
closing out #8 and #14; the world-marker reprojection is the one piece already
shipped (see the world-to-screen subsection above).

1. **For #8 (overlay suppression):** Hook `CUIBase::Invoke` and filter
   full-screen overlay method names (`OnOmniDamage`, `ShowDrowning`,
   `ShowSniperOverlay`). Optionally set `s_disable_health_ui = true` for
   damage indicators. Keep the hit marker (`OnPlayerDidDamage`).

2. **For #14 (dynamic distance):** Distance is currently a manual slider. To
   drive it from scene geometry, near/far presets with an
   `IsInDrivingVehicleState` trigger plus optional depth-buffer probing,
   smoothed with exponential damping, would resize the panel automatically.

3. **For per-element depth (stretch goal, both issues):** The multi-pass
   approach — hook `CUIManager::Render`, toggle `_visible` per clip between
   `HAL::Draw` passes to separate textures. Requires **runtime** clip-path
   discovery (static `.gfx` analysis is insufficient — most elements are
   dynamically parented by ActionScript). Probe at runtime via `GetVariable`
   on candidate paths, or hook `addChild`/`addChildAt` to map the display
   tree. Difficulty is assessed as medium-high.

4. **For alternative feedback (#8):** Hook `CPlayerHealthEffects::OnDamage`
   for directional damage data. Render directional indicators as world-space
   quads at real depth. Use OpenXR haptics for damage feedback. Draw a subtle
   red vignette on the panel edges for low health.

## Key addresses and symbols

Release ground truth recovered for the implementation (all bound in
`pyxis-defs`):

| Symbol | Address / offset | Notes |
|---|---|---|
| `CUIManager::Render` | `0x141007B70` | `IUIManager` vtable slot 4; the split detour target. |
| `UIManager::m_DeferredRenderLock` | `+0x12B8` | Held by both `PreRender` (`Advance`+`Capture`) and `Render`; re-entrant. |
| `UIManager::m_MainThreadId` | `+0x44` | The capture thread to hand ownership back to. |
| Render gates | `+0x212`, `+0x213`, `+0x147F` | `m_RenderReady` / `m_RenderActive` / `m_RenderingEnabled`. |
| `UIManager::m_Movie` | `+0x12E8` | The `GFx::MovieImpl` (the trio was previously bound 0x10 low). |
| `MovieImpl::pASMovieRoot` | `+0x18` | The AS3 `MovieRoot` (SetVariable/Invoke/display tree). |
| `MovieImpl` vtable | slots 24-27, 48 | `Advance`, `Capture`, `GetDisplayHandle`, `SetCaptureThread`, `GetHeap`. |
| `MovieRoot` vtable | `0x1426216B0` | Standard `ASMovieRootBase` layout: `GetDisplayObjectsTree` 35, `SetVariable` 49, `GetVariable` 50, `Invoke` 57. |
| `CUIManager::Convert3DCoordsDefault` | `0x140F899A0` | The grapple reticle's default-VP world-to-screen; hooked for panel reprojection + aim depth. |

Original static-analysis reference:

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
