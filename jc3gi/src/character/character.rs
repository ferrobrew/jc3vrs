#![cfg_attr(any(), rustfmt::skip)]
crate::__bitflags! {
    #[doc = " The character's aiming state, written each frame by"] #[doc =
    " [`HandleAimingInputPlayer`](Character::HandleAimingInputPlayer) for the player. The locomotion"]
    #[doc =
    " state machine reads it to choose aim-relative (strafe) movement over run/steer:"]
    #[doc =
    " `m_AimingWeapon` (or `m_AimingGrapple`) is the switch that makes the body face the aim direction"]
    #[doc =
    " and turns the directional keys into strafes. `m_WasAiming` reflects the state as of the start of"]
    #[doc = " this frame. Bits `0x01`/`0x02` and `0x40`/`0x80` are unmapped."] pub struct
    AimState : u8 { const m_AimingEnabled = 4usize as _; const m_AimingWeapon = 8usize as
    _; const m_AimingGrapple = 16usize as _; const m_WasAiming = 32usize as _; }
}
fn _AimState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1], AimState>([0u8; 0x1]);
    }
    unreachable!()
}
#[repr(C, align(8))]
pub struct AnimatedModel {
    _field_0: [u8; 376],
    pub m_AnimationController: *mut crate::character::character::AnimationController,
    _field_180: [u8; 192],
}
fn _AnimatedModel_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x240], AnimatedModel>([0u8; 0x240]);
    }
    unreachable!()
}
impl AnimatedModel {}
impl std::convert::AsRef<AnimatedModel> for AnimatedModel {
    fn as_ref(&self) -> &AnimatedModel {
        self
    }
}
impl std::convert::AsMut<AnimatedModel> for AnimatedModel {
    fn as_mut(&mut self) -> &mut AnimatedModel {
        self
    }
}
#[repr(C, align(8))]
pub struct AnimationController {}
impl AnimationController {
    pub const GetBoneIndex_ADDRESS: usize = 0x140434E30;
    pub unsafe fn GetBoneIndex(&self, hash: u32) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self, hash: u32) -> u32 = ::std::mem::transmute(
                Self::GetBoneIndex_ADDRESS,
            );
            f(self as *const Self as _, hash)
        }
    }
    pub const GetBoneMatrix_ADDRESS: usize = 0x14043FE70;
    pub unsafe fn GetBoneMatrix(
        &self,
        index: u32,
        matrix: *mut crate::types::math::Matrix4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                index: u32,
                matrix: *mut crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::GetBoneMatrix_ADDRESS);
            f(self as *const Self as _, index, matrix)
        }
    }
    pub const GetJoint_ADDRESS: usize = 0x14043FF90;
    pub unsafe fn GetJoint(
        &self,
        index: u32,
        joint: *mut crate::character::character::Joint,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                index: u32,
                joint: *mut crate::character::character::Joint,
            ) = ::std::mem::transmute(Self::GetJoint_ADDRESS);
            f(self as *const Self as _, index, joint)
        }
    }
    pub const SetJoint_ADDRESS: usize = 0x14043FFF0;
    pub unsafe fn SetJoint(
        &mut self,
        index: u32,
        joint: *mut crate::character::character::Joint,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                index: u32,
                joint: *mut crate::character::character::Joint,
            ) = ::std::mem::transmute(Self::SetJoint_ADDRESS);
            f(self as *mut Self as _, index, joint)
        }
    }
}
impl std::convert::AsRef<AnimationController> for AnimationController {
    fn as_ref(&self) -> &AnimationController {
        self
    }
}
impl std::convert::AsMut<AnimationController> for AnimationController {
    fn as_mut(&mut self) -> &mut AnimationController {
        self
    }
}
#[repr(C, align(8))]
pub struct Character {
    _field_0: [u8; 6016],
    pub m_AnimatedModel: crate::character::character::AnimatedModel,
    _field_19c0: [u8; 1841],
    /// Packed aiming-state bit-flags; see [`AimState`].
    pub m_AimFlags: crate::character::character::AimState,
    _field_20f2: [u8; 1336],
    pub m_IsLocalCharacter: bool,
    _field_262b: [u8; 453],
    pub m_WorldMatrixT0: crate::types::math::Matrix4,
    pub m_WorldMatrixT1: crate::types::math::Matrix4,
    _field_2870: [u8; 400],
    /// The weapon-aim countdown timer, in seconds. It is refreshed while an aim input is held and
    /// decremented by the frame delta otherwise; a positive value keeps
    /// [`m_AimingWeapon`](AimState::m_AimingWeapon) set in [`m_AimFlags`](Character::m_AimFlags),
    /// so the character stays in aim/strafe locomotion for a short tail after the aim button is
    /// released.
    pub m_AimTimer: f32,
    _field_2a04: [u8; 2876],
}
fn _Character_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x3540], Character>([0u8; 0x3540]);
    }
    unreachable!()
}
impl Character {
    pub const GetLocalPlayerCharacter_ADDRESS: usize = 0x1407D5B00;
    pub unsafe fn GetLocalPlayerCharacter() -> *mut crate::character::character::Character {
        unsafe {
            let f: unsafe extern "system" fn() -> *mut crate::character::character::Character = ::std::mem::transmute(
                Self::GetLocalPlayerCharacter_ADDRESS,
            );
            f()
        }
    }
    pub const GetHeadPosition_ADDRESS: usize = 0x1407AF550;
    pub unsafe fn GetHeadPosition(
        &self,
        position: *mut crate::types::math::Vector3,
    ) -> *mut crate::types::math::Vector3 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                position: *mut crate::types::math::Vector3,
            ) -> *mut crate::types::math::Vector3 = ::std::mem::transmute(
                Self::GetHeadPosition_ADDRESS,
            );
            f(self as *const Self as _, position)
        }
    }
    pub const GetSafeIndex_ADDRESS: usize = 0x14079AB30;
    pub unsafe fn GetSafeIndex(
        &self,
        safe_index: crate::character::character::SafeBoneIndex,
    ) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                safe_index: crate::character::character::SafeBoneIndex,
            ) -> u32 = ::std::mem::transmute(Self::GetSafeIndex_ADDRESS);
            f(self as *const Self as _, safe_index)
        }
    }
    pub const GetSafeBoneMatrix_ADDRESS: usize = 0x14079AC30;
    pub unsafe fn GetSafeBoneMatrix(
        &self,
        safe_index: crate::character::character::SafeBoneIndex,
        matrix: *mut crate::types::math::Matrix4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                safe_index: crate::character::character::SafeBoneIndex,
                matrix: *mut crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::GetSafeBoneMatrix_ADDRESS);
            f(self as *const Self as _, safe_index, matrix)
        }
    }
    pub const UpdatePropEffects_ADDRESS: usize = 0x1407C2380;
    /// The per-frame update of the character's attached prop visual effects.
    pub unsafe fn UpdatePropEffects(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::UpdatePropEffects_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const HandleAimingInputPlayer_ADDRESS: usize = 0x1407F0530;
    /// The per-frame player aiming-input update. Resolves whether the player is aiming a weapon or
    /// grapple from the equipped weapon and [`m_AimTimer`](Character::m_AimTimer), then writes the
    /// result into [`m_AimFlags`](Character::m_AimFlags)
    /// ([`m_AimingWeapon`](AimState::m_AimingWeapon) / [`m_AimingGrapple`](AimState::m_AimingGrapple)).
    /// The locomotion tasks read those flags to select aim-relative (strafe) movement. NPCs use a
    /// separate `HandleAimingInputNPC`; this variant is player-only and is the single point that
    /// decides the on-foot aim/strafe state.
    pub unsafe fn HandleAimingInputPlayer(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::HandleAimingInputPlayer_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const IsAimingWeaponPossible_ADDRESS: usize = 0x1407F2B90;
    /// Whether the character is currently permitted to aim a weapon (an aimable weapon is equipped
    /// and the character state allows it). Gates the aim path in the locomotion tasks.
    pub unsafe fn IsAimingWeaponPossible(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsAimingWeaponPossible_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsPlayer_ADDRESS: usize = 0x14075FA60;
    /// Whether the character is the local player. Used by the locomotion queue helpers to restrict
    /// the aim-relative movement path to the player.
    pub unsafe fn IsPlayer(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsPlayer_ADDRESS,
            );
            f(self as *const Self as _)
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
#[derive(Copy, Clone, Default)]
#[repr(C, align(16))]
pub struct Joint {
    pub m_Translation: crate::types::vector_math::AlignedVector3,
    pub m_Orientation: crate::types::vector_math::AlignedQuat,
    pub m_Scale: crate::types::vector_math::AlignedVector3,
}
fn _Joint_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], Joint>([0u8; 0x30]);
    }
    unreachable!()
}
impl Joint {}
impl std::convert::AsRef<Joint> for Joint {
    fn as_ref(&self) -> &Joint {
        self
    }
}
impl std::convert::AsMut<Joint> for Joint {
    fn as_mut(&mut self) -> &mut Joint {
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
