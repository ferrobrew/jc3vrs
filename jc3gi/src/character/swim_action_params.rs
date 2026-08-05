#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(4))]
/// `SSwimActionParams`: the per-character state the swim locomotion family carries across frames,
/// embedded in [`Character`](crate::character::character::Character). Constructed by
/// `SSwimActionParams::SSwimActionParams` (0x140_779_BF0), which is where the fixed capsule offsets
/// below get their values. Only the members needed so far are bound; the rest of the struct
/// (momentum, transition and warping state, the surface-plane quaternions, the procedural anim
/// amounts) is covered by the minimum size.
pub struct SwimActionParams {
    /// The rigid transform from the character's orientation frame to the physics capsule frame
    /// while swimming underwater: `RotationX(-1.256636)` (about -72 degrees) with a small downward
    /// translation. The underwater movement core composes it onto the orientation it computes
    /// before handing the result to
    /// [`PfxCharacterInstance::SetOrientation`](crate::physics::PfxCharacterInstance::SetOrientation).
    pub m_UwSwimmingCapsuleOffset: crate::types::math::Matrix4,
    /// The inverse of [`m_UwSwimmingCapsuleOffset`](crate::character::swim_action_params::SwimActionParams::m_UwSwimmingCapsuleOffset).
    pub m_UwSwimmingCapsuleOffsetInv: crate::types::math::Matrix4,
    /// The capsule offset used while treading water at the surface: the **identity**. The surface
    /// movement core ([`UpdateSurfaceMovement`](crate::input::swim::UpdateSurfaceMovement)) selects this
    /// one when the stick is neutral or the wanted horizontal speed is at most 0.1, so a
    /// near-stationary surface swimmer's capsule frame and orientation frame coincide.
    pub m_SurfaceIdleSwimmingCapsuleOffset: crate::types::math::Matrix4,
    /// The inverse of
    /// [`m_SurfaceIdleSwimmingCapsuleOffset`](crate::character::swim_action_params::SwimActionParams::m_SurfaceIdleSwimmingCapsuleOffset).
    pub m_SurfaceIdleSwimmingCapsuleOffsetInv: crate::types::math::Matrix4,
    /// The capsule offset used while crawling at the surface: `RotationX(-1.5707951)`, exactly a
    /// quarter turn about X, laying the capsule flat for the prone crawl pose. The surface movement
    /// core selects this one whenever the wanted horizontal speed exceeds 0.1, so a moving surface
    /// swimmer's capsule frame is pitched 90 degrees away from its orientation frame -- the matrix
    /// reaching
    /// [`PfxCharacterInstance::SetOrientation`](crate::physics::PfxCharacterInstance::SetOrientation) is
    /// the capsule frame, not the facing frame, and a yaw/pitch/roll decomposition of it is at
    /// gimbal lock.
    pub m_SurfaceCrawlSwimmingCapsuleOffset: crate::types::math::Matrix4,
    /// The inverse of
    /// [`m_SurfaceCrawlSwimmingCapsuleOffset`](crate::character::swim_action_params::SwimActionParams::m_SurfaceCrawlSwimmingCapsuleOffset).
    pub m_SurfaceCrawlSwimmingCapsuleOffsetInv: crate::types::math::Matrix4,
    /// The inverse of whichever capsule offset the movement core selected this frame, written just
    /// before it calls
    /// [`PfxCharacterInstance::SetOrientation`](crate::physics::PfxCharacterInstance::SetOrientation).
    /// Recovers the orientation frame from the capsule frame.
    pub m_CurrentSwimmingCapsuleOffsetInv: crate::types::math::Matrix4,
    _field_1c0: [u8; 96],
    /// The world-space ground-plane direction the animated turn starts from, latched when a turn
    /// act is dispatched (or, for the grapple and planted-explosive cases, re-latched from the
    /// character's current forward as the act begins).
    pub m_AnimatedTurnBaseDir: crate::types::math::Vector3,
    /// The world-space ground-plane direction the animated turn ends at. The movement cores slerp
    /// from [`m_AnimatedTurnBaseDir`](crate::character::swim_action_params::SwimActionParams::m_AnimatedTurnBaseDir) to this by the local time of the
    /// angle-correction animation segment, which is what makes a swim turn advance in act-sized
    /// steps rather than continuously.
    pub m_AnimatedTurnTargetDir: crate::types::math::Vector3,
    _field_238: [u8; 68],
}
fn _SwimActionParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x27C], SwimActionParams>([0u8; 0x27C]);
    }
    unreachable!()
}
impl SwimActionParams {}
impl std::convert::AsRef<SwimActionParams> for SwimActionParams {
    fn as_ref(&self) -> &SwimActionParams {
        self
    }
}
impl std::convert::AsMut<SwimActionParams> for SwimActionParams {
    fn as_mut(&mut self) -> &mut SwimActionParams {
        self
    }
}
