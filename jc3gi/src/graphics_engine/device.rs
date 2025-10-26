#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
use windows::Win32::Graphics::{
    Direct3D11::ID3D11Device, Dxgi::{IDXGISwapChain, IDXGIOutput},
};
#[repr(C, align(1))]
pub struct Context {
    _field_0: [u8; 36080],
}
fn _Context_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8CF0], Context>([0u8; 0x8CF0]);
    }
    unreachable!()
}
impl Context {}
impl std::convert::AsRef<Context> for Context {
    fn as_ref(&self) -> &Context {
        self
    }
}
impl std::convert::AsMut<Context> for Context {
    fn as_mut(&mut self) -> &mut Context {
        self
    }
}
#[repr(C, align(8))]
pub struct Device {
    pub m_Context: *mut crate::graphics_engine::device::Context,
    _field_8: [u8; 24],
    pub m_SwapChain: crate::graphics_engine::device::IDXGISwapChain,
    pub m_Device: crate::graphics_engine::device::ID3D11Device,
    _field_30: [u8; 8],
    pub m_DXGIOutput: crate::graphics_engine::device::IDXGIOutput,
    _field_40: [u8; 36160],
}
fn _Device_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8D80], Device>([0u8; 0x8D80]);
    }
    unreachable!()
}
impl Device {}
impl std::convert::AsRef<Device> for Device {
    fn as_ref(&self) -> &Device {
        self
    }
}
impl std::convert::AsMut<Device> for Device {
    fn as_mut(&mut self) -> &mut Device {
        self
    }
}
