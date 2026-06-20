# VR runtime: OpenXR, per-eye projection, per-eye resolution

What exists today is a desktop stereo prototype, not a VR build. The mod renders the scene twice per frame (rendering §11) and presents the two eyes side-by-side, but there is no OpenXR session, no HMD pose, and no off-axis projection — the camera hook writes a lateral position offset and lets the engine use its own symmetric projection with a hardcoded ~90° FOV. This doc is the runtime half that turns that prototype into something a headset can display. It is the spine the feature work (head/body, HUD, controllers) hangs off.

## The runtime loop

Bring up an OpenXR session against the D3D11 device and immediate context the game already owns (rendering §8 — take `m_Context->m_Mutex` at `+0x8028` around shared-context work), with a swapchain per eye sized to the runtime's recommended resolution (blocker 2). Each frame:

- `xrWaitFrame` / `xrBeginFrame` to pace against the compositor.
- `xrLocateViews` for the predicted display time → per-eye pose and `XrFovf`. The pose drives the camera (orientation: blocker 3 and `docs/head-and-body.md`; position: the existing camera-relative offset, rendering §6). The FOV builds the projection (blocker 1).
- Render both eyes — the existing double-`Draw` path, with the clean-stereo gating (rendering §13).
- Copy each eye into its XR swapchain image. The mod already captures the eye textures after the resolve (rendering §12), so this is a copy into the acquired swapchain image rather than new capture work.
- `xrEndFrame` submits an `XrCompositionLayerProjection` for the world plus, later, the HUD quad layer (`docs/hud.md`).

Present is already handled: the mod suppresses `Graphics::Flip` via `BLOCK_FLIP` (rendering §7), and the XR compositor presents instead. So the loop is mostly new OpenXR plumbing plus three reverse-engineering blockers, each scoped below.

## Blocker 1: per-eye off-axis projection

Real HMDs have asymmetric, per-eye frusta — the pupil isn't centred on its display half — described by the four `XrFovf` angles (`angleLeft`, `angleRight`, `angleUp`, `angleDown`). Today both eyes render with the engine's single symmetric projection, so even with the position offset the stereo isn't geometrically correct for a headset.

The preferred injection point is rendering §2.7's reverse-Z window: write a standard (non-reverse-Z) `m_Projection` (`+0x294`) on the render camera *before* `SetupRenderCamera` (`0x1400B3B80`), so the engine applies its reverse-Z and TAA jitter to it exactly once. Build the off-axis matrix directly from the four FOV angles (an off-centre perspective). The engine's own builder is worth confirming as a reference — `RecalcProjection` / a `PerspectiveOffCenter`-style function near `0x140013347` (cited in PLAN; addresses there are release-build but re-verify before use). Watch the §2.7 wedge bug: a projection written in the wrong depth convention produces a thin valid band and black elsewhere.

## Blocker 2: per-eye resolution

The runtime's recommended swapchain resolution rarely equals the desktop resolution, and a mismatched copy into the XR swapchain either crops or scales wrong. There is no dynamic-resolution path in the engine (rendering §9): every render target is sized from `device->m_DeviceInfo.m_DisplayWidth`/`m_DisplayHeight` through `CreateRenderSetups`, and per-pass viewports follow the bound RT size.

So the clean approach (rendering §9) is to set the device display size to the per-eye render resolution and re-run `CreateRenderSetups`; viewports then follow automatically, with no per-pass viewport patching. The open RE question is whether that re-init can be driven at runtime without tearing down live resources mid-session: xref `CreateRenderSetups`, find its caller inside `Graphics::Reset` / `SetDisplayMode`, and check what state it assumes (device idle, swapchain recreated). This wants confirming before we commit to runtime resolution changes.

## Blocker 3: HMD orientation, and the coordinate-frame gate

The camera hook currently writes position only — the translation columns of `m_TransformF` — and never orientation. Driving the head from the HMD needs a full rotation written into the render camera's `m_TransformF` / `m_CameraTransform`, after which the engine derives a consistent `m_View` (re-derive `m_View = Inverse(m_TransformF)` and rebuild the VP, rendering §2.5/§2.6).

Before writing any rotation, verify the coordinate frame. Rendering §15.7 (PLAN) concludes JC3 is "almost certainly" right-handed Y-up but flags it unverified. Run the experiment first: log the render camera's `m_TransformF` column 2 at `SetupRenderCamera`, press W, and confirm `-column2` aligns with travel direction. Guessing wrong mirrors or rotates the whole view. The body-vs-head split, vehicle handling, and the baked-animation conflict are in `docs/head-and-body.md`; this blocker is just the runtime-side gate plus the matrix write.

## Already solved — do not redo

Stereo geometry is fixed by gating `RotateRenderFrameData` on eye 1 (rendering §11). The per-eye camera *position* offset and the `m_View`/`m_ViewProjection` rebuild are done (rendering §6/§13). Present is suppressed (`BLOCK_FLIP`). Eye-texture capture after the resolve is done (rendering §12). The clean-stereo per-dispatch state gating is mostly in place; the remaining gaps (notably eye-1 `m_Dt = 0`) are tracked separately.
