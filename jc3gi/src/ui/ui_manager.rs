#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A Scaleform render buffer: what [`UIManager::m_RenderBuffer`] points at. Pointing its bound views
/// at an offscreen render-target view redirects where the UI HAL renders, and the rebind is not tied
/// to startup, so it can happen at any time.
pub struct RenderTargetData {}
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
/// HUD into the engine surface; [`InitPlatformRT`](UIManager::InitPlatformRT) rebinds its render
/// target.
pub struct UIManager {
    _field_0: [u8; 5008],
    /// The Scaleform render buffer the UI HAL renders into, set up by
    /// [`InitPlatformRT`](UIManager::InitPlatformRT). Pass it to [`RenderTargetData::UpdateData`] to
    /// rebind where the HUD renders.
    pub m_RenderBuffer: *mut crate::ui::ui_manager::RenderTargetData,
    _field_1398: [u8; 236],
    /// The viewport width that the world-to-screen mapping
    /// ([`Convert3DCoords`](UIManager::Convert3DCoords)) maps NDC into.
    pub m_ViewWidth: f32,
    pub m_ViewHeight: f32,
    _field_148c: [u8; 4],
}
fn _UIManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1490], UIManager>([0u8; 0x1490]);
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
    pub const InitPlatformRT_ADDRESS: usize = 0x140F696E0;
    /// Binds the UI render target: builds a [`RenderTargetData`] from the engine surface's
    /// render-target and depth-stencil views via [`RenderTargetData::UpdateData`]. Called at startup
    /// and on every device or resolution reset; `a2` carries the target dimensions.
    pub unsafe fn InitPlatformRT(&mut self, a2: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, a2: i32) = ::std::mem::transmute(
                Self::InitPlatformRT_ADDRESS,
            );
            f(self as *mut Self as _, a2)
        }
    }
    pub const RestoreAfterReset_ADDRESS: usize = 0x140FA9C70;
    /// Re-runs [`InitPlatformRT`](UIManager::InitPlatformRT) after a device or resolution reset (`a2`
    /// is the new width; the dimensions otherwise track the engine surface).
    pub unsafe fn RestoreAfterReset(&mut self, a2: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, a2: i32) = ::std::mem::transmute(
                Self::RestoreAfterReset_ADDRESS,
            );
            f(self as *mut Self as _, a2)
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
