#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct NStateTask_InputLocoSetTargetDirTask {}
impl NStateTask_InputLocoSetTargetDirTask {
    pub const SetupTargetDir_ADDRESS: usize = 0x14081E130;
    /// Read the move effectors (action IDs 28-31) via the action-map GetEffector, rotate the raw
    /// vector camera-relative, and write the raw and camera-relative move directions to the
    /// character blackboard.
    pub unsafe fn SetupTargetDir(
        &mut self,
        character: *mut crate::character::character::Character,
        props: *mut crate::input::locomotion::SInstanceProperties,
    ) -> f64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                character: *mut crate::character::character::Character,
                props: *mut crate::input::locomotion::SInstanceProperties,
            ) -> f64 = ::std::mem::transmute(Self::SetupTargetDir_ADDRESS);
            f(self as *mut Self as _, character, props)
        }
    }
}
impl std::convert::AsRef<NStateTask_InputLocoSetTargetDirTask>
for NStateTask_InputLocoSetTargetDirTask {
    fn as_ref(&self) -> &NStateTask_InputLocoSetTargetDirTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_InputLocoSetTargetDirTask>
for NStateTask_InputLocoSetTargetDirTask {
    fn as_mut(&mut self) -> &mut NStateTask_InputLocoSetTargetDirTask {
        self
    }
}
#[repr(C, align(8))]
pub struct SInstanceProperties {}
impl SInstanceProperties {}
impl std::convert::AsRef<SInstanceProperties> for SInstanceProperties {
    fn as_ref(&self) -> &SInstanceProperties {
        self
    }
}
impl std::convert::AsMut<SInstanceProperties> for SInstanceProperties {
    fn as_mut(&mut self) -> &mut SInstanceProperties {
        self
    }
}
