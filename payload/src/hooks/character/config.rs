//! Character hook configuration: headset-driven upper-body IK.

use serde::{Deserialize, Serialize};

/// Headset-driven upper-body IK: drive the player's spine and head toward the headpose target by
/// feeding the engine's own HumanIK `MAIN` pass an effector target for the head bone, so the body
/// leans, ducks, and turns to follow where the player looks. Queued pre-solve in
/// [`crate::hooks::character`] (see `docs/engine/character/humanik.md`); the `UpdatePropEffects` head-bone override
/// still sets the exact head orientation on top of the HIK-bent spine.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct BodyIkConfig {
    /// Master switch: queue the head effector target each frame for the local player.
    pub enabled: bool,
    /// The translation-reach weight written to `m_TargetReachT[head]` (scaled by
    /// [`weight`](Self::weight)): how strongly the positional target pulls the upper body toward the
    /// head world target. `0.6` is strong but not rigid, leaving some of the animated pose.
    pub head_reach_t: f32,
    /// The rotation-reach weight written to `m_TargetReachR[head]` (scaled by
    /// [`weight`](Self::weight)) when [`rotation_target`](Self::rotation_target) is set: how strongly
    /// the head is oriented toward the headpose forward.
    pub head_reach_r: f32,
    /// Also queue a rotation target that aims the head's model-space frame at the headpose
    /// orientation (in addition to the positional target). The `UpdatePropEffects` override sets the
    /// final head orientation regardless, so this mainly biases the spine/neck bend.
    pub rotation_target: bool,
    /// A master multiplier on both reach weights (`0..=1`), for tuning the overall IK strength with a
    /// single dial.
    pub weight: f32,
    /// Ease the reach weight in rather than snapping it (the `effector_interpolation` argument). The
    /// game's own hand pass uses `false`; on eases the body into the pose over several frames.
    pub interpolation: bool,
    /// The reach-weight ease-in rate when [`interpolation`](Self::interpolation) is set (the game
    /// default is `3.0`).
    pub interpolation_rate: f32,
    /// Ease the reach weight back out when the target stops being supplied (the game default is
    /// `true`).
    pub blend_out: bool,
    /// The reach-weight ease-out rate (the game default is `1.5`).
    pub blend_out_rate: f32,
    /// An optional character-model-space offset added to the head target position, for tuning where
    /// the body reaches relative to the headpose point. Zero by default.
    pub target_offset: glam::Vec3,
}
impl BodyIkConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            head_reach_t: 0.6,
            head_reach_r: 0.4,
            rotation_target: true,
            weight: 1.0,
            interpolation: false,
            interpolation_rate: 3.0,
            blend_out: true,
            blend_out_rate: 1.5,
            target_offset: glam::Vec3::ZERO,
        }
    }
}
