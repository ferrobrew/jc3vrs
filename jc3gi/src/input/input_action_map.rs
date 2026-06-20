#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// Maps action IDs to effector slots (255 action IDs total).
pub struct InputActionMap {}
impl InputActionMap {
    pub const GetActionEffector_ADDRESS: usize = 0x1402F43B0;
    pub unsafe fn GetActionEffector(
        &mut self,
        action_id: i32,
        device_index: i32,
    ) -> *mut crate::input::input_action_map::InputDeviceEffector {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                action_id: i32,
                device_index: i32,
            ) -> *mut crate::input::input_action_map::InputDeviceEffector = ::std::mem::transmute(
                Self::GetActionEffector_ADDRESS,
            );
            f(self as *mut Self as _, action_id, device_index)
        }
    }
}
impl std::convert::AsRef<InputActionMap> for InputActionMap {
    fn as_ref(&self) -> &InputActionMap {
        self
    }
}
impl std::convert::AsMut<InputActionMap> for InputActionMap {
    fn as_mut(&mut self) -> &mut InputActionMap {
        self
    }
}
#[repr(C, align(4))]
/// One input effector slot. Layout from the debug PDB (Input::InputDeviceEffector, 0x14),
/// cross-checked against retail usage (m_Value@0, m_State@8, m_StateTime@0x10 all match). The
/// pointer GetActionEffector returns is the head of a linked-list node whose extra id/next fields
/// follow this struct; reading the effector itself uses these offsets.
pub struct InputDeviceEffector {
    pub m_Value: f32,
    pub m_PrevValue: f32,
    pub m_State: u32,
    pub m_IsAnalogue: bool,
    pub m_IsDeltaBased: bool,
    pub m_IsUpdated: bool,
    pub m_ForceClick: bool,
    pub m_StateTime: f32,
}
fn _InputDeviceEffector_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x14], InputDeviceEffector>([0u8; 0x14]);
    }
    unreachable!()
}
impl InputDeviceEffector {}
impl std::convert::AsRef<InputDeviceEffector> for InputDeviceEffector {
    fn as_ref(&self) -> &InputDeviceEffector {
        self
    }
}
impl std::convert::AsMut<InputDeviceEffector> for InputDeviceEffector {
    fn as_mut(&mut self) -> &mut InputDeviceEffector {
        self
    }
}
