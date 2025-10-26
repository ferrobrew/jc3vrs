#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Resource, ID3D11ShaderResourceView, ID3D11RenderTargetView,
    ID3D11DepthStencilView,
};
#[repr(C, align(8))]
pub struct Texture {
    pub m_Texture: crate::graphics_engine::texture::ID3D11Resource,
    _field_8: [u8; 8],
    pub m_SRV: crate::graphics_engine::texture::ID3D11ShaderResourceView,
    pub m_RTV: crate::graphics_engine::texture::ID3D11RenderTargetView,
    pub m_DSV: crate::graphics_engine::texture::ID3D11DepthStencilView,
    _field_28: [u8; 20],
    pub m_Format: u32,
    _field_40: [u8; 24],
}
fn _Texture_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x58], Texture>([0u8; 0x58]);
    }
    unreachable!()
}
impl Texture {}
impl std::convert::AsRef<Texture> for Texture {
    fn as_ref(&self) -> &Texture {
        self
    }
}
impl std::convert::AsMut<Texture> for Texture {
    fn as_mut(&mut self) -> &mut Texture {
        self
    }
}
