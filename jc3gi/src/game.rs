#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct Game {
    _field_0: [u8; 16],
    pub m_CountAccumulator: u64,
    pub m_UpdateFrequency: u32,
    pub m_DefaultUpdateFrequency: u32,
    _field_20: [u8; 20],
    pub m_Exit: u8,
    _field_35: [u8; 75],
    pub m_FilteredDrawTime: u64,
    _field_88: [u8; 12],
    pub m_RenderCount: u32,
    pub m_InterpolationMethod: u32,
    pub m_PrevInterpolationMethod: u32,
    pub m_InterpolationOverride: i32,
    _field_a4: [u8; 444],
    pub m_DecoupleEnabled: bool,
    _field_261: [u8; 87],
    pub m_UpdateFlags: u32,
    _field_2bc: [u8; 4],
}
fn _Game_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x2C0], Game>([0u8; 0x2C0]);
    }
    unreachable!()
}
impl Game {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417086568usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl Game {
    pub unsafe fn draw(&self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self, dt: f32) = ::std::mem::transmute(
                0x143C69C40 as usize,
            );
            f(self as *const Self as _, dt)
        }
    }
}
impl std::convert::AsRef<Game> for Game {
    fn as_ref(&self) -> &Game {
        self
    }
}
impl std::convert::AsMut<Game> for Game {
    fn as_mut(&mut self) -> &mut Game {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(8))]
pub struct GameObjectInitContext {
    pub m_Dt: f32,
    pub m_DtIgnorePause: f32,
    pub m_RealDt: f32,
    _field_c: [u8; 4],
    pub m_ResourceCache: *const ::std::ffi::c_void,
    pub m_ProjectContext: *const ::std::ffi::c_void,
}
fn _GameObjectInitContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], GameObjectInitContext>([0u8; 0x20]);
    }
    unreachable!()
}
impl GameObjectInitContext {}
impl std::convert::AsRef<GameObjectInitContext> for GameObjectInitContext {
    fn as_ref(&self) -> &GameObjectInitContext {
        self
    }
}
impl std::convert::AsMut<GameObjectInitContext> for GameObjectInitContext {
    fn as_mut(&mut self) -> &mut GameObjectInitContext {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(8))]
pub struct GameObjectRenderContext {
    pub m_Dt: f32,
    pub m_Dtf: f32,
    pub m_OriginalDt: f32,
    pub m_DtIgnorePause: f32,
    pub m_RealDt: f32,
    _field_14: [u8; 4],
    pub m_ProjectContext: *const ::std::ffi::c_void,
}
fn _GameObjectRenderContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], GameObjectRenderContext>([0u8; 0x20]);
    }
    unreachable!()
}
impl GameObjectRenderContext {}
impl std::convert::AsRef<GameObjectRenderContext> for GameObjectRenderContext {
    fn as_ref(&self) -> &GameObjectRenderContext {
        self
    }
}
impl std::convert::AsMut<GameObjectRenderContext> for GameObjectRenderContext {
    fn as_mut(&mut self) -> &mut GameObjectRenderContext {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(8))]
pub struct GameObjectUpdateContext {
    pub m_Dt: f32,
    pub m_OriginalDt: f32,
    pub m_DtIgnorePause: f32,
    pub m_RealDt: f32,
    pub m_SkippedDt: f32,
    pub m_Paused: bool,
    _field_15: [u8; 3],
    pub m_ProjectContext: *const ::std::ffi::c_void,
}
fn _GameObjectUpdateContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], GameObjectUpdateContext>([0u8; 0x20]);
    }
    unreachable!()
}
impl GameObjectUpdateContext {}
impl std::convert::AsRef<GameObjectUpdateContext> for GameObjectUpdateContext {
    fn as_ref(&self) -> &GameObjectUpdateContext {
        self
    }
}
impl std::convert::AsMut<GameObjectUpdateContext> for GameObjectUpdateContext {
    fn as_mut(&mut self) -> &mut GameObjectUpdateContext {
        self
    }
}
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
pub enum GameState {
    E_GAME_INSTALL = 0isize as _,
    E_GAME_INIT = 1isize as _,
    E_GAME_FRONTEND = 2isize as _,
    E_GAME_LOAD = 3isize as _,
    E_GAME_RUN = 4isize as _,
    E_GAME_STARTUP = 5isize as _,
    NOF_GAME_STATES = 6isize as _,
}
fn _GameState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], GameState>([0u8; 0x4]);
    }
    unreachable!()
}
impl GameState {
    pub unsafe fn get() -> Self {
        unsafe { *(0x142F3404C as *const Self) }
    }
}
impl GameState {
    pub unsafe fn post_update_render(
        update_contexts: *const crate::game::UpdateContexts,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                update_contexts: *const crate::game::UpdateContexts,
            ) = ::std::mem::transmute(0x143D2F130 as usize);
            f(update_contexts)
        }
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(8))]
pub struct UpdateContexts {
    pub m_InitContext: crate::game::GameObjectInitContext,
    pub m_UpdateContext: crate::game::GameObjectUpdateContext,
    pub m_RenderContext: crate::game::GameObjectRenderContext,
}
fn _UpdateContexts_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x60], UpdateContexts>([0u8; 0x60]);
    }
    unreachable!()
}
impl UpdateContexts {}
impl std::convert::AsRef<UpdateContexts> for UpdateContexts {
    fn as_ref(&self) -> &UpdateContexts {
        self
    }
}
impl std::convert::AsMut<UpdateContexts> for UpdateContexts {
    fn as_mut(&mut self) -> &mut UpdateContexts {
        self
    }
}
