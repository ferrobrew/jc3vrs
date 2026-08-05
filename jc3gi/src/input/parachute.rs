#![cfg_attr(any(), rustfmt::skip)]
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// `SCharacterAirMovementSettings`: the shared tuning block for the airborne movement families
/// (parachute, wingsuit, and freefall). It covers the per-axis input/look yaw-pitch-roll mixing
/// (`m_XInputYawPitchRollAmount`, `m_XLookYawPitchRollAmount`), the heading-to-velocity springs
/// (`m_RotateCharYawToVelocity`), the camera-relative look steering (`m_LookSteer*`), the
/// slingshot boost (`m_Slingshot*`), and the near-ground lift. The parachute and wingsuit settings
/// embed one of these as their `m_AirControl`.
pub struct CharacterAirMovementSettings {
    pub m_Gravity: f32,
    pub m_MaxVelocity: f32,
    pub m_MaxVelocityXZ: f32,
    pub m_ClampVelocitySpeed: f32,
    pub m_Lift: [f32; 3],
    pub m_DragX: [f32; 3],
    pub m_DragY: [f32; 3],
    pub m_DragZ: [f32; 3],
    pub m_LiftPoint: [f32; 3],
    pub m_PitchLimit: [f32; 2],
    pub m_YawSteerAmount: f32,
    pub m_DrawDebugLines: u32,
    pub m_RotateCharYawToVelocity: crate::input::parachute::CharacterSpring,
    pub m_RotateCharPitchToVelocity: crate::input::parachute::CharacterSpring,
    pub m_PitchInputSpring: crate::input::parachute::CharacterSpring,
    pub m_YawInputSpring: crate::input::parachute::CharacterSpring,
    pub m_RollInputSpring: crate::input::parachute::CharacterSpring,
    pub m_XInputYawPitchRollAmount: [f32; 3],
    pub m_YInputYawPitchRollAmount: [f32; 3],
    pub m_XLookYawPitchRollAmount: [f32; 3],
    pub m_YLookYawPitchRollAmount: [f32; 3],
    pub m_RotateCharYawToSlingshot: crate::input::parachute::CharacterSpring,
    pub m_PersistYawPitchRoll: [f32; 3],
    pub m_CenterYawPitchRoll: [f32; 3],
    pub m_LiftInputSpring: crate::input::parachute::CharacterSpring,
    pub m_DragInputSpring: crate::input::parachute::CharacterSpring,
    pub m_AnimInputSpringX: crate::input::parachute::CharacterSpring,
    pub m_AnimInputSpringY: crate::input::parachute::CharacterSpring,
    pub m_NearGroundLiftDistance: f32,
    pub m_NearGroundMaxLift: f32,
    pub m_NearGroundLiftExponent: f32,
    pub m_NearGroundLiftDecayMax: f32,
    pub m_NearGroundLiftDecayMultiplier: f32,
    pub m_NearGroundLiftRecoverRate: f32,
    pub m_LookSteerDeadZone: [f32; 2],
    pub m_LookSteerMaxSpeed: [f32; 2],
    pub m_LookSteerMaxYaw: f32,
    pub m_LookSteerMaxPitch: [f32; 2],
    pub m_LookSteerExponential: [f32; 2],
    pub m_LookSteerAimingDeadZone: [f32; 2],
    pub m_LookSteerAimingMaxSpeed: [f32; 2],
    pub m_LookSteerAimingMaxYaw: f32,
    pub m_LookSteerAimingMaxPitch: [f32; 2],
    pub m_LookSteerAimingExponential: [f32; 2],
    pub m_SlingshotPitchAdjust: f32,
    pub m_SlingshotExtraLift: f32,
    pub m_SlingshotDirectAccel: f32,
    pub m_SlingshotForwardAccel: f32,
    pub m_SlingshotMaxSpeedAdjust: f32,
    pub m_SlingshotDragXYZAdjust: [f32; 3],
    pub m_SlingshotBreakPitch: f32,
    pub m_SlingshotBreakYawStart: f32,
    pub m_SlingshotBreakYawEnd: f32,
    pub m_SlingshotBreakYawBlendTime: f32,
    pub m_SlingshotBreakDistance: f32,
    pub m_RetractSlingshotSettings: crate::input::parachute::CharacterRetractSShotSettings,
}
fn _CharacterAirMovementSettings_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1C4], CharacterAirMovementSettings>([0u8; 0x1C4]);
    }
    unreachable!()
}
impl CharacterAirMovementSettings {}
impl std::convert::AsRef<CharacterAirMovementSettings> for CharacterAirMovementSettings {
    fn as_ref(&self) -> &CharacterAirMovementSettings {
        self
    }
}
impl std::convert::AsMut<CharacterAirMovementSettings> for CharacterAirMovementSettings {
    fn as_mut(&mut self) -> &mut CharacterAirMovementSettings {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// `SCharacterParachuteSettings`: the per-character parachute movement tuning block, embedded in
/// `SCustomMovementSettings` alongside the other families' settings. The first fields are the
/// angular motion limits and the reel-to-parachute transition; the rest is the shared
/// [`CharacterAirMovementSettings`](crate::input::parachute::CharacterAirMovementSettings) used for the flight dynamics and the camera-relative look
/// steering.
pub struct CharacterParachuteSettings {
    pub m_MaxAngularSpeed: f32,
    pub m_MaxAngularAccel: f32,
    pub m_VelocityAlignMinSpeed: f32,
    pub m_ParachuteToPivotDistance: f32,
    pub m_ReelToParaSettings: crate::input::parachute::CharacterReelToParaSettings,
    pub m_AirControl: crate::input::parachute::CharacterAirMovementSettings,
}
fn _CharacterParachuteSettings_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x228], CharacterParachuteSettings>([0u8; 0x228]);
    }
    unreachable!()
}
impl CharacterParachuteSettings {}
impl std::convert::AsRef<CharacterParachuteSettings> for CharacterParachuteSettings {
    fn as_ref(&self) -> &CharacterParachuteSettings {
        self
    }
}
impl std::convert::AsMut<CharacterParachuteSettings> for CharacterParachuteSettings {
    fn as_mut(&mut self) -> &mut CharacterParachuteSettings {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// `SCharacterReelToParaSettings`: the settings for the reel-in-to-parachute transition.
pub struct CharacterReelToParaSettings {
    pub m_ExtraLiftCutoff: f32,
    pub m_ExtraLift: [f32; 3],
    pub m_ExtraDownwardForce: [f32; 3],
    pub m_StopExtraLiftHeight: f32,
    pub m_DragX: [f32; 3],
    pub m_DragY: [f32; 3],
    pub m_DragZ: [f32; 3],
    pub m_UpwardVelocityClamp: f32,
    pub m_FakeSlingshotSpeedThreshold: f32,
    pub m_FakeSlingshotAccel: f32,
    pub m_FakeSlingshotMinPitch: f32,
}
fn _CharacterReelToParaSettings_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x54], CharacterReelToParaSettings>([0u8; 0x54]);
    }
    unreachable!()
}
impl CharacterReelToParaSettings {}
impl std::convert::AsRef<CharacterReelToParaSettings> for CharacterReelToParaSettings {
    fn as_ref(&self) -> &CharacterReelToParaSettings {
        self
    }
}
impl std::convert::AsMut<CharacterReelToParaSettings> for CharacterReelToParaSettings {
    fn as_mut(&mut self) -> &mut CharacterReelToParaSettings {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// `SCharacterRetractSShotSettings`: the settings for the parachute slingshot boost on reel-in
/// retract.
pub struct CharacterRetractSShotSettings {
    pub m_BlendInSpeed: f32,
    pub m_BlendOutSpeed: f32,
    pub m_MaxSpeedAdjust: f32,
    pub m_DirectAccel: f32,
    pub m_ForwardAccel: f32,
}
fn _CharacterRetractSShotSettings_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x14], CharacterRetractSShotSettings>([0u8; 0x14]);
    }
    unreachable!()
}
impl CharacterRetractSShotSettings {}
impl std::convert::AsRef<CharacterRetractSShotSettings>
for CharacterRetractSShotSettings {
    fn as_ref(&self) -> &CharacterRetractSShotSettings {
        self
    }
}
impl std::convert::AsMut<CharacterRetractSShotSettings>
for CharacterRetractSShotSettings {
    fn as_mut(&mut self) -> &mut CharacterRetractSShotSettings {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// `SCharacterSpring`: the per-axis spring tuning used throughout the air and parachute
/// movement settings. A spring is expressed with `m_Constant` (the stiffness), `m_Damping`, and
/// `m_Speed` (the per-second response); the air movement cores integrate the spring state per
/// frame from these.
pub struct CharacterSpring {
    pub m_Speed: f32,
    pub m_Constant: f32,
    pub m_Damping: f32,
}
fn _CharacterSpring_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xC], CharacterSpring>([0u8; 0xC]);
    }
    unreachable!()
}
impl CharacterSpring {}
impl std::convert::AsRef<CharacterSpring> for CharacterSpring {
    fn as_ref(&self) -> &CharacterSpring {
        self
    }
}
impl std::convert::AsMut<CharacterSpring> for CharacterSpring {
    fn as_mut(&mut self) -> &mut CharacterSpring {
        self
    }
}
#[repr(C, align(8))]
/// The input state task active while the character is parachuting. Delegates the per-frame
/// movement to [`UpdateParachutePhysics`](crate::input::parachute::UpdateParachutePhysics).
pub struct NStateTask_MovementParachuteTask {}
impl NStateTask_MovementParachuteTask {
    pub const Update_ADDRESS: usize = 0x14082AB10;
    /// The per-frame update. Reads the parachute movement effectors (pitch/yaw inputs via the
    /// action map), calls [`UpdateParachutePhysics`](crate::input::parachute::UpdateParachutePhysics) with the resulting input vector, then handles
    /// the cancel input that closes the parachute.
    pub unsafe fn Update(
        ctx: *mut crate::state::StateContext,
        p1: *mut ::std::ffi::c_void,
        p2: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                ctx: *mut crate::state::StateContext,
                p1: *mut ::std::ffi::c_void,
                p2: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::Update_ADDRESS);
            f(ctx, p1, p2)
        }
    }
}
impl std::convert::AsRef<NStateTask_MovementParachuteTask>
for NStateTask_MovementParachuteTask {
    fn as_ref(&self) -> &NStateTask_MovementParachuteTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_MovementParachuteTask>
for NStateTask_MovementParachuteTask {
    fn as_mut(&mut self) -> &mut NStateTask_MovementParachuteTask {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// `SParachuteActionParams`: the per-character parachute animation/physics state, embedded in
/// `CCharacter` alongside the other movement families' action params. The four proxy matrices
/// describe the parachute rig's pivot (the suspension point), the character, and the canopy, plus
/// the blended steering proxy that interpolates between them; the floats track the parachute
/// blend-in, bump timers, and the velocity-steering blend.
pub struct ParachuteActionParams {
    pub m_IsValid: bool,
    _field_1: [u8; 15],
    pub m_PivoProxy: crate::types::math::Matrix4,
    pub m_CharacterProxy: crate::types::math::Matrix4,
    pub m_ParachuteProxy: crate::types::math::Matrix4,
    pub m_SteeringProxy: crate::types::math::Matrix4,
    pub m_BlendOutPivotProxyCS: crate::types::math::Matrix4,
    pub m_OriginalReelDir: crate::types::math::Vector3,
    pub m_OriginalReelPos: crate::types::math::Vector3,
    pub m_OriginalReelTarget: crate::types::math::Vector3,
    pub m_PivotYOffset: f32,
    pub m_CharacterMaxSwing: f32,
    pub m_SlingshotTime: f32,
    pub m_SlinghotWeight: f32,
    pub m_ParachuteBlendIn: f32,
    pub m_ParachuteBumpTimer: f32,
    pub m_ParachuteBumpCooldownTimer: f32,
    pub m_VelocitySteeringBlendScale: f32,
    pub m_ParaFromAttachedResidueVelocity: crate::types::math::Vector3,
}
fn _ParachuteActionParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1A0], ParachuteActionParams>([0u8; 0x1A0]);
    }
    unreachable!()
}
impl ParachuteActionParams {}
impl std::convert::AsRef<ParachuteActionParams> for ParachuteActionParams {
    fn as_ref(&self) -> &ParachuteActionParams {
        self
    }
}
impl std::convert::AsMut<ParachuteActionParams> for ParachuteActionParams {
    fn as_mut(&mut self) -> &mut ParachuteActionParams {
        self
    }
}
pub const UpdateParachutePhysics_ADDRESS: usize = 0x1407E6DF0;
/// The parachute flight/steering core, called per frame by
/// [`NStateTask_MovementParachuteTask::Update`](crate::input::parachute::NStateTask_MovementParachuteTask::Update). Computes the parachute steering values, the
/// wanted character yaw/pitch/roll, and the camera-relative look steer via
/// [`GetParachuteSteeringValues`](crate::input::parachute::GetParachuteSteeringValues), then drives the parachute velocities and the character/
/// canopy orientation through the parachute action params' proxy matrices.
pub unsafe fn UpdateParachutePhysics(
    character: *mut crate::character::character::Character,
    dt: f32,
    input: *const crate::types::math::Vector2,
    para_settings: *const crate::input::parachute::CharacterParachuteSettings,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            character: *mut crate::character::character::Character,
            dt: f32,
            input: *const crate::types::math::Vector2,
            para_settings: *const crate::input::parachute::CharacterParachuteSettings,
        ) = ::std::mem::transmute(UpdateParachutePhysics_ADDRESS);
        f(character, dt, input, para_settings)
    }
}
pub const GetParachuteSteeringValues_ADDRESS: usize = 0x1407E6480;
/// The parachute steering helper, called from [`UpdateParachutePhysics`](crate::input::parachute::UpdateParachutePhysics). Computes the steering
/// values for the desired character yaw/pitch/roll from the input vector and the *camera input
/// matrix* (`GameCameraManager::GetInputMatrix`): the stick input is mixed through
/// `m_XInputYawPitchRollAmount`/`m_YInputYawPitchRollAmount`, while the camera-relative look is
/// shaped by `m_LookSteer*` (deadzone, max yaw, exponential) into `look_steer_out` and blended in.
/// Also computes the velocity-alignment steering toward the horizontal velocity.
pub unsafe fn GetParachuteSteeringValues(
    character: *mut crate::character::character::Character,
    para_settings: *const crate::input::parachute::CharacterParachuteSettings,
    input: *const crate::types::math::Vector2,
    dt: f32,
    steering_values_out: *mut crate::types::math::Vector3,
    wanted_char_yawpitchroll_out: *mut crate::types::math::Vector3,
    look_steer_out: *mut crate::types::math::Vector2,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            character: *mut crate::character::character::Character,
            para_settings: *const crate::input::parachute::CharacterParachuteSettings,
            input: *const crate::types::math::Vector2,
            dt: f32,
            steering_values_out: *mut crate::types::math::Vector3,
            wanted_char_yawpitchroll_out: *mut crate::types::math::Vector3,
            look_steer_out: *mut crate::types::math::Vector2,
        ) = ::std::mem::transmute(GetParachuteSteeringValues_ADDRESS);
        f(
            character,
            para_settings,
            input,
            dt,
            steering_values_out,
            wanted_char_yawpitchroll_out,
            look_steer_out,
        )
    }
}
