#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct Clock {
    pub __vftable: u64,
    pub m_HasBeenInitialized: bool,
    _field_9: [u8; 7],
    pub m_FPS: f32,
    pub m_SPF: f32,
    pub m_RealFPS: f32,
    pub m_RealSPF: f32,
    pub m_UpdateSpeed: f32,
    pub m_ForceToThisFPS: f32,
    pub m_ForceToThisSPF: f32,
    pub m_Stop: bool,
    pub m_ForceToFps: bool,
    _field_2e: [u8; 2],
    pub m_Time: u64,
    pub m_RealTimeAtUpdateGame: u64,
    pub m_PauseTimeAtUpdateGame: u64,
    pub m_GameTimeMicro: u64,
    pub m_GameRunTimeMicro: u64,
    pub m_ElapsedTime: u64,
    pub m_LastCount: u64,
    pub m_CpuFrequency: u64,
}
fn _Clock_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x70], Clock>([0u8; 0x70]);
    }
    unreachable!()
}
impl Clock {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417799288usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl Clock {
    pub unsafe fn get_spf(&self, ignore_pause: bool) -> f32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                ignore_pause: bool,
            ) -> f32 = ::std::mem::transmute(0x1432AC860 as usize);
            f(self as *const Self as _, ignore_pause)
        }
    }
    pub unsafe fn pause(&mut self, pause: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, pause: bool) = ::std::mem::transmute(
                0x1432AC7E0 as usize,
            );
            f(self as *mut Self as _, pause)
        }
    }
}
impl std::convert::AsRef<Clock> for Clock {
    fn as_ref(&self) -> &Clock {
        self
    }
}
impl std::convert::AsMut<Clock> for Clock {
    fn as_mut(&mut self) -> &mut Clock {
        self
    }
}
