//! Runtime configuration: every user-facing toggle, consolidated into one mutex-guarded struct with
//! sub-structs by concern. The debug UI reads/writes the whole struct; hooks copy out the field(s)
//! they need at the top of a detour. Live engine-interface state (the current eye, frame counters,
//! the trace arm-flag) does NOT live here -- see [`crate::stereo::StereoState`] and the per-subsystem
//! runtime statics.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{headpose::HeadPoseConfig, hud::HudConfig};

/// The global runtime configuration. Cheap to lock (uncontended `parking_lot::Mutex`); read it at the
/// top of a hook and release before doing engine work.
pub static CONFIG: Mutex<Config> = Mutex::new(Config::new());

/// Snapshot the whole config (for the trace manifest / bulk UI reads).
pub fn get() -> Config {
    CONFIG.lock().clone()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub stereo: StereoConfig,
    pub exposure: ExposureConfig,
    pub post_fx: PostFxConfig,
    pub camera: CameraConfig,
    pub movement: MovementConfig,
    pub fsr: FsrConfig,
    pub hud: HudConfig,
    pub headpose: HeadPoseConfig,
}
impl Config {
    pub const fn new() -> Self {
        Self {
            stereo: StereoConfig::new(),
            exposure: ExposureConfig::new(),
            post_fx: PostFxConfig::new(),
            camera: CameraConfig::new(),
            movement: MovementConfig::new(),
            fsr: FsrConfig::new(),
            hud: HudConfig::new(),
            headpose: HeadPoseConfig::new(),
        }
    }

    /// Lock the global config, run `f` against it, and return the result -- the terse read path for
    /// hooks: `Config::lock_query(|c| c.post_fx.skip_sun_halo)`. The lock is held only for `f`.
    pub fn lock_query<R>(f: impl FnOnce(&Config) -> R) -> R {
        f(&CONFIG.lock())
    }
}

/// Stereo rendering toggles. The live per-eye runtime state is [`crate::stereo::StereoState`].
#[derive(Clone, Serialize, Deserialize)]
pub struct StereoConfig {
    /// Master switch: render the scene twice, once per eye.
    pub enabled: bool,
    /// Apply the per-eye IPD camera offset.
    pub cameras: bool,
    /// Interpupillary distance, in metres.
    pub ipd: f32,
    /// Force SMAA 1x in stereo (T2X's shared history ghosts across the two eye dispatches).
    pub force_smaa_1x: bool,
    /// Force the SSAO pass into its "first pass" state before each stereo eye, so each eye computes AO
    /// fresh from its own depth instead of blending against the other eye's history. The SSAO history
    /// index advances once per dispatch (inside CRenderBlockSSAO::Draw), so without this a stereo
    /// render double-steps it and the two eyes compound. Kept on by default.
    pub force_ssao_first_pass: bool,
    /// Which eye reaches the screen (debug A/B).
    pub present_eye_0: bool,
    /// Restore the TAA-jitter / shadow-phase counters between eyes.
    pub restore_frame_counters: bool,
    /// Skip SetupRenderFrameData on eye 1 (experimental; normally inert).
    pub gate_setup_render_frame_data: bool,
    /// Skip HandBackBuffers on eye 1.
    pub gate_hand_back_buffers: bool,
    /// Zero the post-effect dt on eye 1 (so once-per-frame accumulators do not double-step).
    pub gate_eye1_dt: bool,
    /// Drain the engine's draw-dispatch CPU fragment (`GraphicsEngine+0x30`, `m_DrawThreadWorkSignal`)
    /// after each eye's `Draw`, which `WaitForCPUDrawToFinish` does not. `DispatchDraw` kicks that
    /// fragment to run the render passes asynchronously, and the engine only waits on it at the *next*
    /// `Draw`'s entry -- so without this, eye 0's fragment is still in flight when the between-eye
    /// snapshot/restore mutates the shared render-frame state, and the fragment reads a torn per-camera
    /// context (wild `this`) and faults. The fix for the intermittent open-world crash, IDB-verified
    /// (the barrier address is disassembled from the engine's own entry wait). Default on: a wrong
    /// barrier fails on frame 1, which is the wanted behaviour during development -- crash fast and
    /// deterministically rather than mask a latent fault. Toggle off to reproduce the original crash for
    /// an A/B.
    pub drain_draw_fragment: bool,
    /// Correct the sun-shadow cascade anchor per eye. The cascaded shadow map is fit to the shared
    /// center camera, but the material shaders anchor the cascade lookup at the *per-eye* camera
    /// position (`cb0[4]`), so each eye's shadow is shifted by `M * (eyePos - centerPos)` -- the visible
    /// per-eye sun-shadow mismatch (edge/length/strength differing between eyes, only with disparity).
    /// This adds `M * delta` to the cascade transform translation to re-anchor the lookup at center. The
    /// directly visible stereo-shadow fix; A/B by flipping `present_eye_0` with it on/off.
    pub fix_shadow_cascade_anchor: bool,
    /// Diagnostic: hash a curated set of engine render targets after each eye's Draw and record the
    /// per-eye hashes into the active render trace. Run with `cameras` off (both eyes share one
    /// camera) so any RT whose two eyes' hashes differ is being accumulated across the two Draws --
    /// the "stronger in one eye" bug. See [`crate::debug::rt_hash`].
    pub diagnose_rt_hashes: bool,
    /// Diagnostic: skip the SSAO pass on both eyes in stereo, to confirm whether SSAO drives the
    /// "stronger in one eye" darkening. (Equivalent to lowering the in-game AO setting, but toggleable
    /// live.)
    pub disable_ssao: bool,
    /// Experiment: skip the SSAO pass on the second eye only, so the first eye's screen AO is absent
    /// from the second. A crude test of whether the AO asymmetry is the artifact (a real shared-AO fix
    /// needs reprojection, not omission).
    pub ssao_eye0_only: bool,
    /// Diagnostic: restore the `RenderEngine` per-Draw constant-buffer ring index (`+0x16C0`) between
    /// the two stereo eyes. This ring advances once per `Draw` and is *not* one of the engine frame
    /// counters [`restore_frame_counters`](Self::restore_frame_counters) rewinds, so the two eyes
    /// otherwise land on different constant-buffer pool slots -- any pass that reads the previous slot
    /// (reprojection / previous-frame matrices) then sees different data per eye. Test whether pinning
    /// it converges the per-eye MainColor.
    pub restore_cb_ring: bool,
    /// Diagnostic: skip the screen-space reflections pass (`RP_SCREEN_SPACE_REFLECTIONS`) on both eyes.
    /// SSR reads a previous-frame scene-color capture that is regenerated every `Draw`, so eye 1 reads
    /// what eye 0 just wrote -- a content-based per-eye divergence no counter restore can fix. If
    /// dropping SSR converges the per-eye MainColor, the SSR feedback is the source.
    pub skip_ssr: bool,
    /// Diagnostic: skip the global-illumination pass (`RP_GLOBAL_ILLUMINATION`) on both eyes. GI can
    /// carry a temporal/probe history that differs per eye; a companion to [`skip_ssr`](Self::skip_ssr)
    /// for isolating the residual per-eye MainColor divergence that survives SSR-off and SSAO-off.
    pub skip_gi: bool,
    /// Diagnostic: skip the AO-volumes pass (`RP_AO_VOLUMES`) on both eyes. AO volumes are
    /// artist-placed darkening volumes rendered as depth-tested proxy geometry; a volume whose proxy
    /// faces are borderline against nearby geometry can flip its entire contribution on a sub-pixel
    /// depth shift, so the temporal jitter cycles it -- the prime suspect for the blob-scale
    /// "shadows flicker in and out" artifact in MainColor (issue #10's residual flicker).
    pub skip_ao_volumes: bool,
    /// Diagnostic: skip the [`skip_pass_range`](Self::skip_pass_range) passes on both eyes. A
    /// separate flag (rather than an `Option` around the range) so the range can be preset while
    /// disarmed -- dragging the bounds live sweeps through intermediate ranges, some of which are
    /// unsafe to skip.
    pub skip_pass_range_enabled: bool,
    /// Diagnostic: the inclusive render-pass index range `[start, end]` to skip while
    /// [`skip_pass_range_enabled`](Self::skip_pass_range_enabled), for bisecting which pass an
    /// artifact originates in ([`RenderPassId`](jc3gi::graphics_engine::render_engine::RenderPassId)
    /// maps every index; GBuffer 0x2F..0x55, lighting/main 0x56..0x96).
    pub skip_pass_range: (i32, i32),
    /// Restore the SSAO temporal history index (`CSSAOPass +0x9A0`/`+0x9A4`) between the two stereo
    /// eyes. The index advances once per SSAO draw and is *not* reset by the `m_FirstPass` force, so the
    /// two eyes resolve against different history slots -- half the per-eye MainColor divergence. Pinning
    /// it (snapshot before eye 0, restore before eye 1) makes both eyes sample the same slot. **Default
    /// off pending validation** of the byte offsets against the RT-hash diagnostic.
    pub restore_ssao_history: bool,
    /// Restore the global-illumination cascade index (`CGISolver::m_CascadeToUpdate`, reached via the
    /// `CLightManager` singleton) between the two stereo eyes. It toggles which LPV cascade is refreshed
    /// each GI draw, so eye 0 and eye 1 leave the two cascades in different freshness states -- the other
    /// half of the per-eye MainColor divergence. Snapshot before eye 0, restore before eye 1 so eye 1
    /// refreshes the same cascade. **Default off pending validation.**
    pub restore_gi_cascade: bool,
    /// Patch the screen-space PCF rotation hash out of the sun-shadow shaders at creation, so both
    /// eyes use the same unrotated 38-tap PCF (removes the per-eye shadow shimmer + foliage grain).
    /// Applies only to shaders created after the hook installs; trigger a shader reload (e.g. change
    /// shadow quality) if injected mid-session. See [`crate::hooks::graphics_engine::shader`].
    pub patch_shadow_pcf_hash: bool,
    /// Strip the jitter translation from the fit camera's projection for the duration of
    /// `ShadowManager::UpdateRender`, so the fit frustum can never ingest a sub-pixel jitter. The
    /// fit reads the active camera, which the mod does not jitter, so this showed no effect on the
    /// issue-10 flicker; default off, kept as a defensive A/B.
    pub unjitter_shadow_fit: bool,
    /// Restore the render camera's pristine (center, unjittered) matrices after the frame's eye
    /// dispatches, so anything reading it before the next Draw sees the engine-built state rather
    /// than the last eye's jittered, offset projection. Showed no effect on the issue-10 flicker
    /// (the suspected sim-side reader uses the active camera instead); default off, kept as a
    /// hygiene A/B.
    pub restore_render_camera: bool,
    /// Patch the jitter-unstable material LOD dissolve out of the vegetation shaders at creation.
    /// Their screen-door dissolve pattern is keyed to the interpolated clip-space position (not
    /// `SV_Position`), so a camera jitter slides the whole pattern sub-pixel every frame and
    /// mid-fade geometry flips coverage coherently. Bytecode-real, but it was not the issue-10
    /// flicker and only matters while [`FsrConfig::jitter`](FsrConfig::jitter) is on, so it
    /// defaults off with the jitter. The patch makes the dissolve's discard unreachable (LOD
    /// transitions pop instead of dissolving); same reload caveat as
    /// [`patch_shadow_pcf_hash`](Self::patch_shadow_pcf_hash).
    pub patch_lod_dissolve: bool,
    /// Diagnostic: disable the sun-shadow system entirely through the engine's own settings path
    /// (`CShadowManager` enabled flag, synced by the sim-side `UpdateRender` via `SetEnabled`). The
    /// sharpest shadow-pipeline discriminator: an artifact that survives with no shadows at all
    /// cannot be shadow data.
    pub disable_sun_shadows: bool,
    /// Diagnostic: freeze the sun-shadow atlas by re-clearing the pass-enable flags after
    /// `CommitRenderPassSettings` sets them, so no shadow pass renders and the atlas keeps its last
    /// contents. Shadows stay visible but stop updating: an artifact that survives the freeze is in
    /// the shadow *sampling*; one that dies with it is in the atlas *contents*.
    pub freeze_shadow_maps: bool,
    /// Deduplicate the world post-effects block to once per dispatch. `ApplyWorldFilters` enqueues
    /// the block into the pass's *draw* list at draw time, which the between-eye list-parity restore
    /// cannot zero -- so eye 1 draws eye 0's stale entry plus its own, running the whole post chain
    /// (and FSR) twice. The double-stepped FSR history is the residual per-eye flicker of issue #10.
    pub dedupe_post_block: bool,
}
impl StereoConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            cameras: true,
            ipd: 0.068,
            force_smaa_1x: true,
            force_ssao_first_pass: true,
            present_eye_0: false,
            restore_frame_counters: true,
            gate_setup_render_frame_data: false,
            gate_hand_back_buffers: false,
            gate_eye1_dt: true,
            drain_draw_fragment: true,
            fix_shadow_cascade_anchor: true,
            diagnose_rt_hashes: false,
            disable_ssao: false,
            ssao_eye0_only: false,
            restore_cb_ring: false,
            skip_ssr: false,
            skip_gi: false,
            skip_ao_volumes: false,
            skip_pass_range_enabled: false,
            skip_pass_range: (0x56, 0x56),
            restore_ssao_history: false,
            restore_gi_cascade: false,
            patch_shadow_pcf_hash: true,
            unjitter_shadow_fit: false,
            restore_render_camera: false,
            patch_lod_dissolve: false,
            disable_sun_shadows: false,
            freeze_shadow_maps: false,
            dedupe_post_block: true,
        }
    }
}

/// Auto-exposure toggles.
#[derive(Clone, Serialize, Deserialize)]
pub struct ExposureConfig {
    /// Skip the per-frame auto-exposure metering on eye 1 (the stereo-darkening fix).
    pub gate: bool,
    /// Pin `m_CurrentExposure` to `forced_value` instead of the engine's auto-exposure (A/B aid).
    pub force: bool,
    /// The pinned exposure value, used when `force` is set.
    pub forced_value: f32,
}
impl ExposureConfig {
    pub const fn new() -> Self {
        Self {
            gate: true,
            force: false,
            forced_value: 0.11,
        }
    }
}

/// Post-effect skip toggles (bisection aids / VR cleanups).
#[derive(Clone, Serialize, Deserialize)]
pub struct PostFxConfig {
    pub skip_motion_blur: bool,
    pub skip_motion_blur_recon: bool,
    pub skip_dof: bool,
    pub dof_no_reproject: bool,
    pub skip_fade: bool,
    pub skip_glare: bool,
    pub skip_player_damage: bool,
    pub skip_sun_halo: bool,
    pub skip_histogram: bool,
}
impl PostFxConfig {
    pub const fn new() -> Self {
        Self {
            skip_motion_blur: false,
            skip_motion_blur_recon: false,
            skip_dof: false,
            dof_no_reproject: true,
            skip_fade: false,
            skip_glare: false,
            skip_player_damage: false,
            skip_sun_halo: false,
            skip_histogram: false,
        }
    }
}

/// VR head/body camera settings (was `hooks::camera::CameraSettings`).
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct CameraConfig {
    pub enabled: bool,
    pub body_offset: glam::Vec3,
    pub head_offset: glam::Vec3,
    pub use_eye_matrices: bool,
    pub blurs_enabled: bool,
    pub always_use_t1: bool,
}
impl CameraConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            body_offset: glam::Vec3::new(0.0, 0.1, 0.0),
            // Tuned in-game for the headpose path: the head bone origin sits above and behind the
            // eyes, so the camera drops 5 cm and moves 5 cm forward in the head frame.
            head_offset: glam::Vec3::new(0.0, -0.05, -0.05),
            use_eye_matrices: true,
            blurs_enabled: false,
            always_use_t1: false,
        }
    }
}

/// On-foot movement settings.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct MovementConfig {
    /// Force the aim-relative (strafe) locomotion acts on foot, instead of the third-person run
    /// mode where the directional keys rotate the whole body (nauseating in VR). Implemented as a
    /// scoped shim (see [`crate::hooks::input::locomotion`]): the local player's aim flags are
    /// forced to the aim-relative state only while each locomotion task's update runs, and
    /// restored afterwards, so the aim *system* (reticle, auto-aim, ADS) never sees the forced
    /// state. Two known gaps, in-game verified: the aim-loco acts are combat-stance animations
    /// (arms raised, body bladed -- the pose is baked into the animations, not layered by the aim
    /// system), and the continuous body-yaw-tracks-camera behaviour of real aiming is driven by a
    /// separate aim-gated system this shim does not activate, so the body heading is not steered
    /// (reversed-camera backpedal tank-turns). Kept as the acts half of the eventual solution.
    pub force_fps_movement: bool,
    /// Continuously yaw the body toward the camera on foot -- the heading half of FPS movement.
    /// Implemented by writing the camera's ground-plane forward to the character's target-face-dir
    /// blackboard value and forcing the game's own orientation executor
    /// (`NStateTask_LocoUtil::EvaluateCharacterOrientation`) into its face-dir-tracking mode for
    /// the local player, so the native rate-limited turn code does the rotating in every on-foot
    /// state, holstered included. See `crate::hooks::input::locomotion`.
    pub face_camera: bool,
    /// The tracking turn rate: the maximum yaw step, in degrees per orientation update (one per
    /// frame), passed to the orientation executor while [`face_camera`](Self::face_camera) forces
    /// tracking. Must stay positive; the executor divides by it.
    pub face_camera_turn_step: f32,
    /// The half-angle, in degrees, of the input cone around camera-forward within which the
    /// face-camera pin applies while moving (it always applies while idle). At the default 180
    /// the pin always applies; lower it to hand lateral/backward input back to the native steer
    /// (turn-and-run) instead of [`slide_strafe`](Self::slide_strafe).
    pub face_camera_input_cone_deg: f32,
    /// Make lateral and backward input actually translate the character while the body is pinned
    /// to the camera, instead of fighting the turn animations in place. Two overrides for the
    /// local player: the movement task's displacement direction is redirected along the input move
    /// direction after `NStateTask_LocoUtil::EvaluateCharacterDisplacement` computes it (the task
    /// then scales it by the native speed envelope), and `QueueMoveActions` is replaced to always
    /// queue the plain forward move act so the legs play a clean forward run rather than
    /// half-cancelled turn acts. The legs do not match the movement direction (the game ships no
    /// neutral strafe animations) -- deliberate animationless sliding.
    pub slide_strafe: bool,
    /// The yaw correction, in degrees, applied to the input move direction before it is written as
    /// the displacement direction. The direction is consumed in a frame whose ground axes are
    /// rotated from the blackboard move direction's world frame by an amount that in-game tests
    /// have not yet pinned down (candidates disagreed between runs), so it is a live dial: adjust
    /// until W slides away from the camera and D slides right.
    pub slide_rotation_deg: f32,
    /// Reach the target speed instantly while sliding. The native on-foot speed envelope is the
    /// animation's root velocity, so the run-start clips ramp the character up from zero; this
    /// floors `NStateTask_LocoUtil::EvaluateCharacterSpeed`'s result to the blackboard target
    /// speed while input is held, making the motion uniform from the first frame -- the wind-up
    /// stops affecting the movement, which reads much better from a first-person viewpoint.
    pub slide_instant_speed: bool,
    /// Skip the run-start wind-up acts while sliding: when the input tasks would queue a
    /// directional start act, queue the plain forward move act instead -- guarded by the game's
    /// own `TryAct` pre-flight, with the native starts as the fallback when the animation state
    /// machine refuses it. The legs pop straight into the run cycle with no wind-up lean.
    pub slide_skip_starts: bool,
}
impl MovementConfig {
    pub const fn new() -> Self {
        Self {
            // Off by default: the aim-loco acts it forces are combat-stance animations, which
            // obscures assessing the face-camera heading on its own. Turn it on (with a weapon
            // wielded) for the full directional-legs FPS movement.
            force_fps_movement: false,
            face_camera: true,
            face_camera_turn_step: 10.0,
            face_camera_input_cone_deg: 180.0,
            slide_strafe: true,
            // With the world-to-local transform in place this is only the local frame's forward
            // convention; dial live from the Game tab until W slides away from the camera.
            slide_rotation_deg: 0.0,
            slide_instant_speed: true,
            slide_skip_starts: true,
        }
    }
}

/// FSR anti-aliasing / upscaling settings. When `enabled`, FSR runs in place of the engine's SMAA
/// (which is suppressed); off restores the engine AA. See `docs/fsr.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct FsrConfig {
    /// Master switch: run FSR and suppress the engine AA. Off = engine SMAA as normal, FSR idle.
    pub enabled: bool,
    /// Apply the temporal sub-pixel jitter (camera projection + dispatch). FSR needs it to
    /// reconstruct sub-pixel detail, but it also excites a blob-scale shadow-term flicker whose
    /// mechanism resisted a long bisection (issue #10) -- every identified jitter coupling was
    /// fixed or ruled out (motion vectors, the post-chain double-run, the LOD dissolve, the shadow
    /// fit) and the flicker still tracked the jitter, so it ships off: stability over sharpness.
    /// Enable to trade back.
    pub jitter: bool,
    /// The sign convention of the *camera-side* jitter (the clip-space translation on the
    /// projection); the dispatch side always reports FSR's canonical offset. The two sides must
    /// agree or FSR de-jitters in the wrong direction and high-contrast detail pulses at the Halton
    /// cadence (the localised one-frame flicker of issue #10) -- a runtime knob so the convention can
    /// be settled live, like [`mv_sign`](Self::mv_sign). Default `(1, 1)` (the FSR-documented
    /// `(2*jx/w, -2*jy/h)` mapping).
    pub jitter_sign: (f32, f32),
    /// Scale on the jitter amplitude (0..1), applied consistently to the camera and the dispatch. A
    /// diagnostic lever: if no [`jitter_sign`](Self::jitter_sign) fixes the pulse but halving the
    /// amplitude softens it, the cause is FSR's own lock dynamics rather than a convention mismatch.
    pub jitter_scale: f32,
    /// Optional RCAS sharpening strength (0..1); `None` disables the sharpening pass.
    pub sharpness: Option<f32>,
    /// Feed motion vectors to FSR. Off makes FSR reproject with zero motion (ghosts moving objects) --
    /// a debug A/B to confirm the decode is helping.
    pub motion_vectors: bool,
    /// The sign/axis convention applied to the decoded UV motion before FSR. The decode math is now
    /// RE-exact (see `docs/fsr.md`); only FSR's expected sign/Y direction is empirical -- a wrong sign
    /// is visually obvious (trails point backwards). Defaults to `(1, -1)` (UV is Y-down; FSR's
    /// convention TBD against on-screen motion).
    pub mv_sign: (f32, f32),
    /// Correct the motion vectors for stereo in the decode pass. The engine's velocity encodes
    /// `curUV - prevUV` with the *per-eye* current view-projection but the single sim-side *center*
    /// previous view-projection, so every static pixel carries a spurious depth-dependent parallax
    /// vector of opposite sign per eye, and FSR mis-reprojects each eye's temporal history -- the
    /// per-eye shadow-edge flicker under head motion (issue #10). The correction re-anchors each
    /// vector at the eye's own previous pose ([`crate::stereo::VpHistory`]); a no-op without stereo
    /// disparity.
    pub mv_stereo_correction: bool,
    /// Cancel the camera jitter from the motion vectors in the decode pass. The engine measures
    /// `curUV` under the jittered projection, so every stored vector carries the frame's sub-pixel
    /// jitter as a constant offset, while FSR expects jitter-free motion. A correctness fix for
    /// whenever [`jitter`](Self::jitter) is on (it was not the issue-10 flicker); a no-op while
    /// jitter is off.
    pub mv_jitter_cancel: bool,
}
impl FsrConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            jitter: false,
            jitter_sign: (1.0, 1.0),
            jitter_scale: 1.0,
            sharpness: Some(0.2),
            motion_vectors: true,
            mv_sign: (1.0, -1.0),
            mv_stereo_correction: true,
            mv_jitter_cancel: true,
        }
    }
}
