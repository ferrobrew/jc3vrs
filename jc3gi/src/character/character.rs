#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct Character {
    _field_0: [u8; 9770],
    pub m_IsLocalCharacter: bool,
    _field_262b: [u8; 453],
    pub m_WorldMatrixT0: crate::types::math::Matrix4,
    pub m_WorldMatrixT1: crate::types::math::Matrix4,
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
    pub unsafe fn get_safe_bone_matrix(
        &mut self,
        safe_index: crate::character::character::SafeBoneIndex,
        matrix: *mut crate::types::math::Matrix4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                safe_index: crate::character::character::SafeBoneIndex,
                matrix: *mut crate::types::math::Matrix4,
            ) = ::std::mem::transmute(0x143A991B0 as usize);
            f(self as *mut Self as _, safe_index, matrix)
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
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum SafeBoneIndex {
    REFERENCE = 2394094972isize as _,
    OFFSET = 1252689883isize as _,
    HIPS = 1757849759isize as _,
    LEFT_FOOT = 1712403628isize as _,
    RIGHT_FOOT = 4282253387isize as _,
    LEFT_TOE = 3005147562isize as _,
    RIGHT_TOE = 3710742389isize as _,
    LEFT_HAND = 1472741269isize as _,
    RIGHT_HAND = 1776779174isize as _,
    HEAD = 2826426828isize as _,
    NECK = 2714329432isize as _,
    SPINE = 237553739isize as _,
    SPINE1 = 3839615855isize as _,
    SPINE2 = 1877494024isize as _,
    STERNUM = 2647308479isize as _,
    LEFT_SHOULDER = 2268405885isize as _,
    RIGHT_SHOULDER = 808382080isize as _,
    BACK_ATTACH = 227257561isize as _,
    BACK_ATTACH_2 = 3083906404isize as _,
    EQUIPPED_EXPLOSIVE = 2808756785isize as _,
    LEFT_HOLSTER = 1672209727isize as _,
    RIGHT_HOLSTER = 2077750035isize as _,
    ATTACH_HAND_RIGHT = 1707463403isize as _,
    ATTACH_HAND_LEFT = 1100005367isize as _,
    ATTACH_HAND_RIGHT2 = 2079854409isize as _,
    ATTACH_HAND_LEFT2 = 1576246544isize as _,
    RIGHT_HAND_IK_TARGET = 4159613865isize as _,
    LEFT_HAND_IK_TARGET = 2805860545isize as _,
    RIGHT_FOOT_IK_TARGET = 1012388403isize as _,
    LEFT_FOOT_IK_TARGET = 2290357383isize as _,
    AIM_TARGET = 2541339648isize as _,
    TARGET_REF_1 = 3212017811isize as _,
    TARGET_REF_2 = 994268807isize as _,
    RIGHT_LEG = 2828697949isize as _,
    LEFT_LEG = 2016147705isize as _,
    RIGHT_UP_LEG = 2401446677isize as _,
    LEFT_UP_LEG = 641280962isize as _,
    RIGHT_ARM = 433370831isize as _,
    LEFT_ARM = 1307615921isize as _,
    GRAPPLE_ATTACH = 4033878745isize as _,
    GRAPPLE_SHOULDER_ATTACH = 1040341121isize as _,
    GRAPPLE_DEVICE_ATTACH_BONE = 2618472340isize as _,
    CAMERA = 3048058670isize as _,
    NORMAL_MAP0 = 4017403211isize as _,
    NORMAL_MAP1 = 2614705093isize as _,
    NORMAL_MAP2 = 4032476450isize as _,
    NORMAL_MAP3 = 333548isize as _,
    NORMAL_MAP4 = 2094004130isize as _,
    NORMAL_MAP5 = 205856033isize as _,
    BONE_AMOUNT = 4294967295isize as _,
}
fn _SafeBoneIndex_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], SafeBoneIndex>([0u8; 0x4]);
    }
    unreachable!()
}
pub unsafe fn get_Character_GoreEnabled() -> &'static mut bool {
    unsafe { &mut *(0x142F2F301 as *mut bool) }
}
