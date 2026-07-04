#![cfg_attr(any(), rustfmt::skip)]
#[allow(unused_imports)]
use crate::ui::scaleform::Movie;
#[repr(C, align(8))]
/// A Scaleform render buffer: what [`UIManager::m_RenderBuffer`] points at. It holds the
/// render-target and depth-stencil views the UI HAL renders into; [`UpdateData`](RenderTargetData::UpdateData)
/// rebinds those views, and is not tied to startup.
pub struct RenderTargetData {
    _field_0: [u8; 40],
    /// The render-target buffer width. [`InitPlatformRT`](UIManager::InitPlatformRT) builds the
    /// buffer square, setting width = height = its side argument.
    pub m_BufferWidth: i32,
    /// The render-target buffer height; [`InitPlatformRT`](UIManager::InitPlatformRT) sets it equal to
    /// the width (a square buffer).
    pub m_BufferHeight: i32,
    _field_30: [u8; 8],
    /// The view rectangle's right edge (x2 = width).
    pub m_ViewRectRight: i32,
    /// The view rectangle's bottom edge (y2); [`InitPlatformRT`](UIManager::InitPlatformRT) sets it
    /// equal to the width.
    pub m_ViewRectBottom: i32,
}
fn _RenderTargetData_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x40], RenderTargetData>([0u8; 0x40]);
    }
    unreachable!()
}
impl RenderTargetData {
    pub const UpdateData_ADDRESS: usize = 0x141DE0CF0;
    /// Rebinds the buffer's views, releasing the old ones and adding a reference to the new. `self`
    /// is the render buffer itself ([`UIManager::m_RenderBuffer`]); the call reaches its inner
    /// view-holder internally. Passing `depth` as null leaves the depth buffer unchanged.
    pub unsafe fn UpdateData(
        &mut self,
        rtv: *mut ::std::ffi::c_void,
        depth: *mut ::std::ffi::c_void,
        dsv: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                rtv: *mut ::std::ffi::c_void,
                depth: *mut ::std::ffi::c_void,
                dsv: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::UpdateData_ADDRESS);
            f(self as *mut Self as _, rtv, depth, dsv)
        }
    }
}
impl std::convert::AsRef<RenderTargetData> for RenderTargetData {
    fn as_ref(&self) -> &RenderTargetData {
        self
    }
}
impl std::convert::AsMut<RenderTargetData> for RenderTargetData {
    fn as_mut(&mut self) -> &mut RenderTargetData {
        self
    }
}
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The screen-position code returned by the UI world-to-screen and marker placement: on-screen, an
/// off-screen clamped edge or corner, off-screen in front (no clamp), or off-screen behind.
pub enum ScreenPos {
    SCREEN_POS_ONSCREEN = 0isize as _,
    SCREEN_POS_OFFSCREEN_LEFT = 1isize as _,
    SCREEN_POS_OFFSCREEN_RIGHT = 2isize as _,
    SCREEN_POS_OFFSCREEN_TOP = 3isize as _,
    SCREEN_POS_OFFSCREEN_BOTTOM = 4isize as _,
    SCREEN_POS_OFFSCREEN_TOP_LEFT = 5isize as _,
    SCREEN_POS_OFFSCREEN_TOP_RIGHT = 6isize as _,
    SCREEN_POS_OFFSCREEN_BOTTOM_LEFT = 7isize as _,
    SCREEN_POS_OFFSCREEN_BOTTOM_RIGHT = 8isize as _,
    SCREEN_POS_OFFSCREEN_NO_CLAMP_IN_FRONT_CAMERA = 9isize as _,
    SCREEN_POS_OFFSCREEN_NO_CLAMP_BEHIND_CAMERA = 10isize as _,
}
fn _ScreenPos_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], ScreenPos>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The Scaleform-backed UI manager, the single concrete instance behind `IUIManager`. It renders the
/// HUD into the engine surface. [`InitPlatformRT`](UIManager::InitPlatformRT) builds its render
/// buffer, [`ComputeMovieSizeOnViewSize`](UIManager::ComputeMovieSizeOnViewSize) sizes the movie
/// render rectangle, [`SetMovieViewport`](UIManager::SetMovieViewport) sets the Scaleform movie
/// viewport, and [`ComputeSafeArea`](UIManager::ComputeSafeArea) derives the UI safe area.
pub struct UIManager {
    _field_0: [u8; 52],
    /// The movie render rectangle's width, from the engine's `MovieScaleInfo`. The movie is rendered
    /// into a `m_MovieScaleWidth` x `m_MovieScaleHeight` rectangle, centered within the viewport
    /// passed to [`SetMovieViewport`](UIManager::SetMovieViewport).
    /// [`ComputeMovieSizeOnViewSize`](UIManager::ComputeMovieSizeOnViewSize) recomputes it from the
    /// device resolution and the movie stage size.
    pub m_MovieScaleWidth: i32,
    /// The movie render rectangle's height; see [`m_MovieScaleWidth`](UIManager::m_MovieScaleWidth).
    pub m_MovieScaleHeight: i32,
    _field_3c: [u8; 8],
    /// The id of the thread that currently owns the Scaleform capture (`m_CurrentCaptureThread`);
    /// `PreRender` claims it for the update thread each frame via `CUIManager::SetCaptureThread`,
    /// which writes this field and the movie's capture thread together. `CUIBase::Invoke` runs the
    /// AVM immediately only when the calling thread matches this field, and queues into the UI's
    /// command queue otherwise -- so a hook that borrows capture ownership must write this field
    /// too, or game-thread invokes keep mutating the display list concurrently.
    pub m_CurrentCaptureThread: u32,
    _field_48: [u8; 458],
    /// Whether the render system is initialized. One of the three gates `Render` checks before
    /// drawing.
    pub m_RenderReady: bool,
    /// Whether rendering is active (cleared during device resets). The second `Render` gate.
    pub m_RenderActive: bool,
    _field_214: [u8; 4260],
    /// The lock serializing the UI update (`PreRender`: `Advance` + `Capture`) against the UI
    /// render (`Render`, `RenderOffScreenTextures`). A Win32 critical section, so re-entrant on
    /// the owning thread: a `Render` hook can hold it across visibility writes, captures, and
    /// calls to the original (which re-enters it).
    pub m_DeferredRenderLock: *mut crate::graphics_engine::device::CRITICAL_SECTION,
    _field_12c0: [u8; 24],
    /// The Scaleform `GFx::Loader`, created in `InitializeSystem`. Loads `.gfx` files via
    /// `Loader::CreateMovie`.
    pub m_Loader: *mut ::std::ffi::c_void,
    /// The `GFx::MovieDef` for `ui/root.gfx` -- the definition object, not the live instance.
    pub m_MovieDef: *mut ::std::ffi::c_void,
    /// The live `GFx::Movie` instance (a [`MovieImpl`]), created by `MovieDef::CreateInstance` in
    /// `InitializeSystem`. All `CUIBase` subclasses share this single movie. The AS3 side (the
    /// [`Movie`](ui::scaleform::Movie) interface: SetVariable, Invoke, the display tree) hangs off
    /// [`MovieImpl::pASMovieRoot`].
    pub m_Movie: *mut crate::ui::scaleform::MovieImpl,
    _field_12f0: [u8; 160],
    /// The Scaleform render buffer the UI HAL renders into, set up by
    /// [`InitPlatformRT`](UIManager::InitPlatformRT). [`RenderTargetData::UpdateData`] rebinds which
    /// views it renders into.
    pub m_RenderBuffer: *mut crate::ui::ui_manager::RenderTargetData,
    _field_1398: [u8; 231],
    /// The third `Render` gate: whether UI rendering is enabled at all.
    pub m_RenderingEnabled: bool,
    _field_1480: [u8; 4],
    /// The movie stage's authored width (`m_CachedStageSize.x`), refreshed every frame from the loaded
    /// movie. The world-to-screen mapping ([`Convert3DCoords`](UIManager::Convert3DCoords)) maps NDC
    /// into this.
    pub m_CachedStageWidth: f32,
    /// The movie stage's authored height; see [`m_CachedStageWidth`](UIManager::m_CachedStageWidth).
    pub m_CachedStageHeight: f32,
    _field_148c: [u8; 12],
    /// The cached viewport width (`m_CachedViewportSize.x`), refreshed every frame from the graphics
    /// device's display resolution. [`ComputeSafeArea`](UIManager::ComputeSafeArea) reads it to expand
    /// the UI safe area to the viewport's aspect.
    pub m_CachedViewportWidth: i32,
    /// The cached viewport height; see [`m_CachedViewportWidth`](UIManager::m_CachedViewportWidth).
    pub m_CachedViewportHeight: i32,
    /// The cached viewport aspect ratio (`m_CachedViewportSize.y / .x`, i.e. device height / width),
    /// refreshed every frame from the graphics device. [`Convert3DCoords`](UIManager::Convert3DCoords)
    /// uses it -- not the width/height fields -- to aspect-correct world-to-screen.
    pub m_CachedViewportRatio: f32,
    _field_14a4: [u8; 4],
}
fn _UIManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x14A8], UIManager>([0u8; 0x14A8]);
    }
    unreachable!()
}
impl UIManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417317920usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl UIManager {
    pub const Render_ADDRESS: usize = 0x141007B70;
    /// Binds the UI render target: builds a [`RenderTargetData`] from the engine surface's
    /// render-target and depth-stencil views via [`RenderTargetData::UpdateData`]. Called at startup
    /// and on every device or resolution reset; `a2` carries the target side length (the buffer is
    /// square: width = height = a2). This only creates the render buffer -- the Scaleform movie's
    /// viewport is set separately by [`SetMovieViewport`](UIManager::SetMovieViewport).
    /// The UI render: takes [`m_DeferredRenderLock`](UIManager::m_DeferredRenderLock), checks the
    /// three render gates, retargets the Scaleform render-thread ids, drains the thread command
    /// queue, binds the display render target, and draws the movie's latest captured display tree
    /// (`GetDisplayHandle` -> `RTHandle::NextCapture` -> `HAL::Draw`) within a
    /// `BeginFrame`/`BeginScene` pair. Runs on a CPU-fragment worker (kicked by `StartRender`,
    /// joined by `SyncRender`). `IUIManager` vtable slot 4.
    pub unsafe fn Render(
        &mut self,
        context: *mut crate::graphics_engine::graphics_engine::HContext_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                context: *mut crate::graphics_engine::graphics_engine::HContext_t,
            ) = ::std::mem::transmute(Self::Render_ADDRESS);
            f(self as *mut Self as _, context)
        }
    }
    pub const InitPlatformRT_ADDRESS: usize = 0x140F696E0;
    pub unsafe fn InitPlatformRT(&mut self, a2: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, a2: i32) = ::std::mem::transmute(
                Self::InitPlatformRT_ADDRESS,
            );
            f(self as *mut Self as _, a2)
        }
    }
    pub const RestoreAfterReset_ADDRESS: usize = 0x140FA9C70;
    /// Re-runs [`InitPlatformRT`](UIManager::InitPlatformRT) after a device or resolution reset. Also
    /// recomputes the movie size, sets the movie viewport, and recomputes the safe area. Gated by an
    /// internal reset counter, so a direct call may no-op if the counter is not at 1.
    pub unsafe fn RestoreAfterReset(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RestoreAfterReset_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const ComputeMovieSizeOnViewSize_ADDRESS: usize = 0x140F46830;
    /// Recomputes the movie render rectangle ([`m_MovieScaleWidth`](UIManager::m_MovieScaleWidth) /
    /// [`m_MovieScaleHeight`](UIManager::m_MovieScaleHeight)). It first refreshes the cached stage and
    /// viewport sizes from the live device resolution (via an internal `UpdateCachedValues`), then
    /// sizes the movie rectangle from them, so the rectangle always reflects the device aspect.
    pub unsafe fn ComputeMovieSizeOnViewSize(&mut self, a2: bool, a3: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, a2: bool, a3: bool) = ::std::mem::transmute(
                Self::ComputeMovieSizeOnViewSize_ADDRESS,
            );
            f(self as *mut Self as _, a2, a3)
        }
    }
    pub const SetMovieViewport_ADDRESS: usize = 0x140F1B260;
    /// Sets the Scaleform movie's viewport to `width` x `height`, centering the movie (of size
    /// [`m_MovieScaleWidth`](UIManager::m_MovieScaleWidth) x
    /// [`m_MovieScaleHeight`](UIManager::m_MovieScaleHeight)) within it at offset
    /// `((width - m_MovieScaleWidth) / 2, (height - m_MovieScaleHeight) / 2)`. This is the viewport the
    /// Scaleform HAL renders into.
    pub unsafe fn SetMovieViewport(&mut self, width: i32, height: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, width: i32, height: i32) = ::std::mem::transmute(
                Self::SetMovieViewport_ADDRESS,
            );
            f(self as *mut Self as _, width, height)
        }
    }
    pub const ComputeSafeArea_ADDRESS: usize = 0x140F89B30;
    /// Computes the safe-area rectangle from the current movie viewport.
    pub unsafe fn ComputeSafeArea(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::ComputeSafeArea_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const ComputeGameViewArea_ADDRESS: usize = 0x140F89CB0;
    /// Computes the game view area from the current movie viewport.
    pub unsafe fn ComputeGameViewArea(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::ComputeGameViewArea_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const Convert3DCoords_ADDRESS: usize = 0x140F69A70;
    /// World-to-screen: projects `world` through `vp`, divides by w, aspect-corrects, and maps NDC to
    /// pixels. Returns false when the point is behind the camera.
    pub unsafe fn Convert3DCoords(
        &self,
        world: *const crate::types::math::Vector3,
        out_x: *mut f32,
        out_y: *mut f32,
        vp: *const crate::types::math::Matrix4,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                world: *const crate::types::math::Vector3,
                out_x: *mut f32,
                out_y: *mut f32,
                vp: *const crate::types::math::Matrix4,
            ) -> bool = ::std::mem::transmute(Self::Convert3DCoords_ADDRESS);
            f(self as *const Self as _, world, out_x, out_y, vp)
        }
    }
    pub const Convert3DCoordsDefault_ADDRESS: usize = 0x140F899A0;
    /// The default-VP world-to-screen wrapper `CHUDUI::UpdateGrappleReticle` uses for the grapple
    /// reticle (its only callers): fetches the render camera's view-projection internally and
    /// forwards to [`Convert3DCoords`](UIManager::Convert3DCoords). Because the VP is not a
    /// parameter, the grapple reticle bypasses the floating panel's marker reprojection unless
    /// this wrapper is hooked.
    pub unsafe fn Convert3DCoordsDefault(
        &mut self,
        world: *const crate::types::math::Vector3,
        out_x: *mut f32,
        out_y: *mut f32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                world: *const crate::types::math::Vector3,
                out_x: *mut f32,
                out_y: *mut f32,
            ) -> bool = ::std::mem::transmute(Self::Convert3DCoordsDefault_ADDRESS);
            f(self as *mut Self as _, world, out_x, out_y)
        }
    }
    pub const Get2DInfo_ADDRESS: usize = 0x140F69CB0;
    /// Marker placement: [`Convert3DCoords`](UIManager::Convert3DCoords) with the supplied `vp`, plus
    /// an on-screen test and an off-screen edge-clamp via [`ClampToScreen`](UIManager::ClampToScreen).
    /// Gameplay markers route through here.
    pub unsafe fn Get2DInfo(
        &self,
        world: *const crate::types::math::Vector3,
        vp: *const crate::types::math::Matrix4,
        camera: *const crate::types::math::Matrix4,
        a5: f32,
        out_x: *mut f32,
        out_y: *mut f32,
        out_pos: *mut crate::ui::ui_manager::ScreenPos,
        margin: f32,
        a10: bool,
        offset: crate::types::math::Vector2,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                world: *const crate::types::math::Vector3,
                vp: *const crate::types::math::Matrix4,
                camera: *const crate::types::math::Matrix4,
                a5: f32,
                out_x: *mut f32,
                out_y: *mut f32,
                out_pos: *mut crate::ui::ui_manager::ScreenPos,
                margin: f32,
                a10: bool,
                offset: crate::types::math::Vector2,
            ) = ::std::mem::transmute(Self::Get2DInfo_ADDRESS);
            f(
                self as *const Self as _,
                world,
                vp,
                camera,
                a5,
                out_x,
                out_y,
                out_pos,
                margin,
                a10,
                offset,
            )
        }
    }
    pub const ClampToScreen_ADDRESS: usize = 0x140F470A0;
    /// Clamps an off-screen point to the screen-rect edge or corner and sets the position code.
    /// Static.
    pub unsafe fn ClampToScreen(
        x: *mut f32,
        y: *mut f32,
        pos: *mut crate::ui::ui_manager::ScreenPos,
        min_x: f32,
        max_x: f32,
        min_y: f32,
        max_y: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                x: *mut f32,
                y: *mut f32,
                pos: *mut crate::ui::ui_manager::ScreenPos,
                min_x: f32,
                max_x: f32,
                min_y: f32,
                max_y: f32,
            ) = ::std::mem::transmute(Self::ClampToScreen_ADDRESS);
            f(x, y, pos, min_x, max_x, min_y, max_y)
        }
    }
    pub const StartRender_ADDRESS: usize = 0x140F1B030;
    /// Begins the UI render job, posting to the async render thread.
    pub unsafe fn StartRender(
        &mut self,
        context: *const crate::graphics_engine::graphics_engine::HContext_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                context: *const crate::graphics_engine::graphics_engine::HContext_t,
            ) = ::std::mem::transmute(Self::StartRender_ADDRESS);
            f(self as *mut Self as _, context)
        }
    }
    pub const SyncRender_ADDRESS: usize = 0x140F1B0C0;
    /// A barrier that spin-waits until the async UI render job has finished.
    pub unsafe fn SyncRender(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::SyncRender_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const Submit_ADDRESS: usize = 0x140F1B0D0;
    /// The final commit: submits the render HAL's output under a graphics scoped lock.
    pub unsafe fn Submit(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Submit_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const RenderStaticBackGround_ADDRESS: usize = 0x140F46C20;
    /// Draws the pause / menu static background.
    pub unsafe fn RenderStaticBackGround(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RenderStaticBackGround_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const RenderOffScreenTextures_ADDRESS: usize = 0x1410076C0;
    /// Renders Scaleform UI to offscreen textures for in-world screens. This is not the HUD.
    pub unsafe fn RenderOffScreenTextures(
        &mut self,
        ctx: *mut *mut crate::graphics_engine::graphics_engine::HContext_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut *mut crate::graphics_engine::graphics_engine::HContext_t,
            ) = ::std::mem::transmute(Self::RenderOffScreenTextures_ADDRESS);
            f(self as *mut Self as _, ctx)
        }
    }
    pub const IsUsingStaticBackGround_ADDRESS: usize = 0x140F1B4C0;
    /// Whether a static background is being shown.
    pub unsafe fn IsUsingStaticBackGround(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsUsingStaticBackGround_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<UIManager> for UIManager {
    fn as_ref(&self) -> &UIManager {
        self
    }
}
impl std::convert::AsMut<UIManager> for UIManager {
    fn as_mut(&mut self) -> &mut UIManager {
        self
    }
}
pub const GetIUIManager_ADDRESS: usize = 0x1400995A0;
/// Returns the single [`UIManager`] instance.
pub unsafe fn GetIUIManager() -> *mut crate::ui::ui_manager::UIManager {
    unsafe {
        let f: unsafe extern "system" fn() -> *mut crate::ui::ui_manager::UIManager = ::std::mem::transmute(
            GetIUIManager_ADDRESS,
        );
        f()
    }
}
