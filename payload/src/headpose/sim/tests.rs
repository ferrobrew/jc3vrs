use std::f32::consts::{FRAC_PI_2, PI, TAU};

use super::*;
use crate::headpose::HeadPoseConfig;

fn test_config() -> HeadPoseConfig {
    HeadPoseConfig::new()
}

#[test]
fn test_latch_engages_at_threshold() {
    let config = test_config();
    let latch = update_latch(
        LatchState::Decoupled,
        config.latch_threshold_deg,
        HeadMode::OnFoot,
        &config,
    );
    assert_eq!(latch, LatchState::BodyFollowing);
}

#[test]
fn test_latch_does_not_engage_below_threshold() {
    let config = test_config();
    let latch = update_latch(
        LatchState::Decoupled,
        config.latch_threshold_deg - 1.0,
        HeadMode::OnFoot,
        &config,
    );
    assert_eq!(latch, LatchState::Decoupled);
}

#[test]
fn test_latch_engages_at_negative_threshold() {
    let config = test_config();
    let latch = update_latch(
        LatchState::Decoupled,
        -config.latch_threshold_deg,
        HeadMode::OnFoot,
        &config,
    );
    assert_eq!(latch, LatchState::BodyFollowing);
}

#[test]
fn test_latch_disengages_below_hysteresis() {
    let config = test_config();
    let latch = update_latch(
        LatchState::BodyFollowing,
        config.latch_disengage_threshold_deg,
        HeadMode::OnFoot,
        &config,
    );
    assert_eq!(latch, LatchState::Decoupled);
}

#[test]
fn test_latch_hysteresis_prevents_jitter() {
    let config = test_config();
    // Between disengage and engage thresholds: should stay in current state.
    let mid = (config.latch_disengage_threshold_deg + config.latch_threshold_deg) / 2.0;
    let latch = update_latch(LatchState::BodyFollowing, mid, HeadMode::OnFoot, &config);
    assert_eq!(latch, LatchState::BodyFollowing);
    let latch = update_latch(LatchState::Decoupled, mid, HeadMode::OnFoot, &config);
    assert_eq!(latch, LatchState::Decoupled);
}

#[test]
fn test_latch_always_decoupled_in_other_mode() {
    let config = test_config();
    let latch = update_latch(
        LatchState::BodyFollowing,
        config.latch_threshold_deg + 10.0,
        HeadMode::Other,
        &config,
    );
    assert_eq!(latch, LatchState::Decoupled);
}

#[test]
fn test_free_look_clamping_yaw() {
    let config = test_config();
    let limit = config.free_look_yaw_limit_deg.to_radians();
    let clamped = (limit + 1.0).clamp(-limit, limit);
    assert_eq!(clamped, limit);
    let clamped = (-limit - 1.0).clamp(-limit, limit);
    assert_eq!(clamped, -limit);
}

#[test]
fn test_free_look_clamping_pitch() {
    let config = test_config();
    let limit = config.free_look_pitch_limit_deg.to_radians();
    let clamped = (limit + 1.0).clamp(-limit, limit);
    assert_eq!(clamped, limit);
    let clamped = (-limit - 1.0).clamp(-limit, limit);
    assert_eq!(clamped, -limit);
}

#[test]
fn test_mode_detection_from_counter() {
    // Counter advanced: the orientation evaluator ran, so the player is on foot.
    assert_eq!(detect_mode(0, 1), HeadMode::OnFoot);
    assert_eq!(detect_mode(5, 8), HeadMode::OnFoot);
    // Counter unchanged: not on foot.
    assert_eq!(detect_mode(5, 5), HeadMode::Other);
}

#[test]
fn test_wrap_angle() {
    assert!((wrap_angle(0.0)).abs() < 1e-6);
    assert!((wrap_angle(PI + 0.5) - (-PI + 0.5)).abs() < 1e-5);
    assert!((wrap_angle(-PI - 0.5) - (PI - 0.5)).abs() < 1e-5);
    assert!((wrap_angle(TAU + 0.25) - 0.25).abs() < 1e-5);
    assert!((wrap_angle(-0.25) - (-0.25)).abs() < 1e-6);
}

#[test]
fn test_yaw_forward_convention() {
    // At zero yaw, the forward is -Z.
    assert!((yaw_forward(0.0) - glam::Vec3::NEG_Z).length() < 1e-5);
    // A quarter turn about +Y rotates -Z to -X.
    assert!((yaw_forward(FRAC_PI_2) - glam::Vec3::NEG_X).length() < 1e-5);
    // The forward is always unit length on the ground plane.
    let forward = yaw_forward(0.5);
    assert!((forward.length() - 1.0).abs() < 1e-5);
    assert!(forward.y.abs() < 1e-6);
}

#[test]
fn test_body_turn_compensation_keeps_head_world_anchored() {
    // The on-foot compensation: when the body turns by delta, the body-relative yaw shifts by
    // -delta, keeping the world yaw (body + relative) constant.
    let body_then = 0.3_f32;
    let body_now = 0.8_f32;
    let relative = 1.0_f32;
    let compensated = wrap_angle(relative - wrap_angle(body_now - body_then));
    assert!(((body_now + compensated) - (body_then + relative)).abs() < 1e-5);
}

#[test]
fn test_posture_swing_upright_is_identity() {
    let q = posture_swing(glam::Vec3::Y, 25.0, 60.0);
    assert!(q.angle_between(glam::Quat::IDENTITY) < 1e-5);
}

#[test]
fn test_posture_swing_deadband_ignores_small_lean() {
    // A 20° lean is inside the 25° deadband.
    let up = glam::Quat::from_rotation_z(20.0_f32.to_radians()) * glam::Vec3::Y;
    let q = posture_swing(up, 25.0, 60.0);
    assert!(q.angle_between(glam::Quat::IDENTITY) < 1e-5);
}

#[test]
fn test_posture_swing_full_inversion_is_pitch_flip() {
    let q = posture_swing(-glam::Vec3::Y, 25.0, 60.0);
    // Fully inverted: 180° about body X.
    assert!((q.angle_between(glam::Quat::IDENTITY) - PI).abs() < 1e-4);
    let rotated = q * glam::Vec3::Y;
    assert!((rotated - (-glam::Vec3::Y)).length() < 1e-4);
}

#[test]
fn test_posture_swing_past_full_maps_up_exactly() {
    // A 90° sideways lean is past the 60° full-engagement point: the swing must take body-up
    // exactly onto the measured axis.
    let up = glam::Vec3::X;
    let q = posture_swing(up, 25.0, 60.0);
    assert!((q * glam::Vec3::Y - up).length() < 1e-4);
}

#[test]
fn test_posture_swing_ramp_is_partial() {
    // 40° deviation with deadband 25 and full 60: engagement is partial, so the swing is strictly
    // between identity and the full deviation.
    let up = glam::Quat::from_rotation_z(40.0_f32.to_radians()) * glam::Vec3::Y;
    let q = posture_swing(up, 25.0, 60.0);
    let angle = q.angle_between(glam::Quat::IDENTITY);
    assert!(angle > 1e-3);
    assert!(angle < 40.0_f32.to_radians());
}

#[test]
fn test_posture_swing_zero_vector_is_identity() {
    let q = posture_swing(glam::Vec3::ZERO, 25.0, 60.0);
    assert!(q.angle_between(glam::Quat::IDENTITY) < 1e-5);
}
