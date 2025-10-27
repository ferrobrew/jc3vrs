#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct Character {
    _field_0: [u8; 10288],
    m_WorldMatrixT1: crate::types::math::Matrix4,
    _field_2870: [u8; 3280],
}
fn _Character_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x3540], Character>([0u8; 0x3540]);
    }
    unreachable!()
}
impl Character {
    pub unsafe fn get_local_player_character() -> *mut crate::character::character::Character {
        unsafe {
            let f: unsafe extern "system" fn() -> *mut crate::character::character::Character = ::std::mem::transmute(
                0x143AD7B70 as usize,
            );
            f()
        }
    }
    pub unsafe fn get_head_position(
        &mut self,
        position: *mut crate::types::math::Vector3,
    ) -> *mut crate::types::math::Vector3 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                position: *mut crate::types::math::Vector3,
            ) -> *mut crate::types::math::Vector3 = ::std::mem::transmute(
                0x143AAE940 as usize,
            );
            f(self as *mut Self as _, position)
        }
    }
}
impl std::convert::AsRef<Character> for Character {
    fn as_ref(&self) -> &Character {
        self
    }
}
impl std::convert::AsMut<Character> for Character {
    fn as_mut(&mut self) -> &mut Character {
        self
    }
}
