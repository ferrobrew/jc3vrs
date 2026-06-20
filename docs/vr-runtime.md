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

## Reference scaffold

`d3d11-openxr-example` is a minimal, working D3D11 + OpenXR loop (the `openxr` 0.21 crate over `windows` 0.62) with exactly this shape — share a D3D11 device with the runtime, one stereo swapchain, `locate_views` → per-eye pose and FOV → off-axis projection → submit a projection layer. Use it as the skeleton; the differences for JC3 are:

- **Share the game's device, don't create one.** The example calls `D3D11CreateDevice` itself; we instead pass the engine's existing `ID3D11Device` to `create_session::<xr::D3D11>` (rendering §8 — the device behind the master context at `engine+2616`). The session must run on the same device the game renders on.
- **The game renders the scene; we copy into the swapchain.** The example renders its triangle straight into the swapchain RTV. We can't — JC3 renders through its own pipeline into `m_BackBufferLinear` and we capture each eye after the resolve (rendering §12). So per frame: `acquire_image` / `wait_image`, `CopyResource` our captured eye texture into the swapchain image (on the immediate context, under `m_Context->m_Mutex`, rendering §8), `release_image`. The example's single swapchain with `array_size = 2` — one array slice per eye, RTV as `TEXTURE2DARRAY` — is a clean target: eye 0 → slice 0, eye 1 → slice 1.
- **Drive the camera from `locate_views`, don't render from it.** The example feeds each view's pose and FOV into a per-view VP in a constant buffer. We instead feed the pose into the game camera (blocker 3) and the FOV into the game projection (blocker 1), once per eye, and let the engine render the scene.
- **Frame pacing replaces the game's own.** `frame_wait.wait()` → `frame_stream.begin()` → render → `frame_stream.end(predicted_display_time, blend_mode, &[projection_layer])`. The HUD quad layer (`docs/hud.md`) becomes a second layer in that `end` call once we move past the in-scene quad. Present stays suppressed (`BLOCK_FLIP`, rendering §7).

The example also has the input scaffold — action sets, `/interaction_profiles/khr/simple_controller`, grip-pose spaces, `sync_actions` + `locate` — that the OpenXR half of controller input (`docs/input.md`) builds on.

## Blocker 1: per-eye off-axis projection

Real HMDs have asymmetric, per-eye frusta — the pupil isn't centred on its display half — described by the four `XrFovf` angles (`angleLeft`, `angleRight`, `angleUp`, `angleDown`). Today both eyes render with the engine's single symmetric projection, so even with the position offset the stereo isn't geometrically correct for a headset.

The preferred injection point is rendering §2.7's reverse-Z window: write a standard (non-reverse-Z) `m_Projection` on the render camera *before* `SetupRenderCamera`, so the engine applies its reverse-Z and TAA jitter to it exactly once. Build the off-axis matrix directly from the four FOV angles (an off-centre perspective). The engine's own builder is worth confirming as a reference — `RecalcProjection` / a `PerspectiveOffCenter`-style function (cited in PLAN, not yet a def; find and verify before use). Watch the §2.7 wedge bug: a projection written in the wrong depth convention produces a thin valid band and black elsewhere. The example builds the off-axis matrix straight from the four `XrFovf` angles — `Mat4::frustum_rh(near·tan(angleLeft), near·tan(angleRight), near·tan(angleDown), near·tan(angleUp), near, far)` — which is the matrix we need; the only adaptation is depth convention, since the example uses standard depth (`LESS`, near 0.1 / far 100) and JC3 is reverse-Z (§2.7), so build that off-axis frustum in the engine's convention via the pre-`SetupRenderCamera` write.

## Blocker 2: per-eye resolution

The runtime's recommended swapchain resolution rarely equals the desktop resolution, and a mismatched copy into the XR swapchain either crops or scales wrong. (That per-eye resolution is what the example reads from `enumerate_view_configuration_views(system, VIEW_TYPE)[0].recommended_image_rect_width` / `_height`.) There is no dynamic-resolution path in the engine (rendering §9): every render target is sized from `device->m_DeviceInfo.m_DisplayWidth`/`m_DisplayHeight` through `CreateRenderSetups`, and per-pass viewports follow the bound RT size.

So the clean approach (rendering §9) is to set the device display size to the per-eye render resolution and re-run `CreateRenderSetups`; viewports then follow automatically, with no per-pass viewport patching. The open RE question is whether that re-init can be driven at runtime without tearing down live resources mid-session: xref `CreateRenderSetups`, find its caller inside `Graphics::Reset` / `SetDisplayMode`, and check what state it assumes (device idle, swapchain recreated). This wants confirming before we commit to runtime resolution changes.

## Blocker 3: HMD orientation, and the coordinate-frame gate

The camera hook currently writes position only — the translation columns of `m_TransformF` — and never orientation. Driving the head from the HMD needs a full rotation written into the render camera's `m_TransformF` / `m_CameraTransform`, after which the engine derives a consistent `m_View` (re-derive `m_View = Inverse(m_TransformF)` and rebuild the VP, rendering §2.5/§2.6).

Before writing any rotation, verify the coordinate frame. Rendering §15.7 (PLAN) concludes JC3 is "almost certainly" right-handed Y-up but flags it unverified. Run the experiment first: log the render camera's `m_TransformF` column 2 at `SetupRenderCamera`, press W, and confirm `-column2` aligns with travel direction. Guessing wrong mirrors or rotates the whole view. The body-vs-head split, vehicle handling, and the baked-animation conflict are in `docs/head-and-body.md`; this blocker is just the runtime-side gate plus the matrix write.

## Already solved — do not redo

Stereo geometry is fixed by gating `RotateRenderFrameData` on eye 1 (rendering §11). The per-eye camera *position* offset and the `m_View`/`m_ViewProjection` rebuild are done (rendering §6/§13). Present is suppressed (`BLOCK_FLIP`). Eye-texture capture after the resolve is done (rendering §12). The clean-stereo per-dispatch state gating is mostly in place; the remaining gaps (notably eye-1 `m_Dt = 0`) are tracked separately.
