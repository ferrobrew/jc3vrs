#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct CUIManager {
    _field_0: [u8; 5008],
    /// The Scaleform RenderBuffer the UI HAL renders into (set up by InitPlatformRT).
    pub m_RenderBuffer: *mut ::std::ffi::c_void,
    _field_1398: [u8; 236],
    /// Viewport width / height the world-to-screen mapping (Convert3DCoords) maps NDC into.
    pub m_ViewWidth: f32,
    pub m_ViewHeight: f32,
    _field_148c: [u8; 4],
}
fn _CUIManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1490], CUIManager>([0u8; 0x1490]);
    }
    unreachable!()
}
impl CUIManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417317920usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl CUIManager {
    pub const InitPlatformRT_ADDRESS: usize = 0x140F696E0;
    /// Bind the UI render target: build a Scaleform RenderTargetData (m_RenderBuffer) from the
    /// engine surface's RTV/DSV (GetRTVFromSurface / GetDSVFromSurface) via RenderTargetData::UpdateData.
    /// Called at startup (InitializeSystem) and on every device/resolution reset (RestoreAfterReset);
    /// `a2` carries the target dimensions.
    pub unsafe fn InitPlatformRT(&mut self, a2: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, a2: i32) = ::std::mem::transmute(
                Self::InitPlatformRT_ADDRESS,
            );
            f(self as *mut Self as _, a2)
        }
    }
    pub const Convert3DCoords_ADDRESS: usize = 0x140F69A70;
    /// World-to-screen: project `world` through `vp`, divide by w, aspect-correct, and map NDC to
    /// pixels (m_ViewWidth/Height). Returns false when the point is behind the camera.
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
    /// Marker placement: Convert3DCoords with the supplied `vp`, plus an on-screen test and an
    /// off-screen edge-clamp (ClampToScreen). Gameplay markers route through here.
    pub unsafe fn Get2DInfo(
        &self,
        world: *const crate::types::math::Vector3,
        vp: *const crate::types::math::Matrix4,
        camera: *const crate::types::math::Matrix4,
        a5: f32,
        out_x: *mut f32,
        out_y: *mut f32,
        out_pos: *mut u32,
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
                out_pos: *mut u32,
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
    /// Clamp an off-screen point to the screen-rect edge/corner (atan2 against the corners) and set
    /// the position code. Static.
    pub unsafe fn ClampToScreen(
        x: *mut f32,
        y: *mut f32,
        pos: *mut u32,
        min_x: f32,
        max_x: f32,
        min_y: f32,
        max_y: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                x: *mut f32,
                y: *mut f32,
                pos: *mut u32,
                min_x: f32,
                max_x: f32,
                min_y: f32,
                max_y: f32,
            ) = ::std::mem::transmute(Self::ClampToScreen_ADDRESS);
            f(x, y, pos, min_x, max_x, min_y, max_y)
        }
    }
    pub const StartRender_ADDRESS: usize = 0x140F1B030;
    /// Begin the UI render job (post to the async render thread).
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
    /// Barrier: spin-waits until the async UI render job has finished.
    pub unsafe fn SyncRender(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::SyncRender_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const Submit_ADDRESS: usize = 0x140F1B0D0;
    /// Final commit: submit the render HAL's output under a graphics scoped lock.
    pub unsafe fn Submit(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Submit_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const RenderStaticBackGround_ADDRESS: usize = 0x140F46C20;
    /// Draws the pause / menu static background (vtable method).
    pub unsafe fn RenderStaticBackGround(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RenderStaticBackGround_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const RenderOffScreenTextures_ADDRESS: usize = 0x1410076C0;
    /// Renders Scaleform UI to offscreen textures for in-world screens (not the HUD; vtable method).
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
    /// True when a static background is being shown.
    pub unsafe fn IsUsingStaticBackGround(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsUsingStaticBackGround_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<CUIManager> for CUIManager {
    fn as_ref(&self) -> &CUIManager {
        self
    }
}
impl std::convert::AsMut<CUIManager> for CUIManager {
    fn as_mut(&mut self) -> &mut CUIManager {
        self
    }
}
pub const GetIUIManager_ADDRESS: usize = 0x1400995A0;
/// Returns the single CUIManager instance (the concrete class behind IUIManager).
pub unsafe fn GetIUIManager() -> *mut crate::ui::ui_manager::CUIManager {
    unsafe {
        let f: unsafe extern "system" fn() -> *mut crate::ui::ui_manager::CUIManager = ::std::mem::transmute(
            GetIUIManager_ADDRESS,
        );
        f()
    }
}
