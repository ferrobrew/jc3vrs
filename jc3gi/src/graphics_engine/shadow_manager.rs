#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// One cascade's bookkeeping in [`ShadowManager`]: the render passes that draw its atlas slice,
/// followed by the per-parity fit state.
pub struct CascadeData {
    /// The pair of render passes that draw this cascade's atlas slice (dynamic and static
    /// geometry). [`ShadowManager::CommitRenderPassSettings`] gates them per dispatch via
    /// [`RenderPassState::m_Enabled`](graphics_engine::render_pass::RenderPassState::m_Enabled).
    pub m_Passes: [*mut crate::graphics_engine::render_pass::RenderPass; 2],
    _field_10: [u8; 1104],
}
fn _CascadeData_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x460], CascadeData>([0u8; 0x460]);
    }
    unreachable!()
}
impl CascadeData {}
impl std::convert::AsRef<CascadeData> for CascadeData {
    fn as_ref(&self) -> &CascadeData {
        self
    }
}
impl std::convert::AsMut<CascadeData> for CascadeData {
    fn as_mut(&mut self) -> &mut CascadeData {
        self
    }
}
#[repr(C, align(8))]
/// The cascaded sun-shadow system. Each sim frame, [`UpdateRender`](ShadowManager::UpdateRender)
/// fits the scheduled cascades to the active camera and writes the parity-buffered fit data; each
/// dispatch, [`CommitRenderPassSettings`](ShadowManager::CommitRenderPassSettings) enables the
/// scheduled atlas passes. Cascade re-renders are amortised across frames by an update pattern, so
/// an unscheduled cascade keeps its previous fit and contents.
pub struct ShadowManager {
    _field_0: [u8; 4],
    /// The settings-side enable flag. The per-frame [`UpdateRender`](ShadowManager::UpdateRender)
    /// syncs the engine to it via [`SetEnabled`](ShadowManager::SetEnabled) whenever it differs
    /// from the live state -- the graphics-menu path for toggling shadows.
    pub m_Enabled: bool,
    _field_5: [u8; 299],
    /// The per-cascade slots (passes plus fit bookkeeping).
    pub m_Cascades: [crate::graphics_engine::shadow_manager::CascadeData; 8],
    _field_2430: [u8; 56],
    /// The per-cascade shadow-map update level, indexed by cascade. A cascade re-fits and re-renders
    /// only every `2^level` frames (level 0 = every frame); between refreshes its fit is copied
    /// forward from the previous update. `CalculateUpdatePattern` assigns the levels (the default
    /// pattern is `{0, 1, 2, 3}` -- the nearest cascade every frame, each further one half as often),
    /// and [`SetActiveShadowPassCount`](ShadowManager::SetActiveShadowPassCount) reads them each frame,
    /// gated against a rolling counter, to decide which cascades refresh. This is the mechanism that
    /// amortises cascade re-renders across frames.
    pub m_CascadeUpdateLevels: [i32; 6],
    _field_2480: [u8; 15744],
}
fn _ShadowManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x6200], ShadowManager>([0u8; 0x6200]);
    }
    unreachable!()
}
impl ShadowManager {
    pub const GetShadowFade_ADDRESS: usize = 0x140177940;
    /// The global sun-shadow fade factor staged into the shader constants.
    pub unsafe fn GetShadowFade(&self) -> f32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> f32 = ::std::mem::transmute(
                Self::GetShadowFade_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const CommitRenderPassSettings_ADDRESS: usize = 0x1401779C0;
    /// The per-dispatch pass gate: clears every shadow pass's
    /// [`m_Enabled`](graphics_engine::render_pass::RenderPassState::m_Enabled) flag, then
    /// re-enables the passes the update pattern scheduled this frame and re-points their render
    /// targets by the frame parity.
    pub unsafe fn CommitRenderPassSettings(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
            ) = ::std::mem::transmute(Self::CommitRenderPassSettings_ADDRESS);
            f(self as *mut Self as _, ctx)
        }
    }
    pub const SetEnabled_ADDRESS: usize = 0x14019EE50;
    /// Creates or destroys the shadow render targets and passes; the settings path behind
    /// [`m_Enabled`](ShadowManager::m_Enabled).
    pub unsafe fn SetEnabled(&mut self, enabled: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, enabled: bool) = ::std::mem::transmute(
                Self::SetEnabled_ADDRESS,
            );
            f(self as *mut Self as _, enabled)
        }
    }
    pub const UpdateRender_ADDRESS: usize = 0x1401C7370;
    /// The sim-side per-frame update: syncs [`m_Enabled`](ShadowManager::m_Enabled) via
    /// [`SetEnabled`](ShadowManager::SetEnabled), fits the scheduled cascades to
    /// [`CameraManager::m_ActiveCamera`](camera::camera_manager::CameraManager::m_ActiveCamera)
    /// (the fit frustum comes from that camera's `m_ProjectionF`), copies the unscheduled
    /// cascades' previous fits forward, writes the parity-indexed cascade transforms and per-slice
    /// matrices the draw side reads, and regenerates the cull planes.
    pub unsafe fn UpdateRender(&mut self, dt: f32, dtf: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32, dtf: f32) = ::std::mem::transmute(
                Self::UpdateRender_ADDRESS,
            );
            f(self as *mut Self as _, dt, dtf)
        }
    }
    pub const SetActiveShadowPassCount_ADDRESS: usize = 0x14018A7D0;
    /// Sets the number of active shadow passes (sun cascades plus spot shadows) and rebuilds the
    /// per-frame cascade update schedule: it refreshes the amortisation pattern (recomputing
    /// [`m_CascadeUpdateLevels`](ShadowManager::m_CascadeUpdateLevels) on a cascade/spot count change),
    /// then marks each cascade as either refreshing this frame -- when
    /// `((1 << m_CascadeUpdateLevels[c]) - 1) & rolling_counter == 0` -- or copying its previous fit
    /// forward. Called from `CGameStateRun::UpdateShadows` before
    /// [`UpdateRender`](ShadowManager::UpdateRender).
    pub unsafe fn SetActiveShadowPassCount(&mut self, count: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, count: i32) = ::std::mem::transmute(
                Self::SetActiveShadowPassCount_ADDRESS,
            );
            f(self as *mut Self as _, count)
        }
    }
}
impl std::convert::AsRef<ShadowManager> for ShadowManager {
    fn as_ref(&self) -> &ShadowManager {
        self
    }
}
impl std::convert::AsMut<ShadowManager> for ShadowManager {
    fn as_mut(&mut self) -> &mut ShadowManager {
        self
    }
}
