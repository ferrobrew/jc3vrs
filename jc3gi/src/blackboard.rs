#![cfg_attr(any(), rustfmt::skip)]
#[allow(unused_imports)]
use crate::character::character::Character;
#[repr(C, align(8))]
/// A keyed value store attached to game objects (`CObjectBlackboard`); the character's instance
/// lives at [`Character::m_Blackboard`](Character::m_Blackboard) and carries
/// the locomotion values (the camera-relative move direction, the target face direction, the up
/// direction). Values are read and written by id through the template accessors below, under
/// [`m_BbLock`](ObjectBlackboard::m_BbLock).
pub struct ObjectBlackboard {
    /// The key table, [`m_KeyInfoCapacity`](ObjectBlackboard::m_KeyInfoCapacity) slots; keys are
    /// assigned as they are first written.
    pub m_KeyInfos: *mut crate::blackboard::ObjectBlackboardKeyInfo,
    pub m_KeyInfoCapacity: u16,
    _field_a: [u8; 6],
    /// The value arena; each entry lives at its key's
    /// [`m_DataOffset`](ObjectBlackboardKeyInfo::m_DataOffset).
    pub m_Data: *mut u8,
    pub m_DataCapacity: u16,
    /// The arena's bump-allocation cursor.
    pub m_CurrentOffset: u16,
    _field_1c: [u8; 4],
    /// The lock guarding every accessor (`SThreadMutex`, a one-pointer wrapper; the accessors
    /// enter the critical section directly).
    pub m_BbLock: *mut crate::graphics_engine::device::CRITICAL_SECTION,
    /// The dev dump cursor ("first line to dump"); unreferenced by the shipped code.
    pub m_FirstLineToDump: i32,
    _field_2c: [u8; 4],
}
fn _ObjectBlackboard_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], ObjectBlackboard>([0u8; 0x30]);
    }
    unreachable!()
}
impl ObjectBlackboard {
    pub const GetVector3_ADDRESS: usize = 0x1405C5AC0;
    /// The `GetBlackboardValue<CVector3f>` template instantiation. Returns whether the id was
    /// present; the value is written through `value`.
    pub unsafe fn GetVector3(
        &mut self,
        id: u32,
        value: *mut crate::types::math::Vector3,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                id: u32,
                value: *mut crate::types::math::Vector3,
            ) -> bool = ::std::mem::transmute(Self::GetVector3_ADDRESS);
            f(self as *mut Self as _, id, value)
        }
    }
    pub const GetFloat_ADDRESS: usize = 0x1405C5BA0;
    /// The `GetBlackboardValue<float>` template instantiation.
    pub unsafe fn GetFloat(&mut self, id: u32, value: *mut f32) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                id: u32,
                value: *mut f32,
            ) -> bool = ::std::mem::transmute(Self::GetFloat_ADDRESS);
            f(self as *mut Self as _, id, value)
        }
    }
    pub const SetVector3_ADDRESS: usize = 0x1405C6A70;
    /// The `SetBlackboardValue<CVector3f>` template instantiation. `value` is by-value `CVector3f`
    /// at the C++ level, which the MSVC x64 ABI passes by reference — hence the pointer here.
    /// `flags` is `1` at every observed call site; `name` is an optional debug label, null in
    /// release calls.
    pub unsafe fn SetVector3(
        &mut self,
        id: u32,
        value: *const crate::types::math::Vector3,
        flags: u32,
        name: *const u8,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                id: u32,
                value: *const crate::types::math::Vector3,
                flags: u32,
                name: *const u8,
            ) -> bool = ::std::mem::transmute(Self::SetVector3_ADDRESS);
            f(self as *mut Self as _, id, value, flags, name)
        }
    }
}
impl ObjectBlackboard {
    /// The id of the second float the input move task reads alongside
    /// [`SPEED_ID`](ObjectBlackboard::SPEED_ID); semantics unmapped, a candidate input-strength
    /// signal.
    pub const AUX_FLOAT_ID: u32 = 2217900102;
    /// The id of the constrained-ground movement direction (`CVector3f`), present on slopes (and
    /// other surface-constrained locomotion): when set, the movement task blends its displacement
    /// direction toward it instead of using the primary displacement output.
    pub const CONSTRAINED_DIR_ID: u32 = 2485695409;
    /// The id of the camera-relative world-space move direction (`CVector3f`), written each frame
    /// by `NStateTask_InputLocoSetTargetDirTask::SetupTargetDir`.
    pub const MOVE_DIR_ID: u32 = 2113030792;
    /// The id of the float speed value the input locomotion tasks branch on (`<= 0` routes into
    /// the stop acts).
    pub const SPEED_ID: u32 = 3396837917;
    /// The id of the target face direction (`CVector3f`): the desired body facing the orientation
    /// executor yaws toward in its tracking mode. Written per-state by the game's
    /// `SetUpTargetFaceDir` tasks.
    pub const TARGET_FACE_DIR_ID: u32 = 736589998;
}
impl std::convert::AsRef<ObjectBlackboard> for ObjectBlackboard {
    fn as_ref(&self) -> &ObjectBlackboard {
        self
    }
}
impl std::convert::AsMut<ObjectBlackboard> for ObjectBlackboard {
    fn as_mut(&mut self) -> &mut ObjectBlackboard {
        self
    }
}
#[repr(C, align(8))]
/// One key slot in an [`ObjectBlackboard`]'s table (`SObjectBlackboardKeyInfo`): the key hash, the
/// value's type tag and arena offset, and an optional debug label.
pub struct ObjectBlackboardKeyInfo {
    /// The key hash the accessors look up by.
    pub m_KeyHash: u32,
    /// The value's type tag (`SObjectBlackboardKeyInfo::EValueType`, stored narrow): `0` none,
    /// `1` float, `2` vector, `3` u32, `4` i32, `5` hash (aliases to u32 on read), `6` u64,
    /// `7` u8.
    pub m_ValueType: u8,
    _field_5: [u8; 3],
    /// The persistence class (`SObjectBlackboardKeyInfo::EPersistenceType`): `0` session,
    /// `1` local frame, `2` global frame.
    pub m_PersistenceType: u32,
    /// The value's byte offset within [`ObjectBlackboard::m_Data`].
    pub m_DataOffset: u16,
    _field_e: [u8; 2],
    /// An optional debug label for the key; null at release call sites.
    pub m_DebugName: *const u8,
}
fn _ObjectBlackboardKeyInfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], ObjectBlackboardKeyInfo>([0u8; 0x18]);
    }
    unreachable!()
}
impl ObjectBlackboardKeyInfo {}
impl std::convert::AsRef<ObjectBlackboardKeyInfo> for ObjectBlackboardKeyInfo {
    fn as_ref(&self) -> &ObjectBlackboardKeyInfo {
        self
    }
}
impl std::convert::AsMut<ObjectBlackboardKeyInfo> for ObjectBlackboardKeyInfo {
    fn as_mut(&mut self) -> &mut ObjectBlackboardKeyInfo {
        self
    }
}
