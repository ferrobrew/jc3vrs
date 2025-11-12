#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct InputDeviceManager {
    vftable: *const crate::input::input_device_manager::InputDeviceManagerVftable,
    _field_8: [u8; 4],
    pub m_Enabled: bool,
    _field_d: [u8; 107],
}
fn _InputDeviceManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x78], InputDeviceManager>([0u8; 0x78]);
    }
    unreachable!()
}
impl InputDeviceManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417121528usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl InputDeviceManager {
    pub fn vftable(
        &self,
    ) -> *const crate::input::input_device_manager::InputDeviceManagerVftable {
        self.vftable
            as *const crate::input::input_device_manager::InputDeviceManagerVftable
    }
    pub unsafe fn Destructor(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).Destructor).read();
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<InputDeviceManager> for InputDeviceManager {
    fn as_ref(&self) -> &InputDeviceManager {
        self
    }
}
impl std::convert::AsMut<InputDeviceManager> for InputDeviceManager {
    fn as_mut(&mut self) -> &mut InputDeviceManager {
        self
    }
}
#[repr(C, align(8))]
pub struct InputDeviceManagerVftable {
    pub Destructor: unsafe extern "system" fn(
        this: *mut crate::input::input_device_manager::InputDeviceManager,
    ),
}
fn _InputDeviceManagerVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], InputDeviceManagerVftable>([0u8; 0x8]);
    }
    unreachable!()
}
impl InputDeviceManagerVftable {}
impl std::convert::AsRef<InputDeviceManagerVftable> for InputDeviceManagerVftable {
    fn as_ref(&self) -> &InputDeviceManagerVftable {
        self
    }
}
impl std::convert::AsMut<InputDeviceManagerVftable> for InputDeviceManagerVftable {
    fn as_mut(&mut self) -> &mut InputDeviceManagerVftable {
        self
    }
}
