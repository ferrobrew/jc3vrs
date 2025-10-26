#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(1))]
pub struct GameCameraManager {
    _field_0: [u8; 1880],
}
fn _GameCameraManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x758], GameCameraManager>([0u8; 0x758]);
    }
    unreachable!()
}
impl GameCameraManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418092208usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl GameCameraManager {}
impl std::convert::AsRef<GameCameraManager> for GameCameraManager {
    fn as_ref(&self) -> &GameCameraManager {
        self
    }
}
impl std::convert::AsMut<GameCameraManager> for GameCameraManager {
    fn as_mut(&mut self) -> &mut GameCameraManager {
        self
    }
}
