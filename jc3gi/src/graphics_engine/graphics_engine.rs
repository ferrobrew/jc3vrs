#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::HICON};
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
/// CGraphicsEngine::DrawThreadTaskParam
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
/// One reflection-proxy depth-history slot (the planar / water reflection state machine). Five of
/// these live on the graphics engine; the lifecycle byte advances once per scene dispatch.
pub struct EffectInfo {
    /// Reflection-proxy depth texture (Graphics::HTexture_t handle).
    pub m_DepthTexture: *mut ::std::ffi::c_void,
    pub m_Transform: crate::types::math::Matrix4,
    /// Lifecycle counter: 0 = free, 2 -> promote to 3, 3 = pick, else += 1.
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
    _field_0: [u8; 24],
    pub m_CPUFinishedDrawingEvent: u32,
    _field_1c: [u8; 268],
    pub m_ActiveCursor: crate::graphics_engine::graphics_engine::ActiveCursor,
    _field_12c: [u8; 3460],
    pub m_Device: *mut crate::graphics_engine::device::Device,
    _field_eb8: [u8; 16],
    /// Deferred-shading render targets ("GBuffer0".."GBuffer3").
    pub m_GBufferTexture: [*mut crate::graphics_engine::texture::Texture; 4],
    _field_ee8: [u8; 184],
    /// Motion-blur velocity buffer.
    pub m_VelocityBufferTexture: *mut crate::graphics_engine::texture::Texture,
    _field_fa8: [u8; 176],
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
    _field_1238: [u8; 144],
    /// Index of the reflection-proxy slot picked this frame.
    pub m_EffectInfoIndex: u32,
    _field_12cc: [u8; 3140],
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
    /// Graphics entry point: runs the per-frame prologue (presents the previous frame, advances the
    /// clock and constant-buffer pools) then dispatches this frame's draw.
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
    /// Render-thread body: gbuffer, lighting, post-effects and UI. `param` is
    /// DrawThreadTaskParam* (layout TBD).
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
/// Graphics::HContext_t (GPU context handle)
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
/// Graphics::HDevice_t (GPU device handle)
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
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// Per-real-frame counters advanced once in the CGraphicsEngine::Draw prologue. m_FrameIndex (set
/// from m_Counter, which post-increments) drives the TAA jitter phase and shadow parity (& 1);
/// m_RingIndex is the %3 constant-buffer ring (m_FrameIndex % 3).
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
/// Low-level present (Graphics::Flip); returns Graphics::EResult. `device` is
/// Graphics::HDevice_t* (opaque).
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
