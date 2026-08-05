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
    /// head world target. Full reach by default: with body IK on, the solver — not the
    /// `UpdatePropEffects` override — is what places the head, so anything less leaves the rendered
    /// head short of where the player's head actually is.
    pub head_reach_t: f32,
    /// The rotation-reach weight written to `m_TargetReachR[head]` (scaled by
    /// [`weight`](Self::weight)) when [`rotation_target`](Self::rotation_target) is set: how strongly
    /// the head is oriented toward the headpose forward.
    pub head_reach_r: f32,
    /// The pull weight written to `m_TargetPull[head]` (scaled by [`weight`](Self::weight)): how
    /// much reaching the head target propagates *down the chain* into the shoulders, chest, and
    /// spine rather than being absorbed by the neck alone. This is the knob that makes the upper
    /// body follow the head; with it at zero the solver reaches the target locally and the torso
    /// stays on the animated pose.
    pub head_pull: f32,
    /// The resistance weight written to `m_TargetResist[head]` (scaled by [`weight`](Self::weight))
    /// when [`rotation_target`](Self::rotation_target) is set: how much the head effector resists
    /// being displaced by other effectors' pull, so the game's own aim and hand targets do not drag
    /// it off the headpose.
    pub head_resist: f32,
    /// Also queue a rotation target that aims the head's model-space frame at the headpose
    /// orientation (in addition to the positional target). The `UpdatePropEffects` override sets the
    /// final head orientation regardless, so this mainly biases the spine/neck bend.
    pub rotation_target: bool,
    /// How much of the player's lean the torso takes: the share applied to the swing about the base
    /// of the spine that would carry the animated head to where the player's head actually is. `1.0`
    /// articulates the spine the whole way, leaving nothing for the neck; lower values split the
    /// lean between the two. Being a rotation about the spine's base, it handles a lean in any
    /// direction — sideways, forward, or a duck — from the one construction, and it can never
    /// telescope the spine the way a straight displacement would.
    pub lean_share: f32,
    /// Where the torso pivots, between the base of the spine (`0.0`) and the hips (`1.0`).
    ///
    /// No single point suits both directions. A sideways lean is genuinely spinal, and the spine's
    /// base is the joint that bends. A forward lean is mostly a hip hinge, and pivoting up in the
    /// back tips the chest without carrying the torso forward, which reads as the lean barely doing
    /// anything. Dropping the pivot trades angle for travel: the same head displacement resolves to
    /// a smaller rotation about a longer arm, so the shoulders move further.
    pub pivot_drop: f32,
    /// The maximum torso lean, in degrees — an anatomical limit on the spine's range, and the bound
    /// that keeps the lean model inside the region it is valid over. The swing's angle grows toward
    /// 180 degrees as the head approaches the height of the pivot, about an axis that becomes
    /// ill-conditioned there, so without a cap a head dropped to waist height would fold the torso
    /// over backwards in a near-arbitrary direction.
    pub lean_max_deg: f32,
    /// How much of the head's body-relative yaw the torso takes, as a rotation about the spine base's
    /// vertical axis. Composes with [`lean_share`](Self::lean_share) into a single torso rotation.
    pub yaw_share: f32,
    /// The translation-reach weight written to `m_TargetReachT[shoulder]` (scaled by
    /// [`weight`](Self::weight)) for each shoulder carried by the torso rotation.
    pub shoulder_reach_t: f32,
    /// The pull weight written to `m_TargetPull[shoulder]` (scaled by [`weight`](Self::weight)): how
    /// much the shoulders' displacement carries on down into the chest and spine.
    pub shoulder_pull: f32,
    /// Stand the torso targets and hand pins down while the character aims a weapon or the grapple
    /// (`Character::m_AimFlags`), leaving only the head target. In VR the body does not turn toward
    /// the aim, so the aim IK must swing the free arm chain to cover everything past the authored
    /// sweep; live pins and girdle targets clamp that swing to roughly a ±30° cone.
    pub defer_while_aiming: bool,
    /// Hold each hand at the position the animation gave it, by targeting the wrist effectors (3/4)
    /// at their own animated positions. Without it the arms simply ride the shoulders, so leaning
    /// slides the hands off whatever they were holding — a steering wheel is posed by the animation,
    /// not by a constraint the solver knows about. Pinned hands are queued with zero pull, so they
    /// hold the arms without dragging the torso back.
    pub pin_hands: bool,
    /// The translation-reach weight written to `m_TargetReachT[wrist]` (scaled by
    /// [`weight`](Self::weight)) for each hand held by [`pin_hands`](Self::pin_hands): how rigidly
    /// the hands keep their animated position as the shoulders move.
    pub hand_reach_t: f32,
    /// The rotation-reach weight written to `m_TargetReachR[wrist]` (scaled by
    /// [`weight`](Self::weight)) for each hand held by [`pin_hands`](Self::pin_hands): how rigidly
    /// the hands keep their animated *orientation*, as opposed to merely their position. Holding
    /// position alone lets the solver reach it from any wrist orientation, which spins the hands in
    /// place and takes the wielded weapon's aim with them.
    pub hand_reach_r: f32,
    /// The resistance weight written to `m_TargetResist[wrist]` (scaled by
    /// [`weight`](Self::weight)) for each pinned hand: how much the wrist refuses to be displaced by
    /// the shoulders' pull. Zero by default — the reach weights are the primary hold, and this is
    /// the knob to reach for if the hands still drift under a strong lean.
    pub hand_resist: f32,
    /// The rotation-reach weight written to `m_TargetReachR[chestEnd]` (scaled by
    /// [`weight`](Self::weight)), orienting the chest by the same torso rotation that places the
    /// shoulders.
    pub chest_reach_r: f32,
    /// A master multiplier on every reach, pull, and resist weight (`0..=1`), for tuning the overall
    /// IK strength with a single dial.
    pub weight: f32,
    /// Ease the reach weight in rather than snapping it (the `effector_interpolation` argument). The
    /// game's own hand pass uses `false`; with it on, the body eases into the pose over several
    /// frames.
    pub interpolation: bool,
    /// The reach-weight ease-in rate when [`interpolation`](Self::interpolation) is set (the game
    /// default is `3.0`).
    pub interpolation_rate: f32,
    /// Ease the reach weight back out when the target stops being supplied (the
    /// `effector_blend_out` argument). Off by default, against the game's hand-pass precedent: a
    /// blending-out entry keeps steering with the stale pose it was queued with, which is fine for
    /// the game's world-anchored targets but wrong a frame later for these animation-anchored ones.
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
            head_reach_t: 1.0,
            head_reach_r: 0.4,
            head_pull: 0.5,
            head_resist: 0.0,
            rotation_target: true,
            lean_share: 0.8,
            pivot_drop: 0.0,
            lean_max_deg: 45.0,
            yaw_share: 0.4,
            shoulder_reach_t: 0.9,
            shoulder_pull: 0.5,
            defer_while_aiming: true,
            pin_hands: true,
            hand_reach_t: 1.0,
            hand_reach_r: 1.0,
            hand_resist: 0.0,
            chest_reach_r: 0.8,
            weight: 1.0,
            interpolation: false,
            interpolation_rate: 3.0,
            blend_out: false,
            blend_out_rate: 1.5,
            target_offset: glam::Vec3::ZERO,
        }
    }
}
