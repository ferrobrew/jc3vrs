# Floating HUD in VR

The flat game draws its HUD — health, ammo, minimap, reticle, objective markers, distance labels — straight onto the final LDR image (rendering §10). In VR that pins the whole HUD to the screen plane at infinity, which is unreadable and uncomfortable. The plan: render the HUD into our own texture, then draw that texture as a quad floating a couple of metres in front of the camera, damped so it follows the head without feeling nailed to it. World-anchored markers are a separate problem, handled by reprojection rather than by the panel — see "World-to-screen" below.

## Approach: in-engine quad first, OpenXR layer later

There are two ways to present the HUD texture in 3D. An **OpenXR quad layer** (`XrCompositionLayerQuad`) hands the runtime the texture plus a pose and a size; the compositor draws it once at display rate, reprojected every frame and sampled directly — sharpest text, robust to game-framerate drops. An **in-scene quad** is one we draw into each eye ourselves, inside the stereo render.

We are doing the in-scene quad first. The point right now is to dial in distance, size, and follow-lag against the existing desktop stereo render — the side-by-side preview — long before a headset is in the loop. A quad layer can only be seen in a headset, so it can't be iterated on the desktop; the in-scene quad shows up in the preview immediately. Its downsides (the compositor double-samples it, so text is slightly softer; it's coupled to game framerate) don't matter while tuning, and we need an in-scene quad for world markers regardless. The quad layer stays on the table as a later swap for final sharpness once we're in-headset.

## Redirecting the HUD into our texture

`CUIManager` (the Scaleform GFx singleton — `GetIUIManager` `0x1400995A0` returns `0x142E5B620`) renders into the engine's back buffer by default. `CUIManager::InitPlatformRT` (`0x140F696E0`, verified) is where it binds its render target: it builds a Scaleform `RenderTargetData` (stored at `CUIManager+0x1390`), pulls the RTV and DSV from the engine surface (`GetRTVFromSurface` `0x141956240` / `GetDSVFromSurface` `0x141956250`), and binds them via `RenderTargetData::UpdateData` (`0x141DE0CF0`). It runs once at startup (`InitializeSystem` `0x14106F890`) and again on every device or resolution reset (`RestoreAfterReset` `0x140FA9C70`).

Hook `InitPlatformRT` and substitute our own `ID3D11RenderTargetView`, created over our HUD texture, for the value `GetRTVFromSurface` returns; let `UpdateData` bind it as before. The HUD then renders into our texture instead of the back buffer. Re-apply on resolution change (it's called from `RestoreAfterReset`), and size the texture to the dimensions passed in its `a2`. `CUIManager::RenderOffScreenTextures` (`0x1410076C0`) already renders Scaleform to offscreen textures for in-world screens, so retargeting the main HUD is well within the engine's existing capability.

## Comfort: lazy follow

A HUD rigidly locked to the head is the worst case — the eyes can't settle on it and head tremor is amplified. World-locked is the other extreme — it slides out of view the moment you turn to fight. The comfortable middle is a delayed, lazy follow: the panel eases toward a head-relative target, with position decoupled from orientation.

Put the panel at roughly 1.8 m (inside the 1.5–2.5 m comfort band — far enough to avoid vergence strain, near enough to read), around 45–55° wide, flat. Yaw follows head yaw through a deadzone of about ±10° and then eases in, so a quick flick to aim doesn't drag the HUD with it. Pitch follows looser (≈±6° deadzone, roughly horizon-anchored) so looking down at your gun doesn't yank the whole HUD down. Position is just de-jittered.

Use a critically-damped exponential so it converges fast with no overshoot, and so it's frame-rate independent: `alpha = 1 - 2^(-dt/halflife); current = lerp(current, target, alpha)` (Holden's damper / Unity SmoothDamp). Sensible starting halflives are yaw 0.15 s, pitch 0.3 s, position 0.1 s. Expose all of them as sliders — these are starting points to feel out in the preview, not settled values.

## World-to-screen: split the HUD

This is the crux. The game places objective markers, enemy pips, and distance labels by projecting a world point through the camera's view-projection to a screen coordinate. If we bake that into the lagging panel texture, a marker that should point 20° to the left ends up wherever the panel happens to have drifted. The fix is to split the HUD by what each element actually *means*:

- **Static HUD** — health, ammo, minimap, reticle frame — has no world anchor. Draw it into the panel texture at its native flat positions and let it lag with the panel. That lag is correct and comfortable, and no world-to-screen is involved.
- **World-anchored markers** mean a *direction*, not a panel position. They must not be baked into the lagging panel. Reproject each one every frame with the *current* per-eye view-projection (`VP = P_eye · V_eye(current head pose)`) and present it direction-locked to the live head at the marker's real depth — a small quad per marker, or a head-current overlay. Distance labels ride along on their marker. Never reuse the flat game-camera VP once we're stereo; it no longer corresponds to either eye.

The engine makes this directly doable because its world-to-screen takes the VP as a parameter. `CUIManager::Convert3DCoords` (`0x140F69A70`, verified) is `bool(this, CVector3f *world, float *outX, float *outY, CMatrix4f *vp)` — it multiplies the world point by `vp`, divides by `|w|`, aspect-corrects, and maps NDC to pixels (`viewW`/`viewH` at `this+0x1484`/`+0x1488`), returning false when the point is behind the camera. The marker-placement wrapper `CUIManager::Get2DInfo` (`0x140F69CB0`) forwards a caller-supplied `vp` to it and handles the on-screen test and edge-clamping (`ClampToScreen` `0x140F470A0`). Because the VP is an argument, feeding our chosen per-eye VP to those callsites relocates every marker onto the plane we want. Find the callsites by xref'ing `Get2DInfo`; the default VP is the render camera's `m_ViewProjectionF` (camera at `*(0x142ED0E20+0x5C0)`, field `+0x194`). See rendering §10 for the surrounding UI-emission flow.

## Order of work

1. Redirect the HUD into a texture via the `InitPlatformRT` hook; confirm it still renders correctly off-screen.
2. Draw it as a fixed in-scene quad per eye in the stereo render; get it visible in the side-by-side preview.
3. Add the lazy-follow + the sliders; tune distance, size, and the halflives on the desktop.
4. Split markers out: feed the per-eye VP to the `Get2DInfo` / `Convert3DCoords` callsites and present them direction-locked.
5. Later, once in-headset, swap the static panel to an `XrCompositionLayerQuad` for final sharpness.

## To verify before implementing

`Get2DInfo` (`0x140F69CB0`) and its gameplay callsites' VP sources are from the research agent, not yet personally line-traced; `Convert3DCoords` and `InitPlatformRT` are confirmed live in the release i64. The `CUIManager` field-offset semantics (`+0x1390` RenderBuffer, `+0x1484`/`+0x1488` view size) are read from the decompiled math and are solid as offsets; the labels are inference.
