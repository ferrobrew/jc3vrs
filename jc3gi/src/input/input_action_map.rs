#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct CInputActionMap {}
impl CInputActionMap {
    pub unsafe fn GetActionEffector(
        &mut self,
        action_id: i32,
        device_index: i32,
    ) -> *mut crate::input::input_action_map::CInputDeviceEffector {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                action_id: i32,
                device_index: i32,
            ) -> *mut crate::input::input_action_map::CInputDeviceEffector = ::std::mem::transmute(
                0x1402F43B0 as usize,
            );
            f(self as *mut Self as _, action_id, device_index)
        }
    }
}
impl std::convert::AsRef<CInputActionMap> for CInputActionMap {
    fn as_ref(&self) -> &CInputActionMap {
        self
    }
}
impl std::convert::AsMut<CInputActionMap> for CInputActionMap {
    fn as_mut(&mut self) -> &mut CInputActionMap {
        self
    }
}
#[repr(C, align(4))]
pub struct CInputDeviceEffector {
    pub m_Value: f32,
    pub m_PrevValue: f32,
    pub m_State: u32,
    pub m_IsAnalogue: bool,
    pub m_IsDeltaBased: bool,
    pub m_IsUpdated: bool,
    pub m_ForceClick: bool,
    pub m_StateTime: f32,
}
fn _CInputDeviceEffector_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x14], CInputDeviceEffector>([0u8; 0x14]);
    }
    unreachable!()
}
impl CInputDeviceEffector {}
impl std::convert::AsRef<CInputDeviceEffector> for CInputDeviceEffector {
    fn as_ref(&self) -> &CInputDeviceEffector {
        self
    }
}
impl std::convert::AsMut<CInputDeviceEffector> for CInputDeviceEffector {
    fn as_mut(&mut self) -> &mut CInputDeviceEffector {
        self
    }
}
