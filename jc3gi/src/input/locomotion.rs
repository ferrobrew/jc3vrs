#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct InstanceProperties {}
impl InstanceProperties {}
impl std::convert::AsRef<InstanceProperties> for InstanceProperties {
    fn as_ref(&self) -> &InstanceProperties {
        self
    }
}
impl std::convert::AsMut<InstanceProperties> for InstanceProperties {
    fn as_mut(&mut self) -> &mut InstanceProperties {
        self
    }
}
#[repr(C, align(8))]
/// The locomotion task that reads the movement effectors and writes the move direction to the
/// character blackboard.
pub struct NStateTask_InputLocoSetTargetDirTask {}
impl NStateTask_InputLocoSetTargetDirTask {
    pub const SetupTargetDir_ADDRESS: usize = 0x14081E130;
    /// Reads the move effectors via the action-map effector lookup, rotates the raw vector
    /// camera-relative, and writes both the raw and camera-relative move directions to the character
    /// blackboard.
    pub unsafe fn SetupTargetDir(
        &mut self,
        character: *mut crate::character::character::Character,
        props: *mut crate::input::locomotion::InstanceProperties,
    ) -> f64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                character: *mut crate::character::character::Character,
                props: *mut crate::input::locomotion::InstanceProperties,
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
