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
    pub m_InterpolationOverride: u32,
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
impl Game {}
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
