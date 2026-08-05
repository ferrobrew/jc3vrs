#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// `CControllerUtility`: static helpers that relate controller input, the camera, and character
/// orientation. Only the members needed so far are bound.
pub struct ControllerUtility {}
impl ControllerUtility {
    pub const GetDeltaAngleFromOrientation_ADDRESS: usize = 0x140781410;
    /// The signed XZ angle (degrees, -180..180) from the character's current world forward to
    /// `dir`. Read by the swim input tasks to pick between the forward crawl-start act and the
    /// directional 120-degree turn acts, by the AI turn/exit-vehicle behaviors, and by the
    /// locomotion aim-blend state's sampler weighting.
    pub unsafe fn GetDeltaAngleFromOrientation(
        character: *const crate::character::character::Character,
        dir: *const crate::types::math::Vector3,
    ) -> f32 {
        unsafe {
            let f: unsafe extern "system" fn(
                character: *const crate::character::character::Character,
                dir: *const crate::types::math::Vector3,
            ) -> f32 = ::std::mem::transmute(Self::GetDeltaAngleFromOrientation_ADDRESS);
            f(character, dir)
        }
    }
    pub const TransformInputDirToWorldDir_ADDRESS: usize = 0x140782430;
    /// Transforms a camera-relative direction (`input_dir`, the negated camera forward) and a
    /// camera-relative input vector (`input`, the stick's X and Y axes) into a world direction.
    /// `input.x` is the lateral (strafe) axis and `input.y` the forward/back axis. The general
    /// helper for turning controller input into a world direction: fifteen call sites across the
    /// locomotion, swim, jump, melee, grapple, aim, and AI steering paths.
    pub unsafe fn TransformInputDirToWorldDir(
        out: *mut crate::types::math::Vector3,
        input_dir: *const crate::types::math::Vector3,
        input: *const crate::types::math::Vector2,
    ) -> *mut crate::types::math::Vector3 {
        unsafe {
            let f: unsafe extern "system" fn(
                out: *mut crate::types::math::Vector3,
                input_dir: *const crate::types::math::Vector3,
                input: *const crate::types::math::Vector2,
            ) -> *mut crate::types::math::Vector3 = ::std::mem::transmute(
                Self::TransformInputDirToWorldDir_ADDRESS,
            );
            f(out, input_dir, input)
        }
    }
}
impl std::convert::AsRef<ControllerUtility> for ControllerUtility {
    fn as_ref(&self) -> &ControllerUtility {
        self
    }
}
impl std::convert::AsMut<ControllerUtility> for ControllerUtility {
    fn as_mut(&mut self) -> &mut ControllerUtility {
        self
    }
}
