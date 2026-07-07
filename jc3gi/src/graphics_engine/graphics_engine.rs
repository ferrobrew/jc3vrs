#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
pub enum ActiveCursor {
    None = -1isize as _,
    Arrow = 0isize as _,
    Cross = 1isize as _,
    Slider = 2isize as _,
    Zoom = 3isize as _,
}
fn _ActiveCursor_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], ActiveCursor>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The opaque parameter block passed to
/// [`HandleDrawThreadTask`](GraphicsEngine::HandleDrawThreadTask).
pub struct DrawThreadTaskParam {}
impl DrawThreadTaskParam {}
impl std::convert::AsRef<DrawThreadTaskParam> for DrawThreadTaskParam {
    fn as_ref(&self) -> &DrawThreadTaskParam {
        self
    }
}
impl std::convert::AsMut<DrawThreadTaskParam> for DrawThreadTaskParam {
    fn as_mut(&mut self) -> &mut DrawThreadTaskParam {
        self
    }
}
#[repr(C, align(8))]
/// One reflection-proxy depth-history slot in the planar / water reflection state machine. Five of
/// these live on the graphics engine; the lifecycle byte advances once per scene dispatch.
pub struct EffectInfo {
    /// The reflection-proxy depth texture.
    pub m_DepthTexture: *mut crate::graphics_engine::graphics_engine::HTexture_t,
    pub m_Transform: crate::types::math::Matrix4,
    /// The lifecycle counter: `0` is free, `2` promotes to `3`, `3` is picked, otherwise it
    /// increments.
    pub m_FrameIndex: u8,
    _field_49: [u8; 7],
}
fn _EffectInfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x50], EffectInfo>([0u8; 0x50]);
    }
    unreachable!()
}
impl EffectInfo {}
impl std::convert::AsRef<EffectInfo> for EffectInfo {
    fn as_ref(&self) -> &EffectInfo {
        self
    }
}
impl std::convert::AsMut<EffectInfo> for EffectInfo {
    fn as_mut(&mut self) -> &mut EffectInfo {
        self
    }
}
#[repr(C, align(8))]
pub struct GraphicsEngine {
    _field_0: [u8; 8],
    /// Whether the engine has finished its system initialisation. [`ResizeBuffers`](GraphicsEngine::ResizeBuffers)
    /// only applies a resize inline once this is set.
    pub m_HasBeenInitialized: bool,
    _field_9: [u8; 15],
    pub m_CPUFinishedDrawingEvent: u32,
    _field_1c: [u8; 20],
    /// Completion signal for the async draw-dispatch CPU fragment that
    /// [`DispatchDraw`](GraphicsEngine::DispatchDraw) kicks to run the render passes. The fragment sets
    /// it non-zero on completion; the engine waits on it (via
    /// `cpu_fragment::CpuFragmentWaitUntilSignalIsNonZero`, gated by `CpuPrimaryCount() > 1`) only at the
    /// *next* [`Draw`](GraphicsEngine::Draw)'s entry.
    /// [`WaitForCPUDrawToFinish`](GraphicsEngine::WaitForCPUDrawToFinish) does *not* wait on it.
    pub m_DrawThreadWorkSignal: u32,
    _field_34: [u8; 212],
    /// The display-mode state machine serviced once per frame by
    /// [`HandleModeChange`](GraphicsEngine::HandleModeChange): while idle it applies a deferred
    /// window resize, and while a mode change is pending it applies the fullscreen/adapter change.
    pub m_DisplayModeChangeState: u32,
    _field_10c: [u8; 12],
    /// Set at the tail of [`ApplyResize`](GraphicsEngine::ApplyResize) once a resize has been applied
    /// and a valid display mode is in effect.
    pub m_HasNewValidDisplayMode: bool,
    _field_119: [u8; 3],
    /// The pending deferred-resize width, stashed by [`ResizeBuffers`](GraphicsEngine::ResizeBuffers)
    /// and consumed by [`HandleModeChange`](GraphicsEngine::HandleModeChange).
    pub m_WindowWidth: u32,
    /// The pending deferred-resize height.
    pub m_WindowHeight: u32,
    /// When set, [`ResizeBuffers`](GraphicsEngine::ResizeBuffers) applies a resize inline rather than
    /// deferring it; the fullscreen/adapter path sets it around its device reset.
    pub m_SynchronousResize: bool,
    /// Set when a deferred resize is pending in [`m_WindowWidth`](GraphicsEngine::m_WindowWidth)/
    /// [`m_WindowHeight`](GraphicsEngine::m_WindowHeight), consumed by
    /// [`HandleModeChange`](GraphicsEngine::HandleModeChange).
    pub m_HasNewWindowSettings: bool,
    _field_126: [u8; 2],
    pub m_ActiveCursor: crate::graphics_engine::graphics_engine::ActiveCursor,
    _field_12c: [u8; 44],
    /// The cascaded sun-shadow system.
    pub m_ShadowManager: *mut crate::graphics_engine::shadow_manager::ShadowManager,
    _field_160: [u8; 16],
    /// The engine-owned scene render camera: a by-value copy rebuilt each
    /// [`Draw`](GraphicsEngine::Draw) by `Camera::SetupRenderCamera` (reverse-Z + jitter, then the
    /// view-projection products from `m_View`). This is the camera the render passes consume;
    /// distinct from the `CameraManager`'s camera objects, which are pointers to the gameplay
    /// cameras.
    pub m_RenderCamera: crate::camera::camera::Camera,
    _field_720: [u8; 1936],
    pub m_Device: *mut crate::graphics_engine::device::Device,
    _field_eb8: [u8; 16],
    /// Deferred-shading render targets ("GBuffer0".."GBuffer3").
    pub m_GBufferTexture: [*mut crate::graphics_engine::texture::Texture; 4],
    _field_ee8: [u8; 184],
    /// Motion-blur velocity buffer.
    pub m_VelocityBufferTexture: *mut crate::graphics_engine::texture::Texture,
    _field_fa8: [u8; 168],
    /// The final composite render setup: colour → [`m_BackBufferLinear`](GraphicsEngine::m_BackBufferLinear),
    /// depth → the main depth surface. Built by
    /// [`CreateRenderSetups`](GraphicsEngine::CreateRenderSetups) against the live swapchain back
    /// buffer and stored as the render context's setup; the HUD and the scene resolve target it.
    pub m_BackBufferRenderSetup: *mut crate::graphics_engine::graphics_engine::HRenderSetup_t,
    /// Main scene depth ("MainDepthBuffer").
    pub m_MainDepthTexture: *mut crate::graphics_engine::texture::Texture,
    _field_1060: [u8; 8],
    /// Main scene HDR color ("MainColorBuffer").
    pub m_MainColorBuffer: *mut crate::graphics_engine::texture::Texture,
    _field_1070: [u8; 8],
    /// Hi-Z / downsampled depth ("DownsampledDepth").
    pub m_DownSampledDepthTexture: *mut crate::graphics_engine::texture::Texture,
    _field_1080: [u8; 32],
    /// Reflection-proxy depth-history (planar / water reflection state machine), 5 slots.
    pub m_EffectInfo: [crate::graphics_engine::graphics_engine::EffectInfo; 5],
    /// Final linear back buffer ("BackBufferLinear").
    pub m_BackBufferLinear: *mut crate::graphics_engine::texture::Texture,
    _field_1238: [u8; 134],
    /// Whether [`HandleDrawThreadTask`](GraphicsEngine::HandleDrawThreadTask) renders the 3D scene
    /// this frame (GBuffer, world, post-effects) rather than only the UI. [`Draw`](GraphicsEngine::Draw)
    /// sets it from the game state and the UI's static-background grab: it is cleared while a
    /// full-screen UI with a static background is shown (pause / map / menus), in which case the draw
    /// thread clears the target to transparent instead of rendering the scene.
    pub m_DrawScene: bool,
    _field_12bf: [u8; 9],
    /// Index of the reflection-proxy slot picked this frame.
    pub m_EffectInfoIndex: u32,
    _field_12cc: [u8; 52],
    /// The currently-loaded shader bundle name, compared by
    /// [`LoadShaderBundle`](GraphicsEngine::LoadShaderBundle) to skip a same-name reload. The
    /// bundle names are `"Shaders"` / `"ShadersLowShadows"` (and the Intel `"ShadersConstMath*"`
    /// variants).
    pub m_CurrentBundleName: crate::types::std_string::String,
    _field_1320: [u8; 3056],
}
fn _GraphicsEngine_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1F10], GraphicsEngine>([0u8; 0x1F10]);
    }
    unreachable!()
}
impl GraphicsEngine {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417121520usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl GraphicsEngine {
    pub const WaitForCPUDrawToFinish_ADDRESS: usize = 0x1400C4690;
    pub unsafe fn WaitForCPUDrawToFinish(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::WaitForCPUDrawToFinish_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const Draw_ADDRESS: usize = 0x1400F4170;
    /// The graphics entry point: runs the per-frame prologue (presents the previous frame, advances
    /// the clock and constant-buffer pools), then dispatches this frame's draw.
    pub unsafe fn Draw(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::Draw_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const Flip_ADDRESS: usize = 0x1400B89D0;
    /// Presents the previous frame's back buffer.
    pub unsafe fn Flip(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Flip_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const DispatchDraw_ADDRESS: usize = 0x1400F3A30;
    /// Queues this frame's render work on the render thread.
    pub unsafe fn DispatchDraw(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::DispatchDraw_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const HandleDrawThreadTask_ADDRESS: usize = 0x1400F1D10;
    /// The render-thread body: GBuffer, lighting, post-effects, and UI.
    pub unsafe fn HandleDrawThreadTask(
        &mut self,
        param: *mut crate::graphics_engine::graphics_engine::DrawThreadTaskParam,
    ) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                param: *mut crate::graphics_engine::graphics_engine::DrawThreadTaskParam,
            ) -> u32 = ::std::mem::transmute(Self::HandleDrawThreadTask_ADDRESS);
            f(self as *mut Self as _, param)
        }
    }
    pub const TextureCachePlatformUpdate_ADDRESS: usize = 0x1400C46D0;
    /// A draw-prologue step. Copies the active camera into the engine-owned render-camera slot, runs
    /// [`Camera::SetupRenderCamera`] on it, publishes it as the camera manager's render camera, then
    /// runs the per-frame texture-cache update under the context lock.
    pub unsafe fn TextureCachePlatformUpdate(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        dt: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                dt: f32,
            ) = ::std::mem::transmute(Self::TextureCachePlatformUpdate_ADDRESS);
            f(self as *mut Self as _, ctx, dt)
        }
    }
    pub const CreateRenderSetups_ADDRESS: usize = 0x1400CE930;
    /// Creates every scene render target and render setup. Each scene target
    /// (main depth/colour, the four GBuffers, velocity, downsampled depth, the reflection-proxy
    /// targets, the AO volume, and the `VfxDepthCopy_%d` slots) is sized from
    /// `device_info`'s [`m_DisplayWidth`](graphics_engine::device::DeviceInfo::m_DisplayWidth)/
    /// [`m_DisplayHeight`](graphics_engine::device::DeviceInfo::m_DisplayHeight) (some at half
    /// resolution), so it can be re-run at any size by passing a modified copy. The final
    /// `BackBufferLinear` alias plus [`m_BackBufferRenderSetup`](GraphicsEngine::m_BackBufferRenderSetup)
    /// and the post-effect setup are instead built against the live swapchain surface via
    /// `GetDeviceSurface(BackBuffer)`, independent of
    /// `device_info`. Per-pass viewports follow the bound target's size, so no per-pass viewport
    /// changes are needed. Assumes the previously created setups have been torn down
    /// ([`DestroyRenderSetups`](GraphicsEngine::DestroyRenderSetups)) and that no draw is in flight.
    pub unsafe fn CreateRenderSetups(
        &mut self,
        device_info: *const crate::graphics_engine::device::DeviceInfo,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                device_info: *const crate::graphics_engine::device::DeviceInfo,
            ) -> bool = ::std::mem::transmute(Self::CreateRenderSetups_ADDRESS);
            f(self as *mut Self as _, device_info)
        }
    }
    pub const DestroyRenderSetups_ADDRESS: usize = 0x1400C4090;
    /// Destroys every scene render target, surface, and render setup created by
    /// [`CreateRenderSetups`](GraphicsEngine::CreateRenderSetups), including the `BackBufferLinear`
    /// alias, after first unbinding the active setup. The swapchain back buffer itself is not touched.
    /// Pass-owned render targets (post-effect, SSAO, SSR, anti-aliasing pools) are not freed here — the
    /// registered resize callbacks re-allocate those.
    pub unsafe fn DestroyRenderSetups(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::DestroyRenderSetups_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const ApplyResize_ADDRESS: usize = 0x1400CFA90;
    /// Applies a resize: tears down the render setups, has the UI drop its references, resizes the
    /// swapchain (the device-level `ResizeBuffers`), re-creates the render
    /// setups at the new size ([`CreateRenderSetups`](GraphicsEngine::CreateRenderSetups)), runs every
    /// registered resize callback (which re-sizes the pass-owned pools), restores the UI, and updates
    /// the camera aspect and window params. Called from
    /// [`HandleModeChange`](GraphicsEngine::HandleModeChange) in the [`Draw`](GraphicsEngine::Draw)
    /// prologue, so it runs on the main thread with no draw in flight.
    pub unsafe fn ApplyResize(&mut self, width: u32, height: u32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, width: u32, height: u32) = ::std::mem::transmute(
                Self::ApplyResize_ADDRESS,
            );
            f(self as *mut Self as _, width, height)
        }
    }
    pub const ResizeBuffers_ADDRESS: usize = 0x1400D43C0;
    /// The resize request entry. Applies the resize inline via
    /// [`ApplyResize`](GraphicsEngine::ApplyResize) when
    /// [`m_SynchronousResize`](GraphicsEngine::m_SynchronousResize) and
    /// [`m_HasBeenInitialized`](GraphicsEngine::m_HasBeenInitialized) are set; otherwise it stashes the
    /// dimensions in [`m_WindowWidth`](GraphicsEngine::m_WindowWidth)/
    /// [`m_WindowHeight`](GraphicsEngine::m_WindowHeight) and defers to the next
    /// [`HandleModeChange`](GraphicsEngine::HandleModeChange).
    pub unsafe fn ResizeBuffers(&mut self, width: u32, height: u32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, width: u32, height: u32) = ::std::mem::transmute(
                Self::ResizeBuffers_ADDRESS,
            );
            f(self as *mut Self as _, width, height)
        }
    }
    pub const HandleModeChange_ADDRESS: usize = 0x1400F40C0;
    /// Services the display-mode state machine once per frame from the [`Draw`](GraphicsEngine::Draw)
    /// prologue: applies a deferred resize ([`ApplyResize`](GraphicsEngine::ApplyResize)) when one is
    /// pending, or the fullscreen/adapter change when a mode change is pending, then reconciles the
    /// flip interval. It runs after the previous frame's draw dispatch has drained and before the
    /// current frame is dispatched.
    pub unsafe fn HandleModeChange(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::HandleModeChange_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const LoadShaderBundle_ADDRESS: usize = 0x1400DE9A0;
    /// Loads (or reloads) the named shader bundle and re-creates every shader holder from it, but only
    /// if `name` differs from [`m_CurrentBundleName`](GraphicsEngine::m_CurrentBundleName).
    /// Re-creating the holders routes every shader through
    /// `Graphics::CreateFragmentProgram`. The bundle names are `"Shaders"` / `"ShadersLowShadows"`
    /// (and the Intel `"ShadersConstMath*"` variants), selected by shadow quality in
    /// `CSettingsManager::UpdateSettings`. `name` is a NUL-terminated C string.
    pub unsafe fn LoadShaderBundle(&mut self, name: *const u8) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, name: *const u8) -> bool = ::std::mem::transmute(
                Self::LoadShaderBundle_ADDRESS,
            );
            f(self as *mut Self as _, name)
        }
    }
    pub const SetCursor_ADDRESS: usize = 0x1400A1AB0;
    /// Sets [`m_ActiveCursor`](GraphicsEngine::m_ActiveCursor) and, when it changed, posts
    /// `WM_SETCURSOR` to the game window so `WndProc` runs
    /// [`UpdateCursor`](GraphicsEngine::UpdateCursor). `COverlayUI` drives it: `Arrow` when the
    /// overlay cursor becomes visible, `None` when it hides.
    pub unsafe fn SetCursor(
        &mut self,
        cursor: crate::graphics_engine::graphics_engine::ActiveCursor,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                cursor: crate::graphics_engine::graphics_engine::ActiveCursor,
            ) = ::std::mem::transmute(Self::SetCursor_ADDRESS);
            f(self as *mut Self as _, cursor)
        }
    }
    pub const UpdateCursor_ADDRESS: usize = 0x1400A1AF0;
    /// Applies [`m_ActiveCursor`](GraphicsEngine::m_ActiveCursor) to the OS: for `None` it sets a
    /// null `HCURSOR` and clips the cursor to the client rect (when the window is foreground);
    /// otherwise it unclips and sets `GraphicsParams::m_Cursors[cursor]`. The four entries are
    /// loaded at startup as the system `IDC_ARROW`, `IDC_CROSS`, `IDC_HAND`, and `IDC_NO`
    /// cursors. Called from `WndProc` on `WM_SETCURSOR`.
    pub unsafe fn UpdateCursor(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::UpdateCursor_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<GraphicsEngine> for GraphicsEngine {
    fn as_ref(&self) -> &GraphicsEngine {
        self
    }
}
impl std::convert::AsMut<GraphicsEngine> for GraphicsEngine {
    fn as_mut(&mut self) -> &mut GraphicsEngine {
        self
    }
}
#[repr(C, align(8))]
pub struct GraphicsParams {
    pub m_AppTitle: *const u8,
    pub m_Cursors: [crate::graphics_engine::graphics_engine::HICON; 4],
    pub m_Hwnd: crate::graphics_engine::graphics_engine::HWND,
    pub m_FullscreenWidth: i32,
    pub m_FullscreenHeight: i32,
    pub m_WindowedWidth: i32,
    pub m_WindowedHeight: i32,
    pub m_Fullscreen: bool,
    pub m_HighResShadows: bool,
    _field_42: [u8; 2],
    pub m_Width: u32,
    pub m_Height: u32,
    pub m_IsHighDef: bool,
    _field_4d: [u8; 3],
    pub m_DisplayPresentationInterval: u32,
    pub m_RendertargetCount: u32,
    pub m_RefreshRate: u16,
    _field_5a: [u8; 22],
}
fn _GraphicsParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x70], GraphicsParams>([0u8; 0x70]);
    }
    unreachable!()
}
impl GraphicsParams {}
impl std::convert::AsRef<GraphicsParams> for GraphicsParams {
    fn as_ref(&self) -> &GraphicsParams {
        self
    }
}
impl std::convert::AsMut<GraphicsParams> for GraphicsParams {
    fn as_mut(&mut self) -> &mut GraphicsParams {
        self
    }
}
#[repr(C, align(8))]
/// A GPU context handle.
pub struct HContext_t {}
impl HContext_t {}
impl std::convert::AsRef<HContext_t> for HContext_t {
    fn as_ref(&self) -> &HContext_t {
        self
    }
}
impl std::convert::AsMut<HContext_t> for HContext_t {
    fn as_mut(&mut self) -> &mut HContext_t {
        self
    }
}
#[repr(C, align(8))]
/// A GPU device handle.
pub struct HDevice_t {}
impl HDevice_t {}
impl std::convert::AsRef<HDevice_t> for HDevice_t {
    fn as_ref(&self) -> &HDevice_t {
        self
    }
}
impl std::convert::AsMut<HDevice_t> for HDevice_t {
    fn as_mut(&mut self) -> &mut HDevice_t {
        self
    }
}
pub use windows::Win32::UI::WindowsAndMessaging::HICON as HICON;
#[repr(C, align(8))]
/// A render-target configuration a pass draws into.
pub struct HRenderSetup_t {}
impl HRenderSetup_t {}
impl std::convert::AsRef<HRenderSetup_t> for HRenderSetup_t {
    fn as_ref(&self) -> &HRenderSetup_t {
        self
    }
}
impl std::convert::AsMut<HRenderSetup_t> for HRenderSetup_t {
    fn as_mut(&mut self) -> &mut HRenderSetup_t {
        self
    }
}
#[repr(C, align(8))]
/// A GPU texture handle.
pub struct HTexture_t {}
impl HTexture_t {}
impl std::convert::AsRef<HTexture_t> for HTexture_t {
    fn as_ref(&self) -> &HTexture_t {
        self
    }
}
impl std::convert::AsMut<HTexture_t> for HTexture_t {
    fn as_mut(&mut self) -> &mut HTexture_t {
        self
    }
}
pub use windows::Win32::Foundation::HWND as HWND;
#[repr(C, align(8))]
/// The per-view render context the render passes read: the camera matrices (view, projection, the
/// translation-free offset view-projection, and the separate camera world position), shadow data, and
/// per-frame flags. Filled each dispatch by [`RenderPass::SetRenderContextCamera`].
pub struct RenderContext {
    _field_0: [u8; 216],
    /// The translation-free view-projection for this dispatch (the rotation and projection without the
    /// camera world translation). The tessellation constant buffers bake it directly (e.g.
    /// [`RenderBlockTypeTerrain::SetupConstantBuffers`](crate::graphics_engine::render_block::RenderBlockTypeTerrain)),
    /// so it carries the per-view (and, off-axis, per-eye) projection.
    pub m_OffsetViewProjection: crate::types::math::Matrix4,
    _field_118: [u8; 816],
    /// The per-real-frame stamp for this dispatch, set from [`get_render_frame_counters`]'s `m_FrameIndex`
    /// during render-context setup. Passes that cache per-frame state key on it — the terrain
    /// tessellation blocks compare it against their per-slot
    /// [`m_WasCBApplied`](graphics_engine::render_block::RenderBlockTypeTerrain::m_WasCBApplied) stamp
    /// to decide whether to re-upload the constant buffer.
    pub m_RenderFrameNo: u32,
    /// The 8 per-atlas-slice projective shadow transforms, copied per dispatch from the shadow
    /// manager's parity storage. The deferred lighting shaders index them dynamically by a light's
    /// packed slice index (`cb0[63 + 4*slice .. 66 + 4*slice]` in the GlobalConstants) to project a
    /// light-relative position into its shadow-atlas slice; the sun resolve uses
    /// [`m_ShadowCascades`](RenderContext::m_ShadowCascades) instead.
    /// The instance-transform slot the render blocks pass to
    /// [`RBIInfo::GetMatrix`](graphics_engine::render_block::RBIInfo::GetMatrix) for the current
    /// dispatch.
    pub m_TransformIndex: u32,
    _field_450: [u8; 4],
    pub m_ShadowMatrices: [crate::types::math::Matrix4; 8],
    /// The forward-material cascaded sun-shadow transform + cascade box-test parameters.
    pub m_ShadowCascades: crate::graphics_engine::graphics_engine::ShadowCascades,
    /// The number of active cascades this frame, copied per dispatch from the shadow manager's
    /// parity storage (a byte store in [`RenderPass::SetRenderContextCamera`]).
    pub m_ActiveCascadeCount: u8,
    _field_775: [u8; 3],
    /// The pass-family status bits for the current draw: `0x1` default, `0x2` static shadow map,
    /// `0x4` dynamic shadow map. Render blocks branch on `& 6` to select the shadow/depth-only
    /// path versus the full material path.
    pub m_RenderStatus: u32,
    _field_77c: [u8; 4],
}
fn _RenderContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x780], RenderContext>([0u8; 0x780]);
    }
    unreachable!()
}
impl RenderContext {}
impl std::convert::AsRef<RenderContext> for RenderContext {
    fn as_ref(&self) -> &RenderContext {
        self
    }
}
impl std::convert::AsMut<RenderContext> for RenderContext {
    fn as_mut(&mut self) -> &mut RenderContext {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// Per-real-frame counters, advanced once in the [`GraphicsEngine::Draw`] prologue. `m_FrameIndex`
/// (set from the post-incrementing `m_Counter`) drives the TAA jitter phase and shadow parity;
/// `m_RingIndex` is the three-slot constant-buffer ring.
pub struct RenderFrameCounters {
    pub m_Counter: u32,
    pub m_FrameIndex: u32,
    pub m_RingIndex: u32,
}
fn _RenderFrameCounters_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xC], RenderFrameCounters>([0u8; 0xC]);
    }
    unreachable!()
}
impl RenderFrameCounters {}
impl std::convert::AsRef<RenderFrameCounters> for RenderFrameCounters {
    fn as_ref(&self) -> &RenderFrameCounters {
        self
    }
}
impl std::convert::AsMut<RenderFrameCounters> for RenderFrameCounters {
    fn as_mut(&mut self) -> &mut RenderFrameCounters {
        self
    }
}
#[repr(C, align(4))]
/// The cascaded sun-shadow constants for the forward-material resolve: the cascade transform plus the
/// per-cascade box-test parameters, staged into cb0 by `RenderEngine::SetGlobalShaderConstants`.
pub struct ShadowCascades {
    /// Maps a camera-relative world position into cascade/texture space (row-major, row-vector; its
    /// columns are `cb0[45..47]` in the GlobalConstants, the translation `cb0[48]`). The
    /// forward-material shadow resolve evaluates `(worldPos - cameraPos) * M + translation`, with the
    /// camera position from `cb0[4]`; `CShadowManager::UpdateCascade` bakes the transform relative to
    /// the active camera's position.
    pub m_Transform: crate::types::math::Matrix4,
    pub m_TextureSize: crate::types::math::Vector4,
    pub m_Params: crate::types::math::Vector4,
    /// Per-cascade scale + blend-band (`.w`), one per cascade.
    pub m_ScaleBlend: [crate::types::math::Vector4; 6],
    /// Per-cascade offset + texture-array slice (`.w`), one per cascade.
    pub m_OffsetRadius: [crate::types::math::Vector4; 6],
}
fn _ShadowCascades_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x120], ShadowCascades>([0u8; 0x120]);
    }
    unreachable!()
}
impl ShadowCascades {}
impl std::convert::AsRef<ShadowCascades> for ShadowCascades {
    fn as_ref(&self) -> &ShadowCascades {
        self
    }
}
impl std::convert::AsMut<ShadowCascades> for ShadowCascades {
    fn as_mut(&mut self) -> &mut ShadowCascades {
        self
    }
}
pub unsafe fn get_graphics_params() -> &'static mut crate::graphics_engine::graphics_engine::GraphicsParams {
    unsafe {
        &mut *(0x142D3A850
            as *mut crate::graphics_engine::graphics_engine::GraphicsParams)
    }
}
pub unsafe fn get_render_frame_counters() -> &'static mut crate::graphics_engine::graphics_engine::RenderFrameCounters {
    unsafe {
        &mut *(0x142D3A6AC
            as *mut crate::graphics_engine::graphics_engine::RenderFrameCounters)
    }
}
pub const graphics_flip_ADDRESS: usize = 0x14195A820;
/// The low-level present.
unsafe fn graphics_flip(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
) -> i32 {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
        ) -> i32 = ::std::mem::transmute(graphics_flip_ADDRESS);
        f(device)
    }
}
