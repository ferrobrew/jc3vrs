//! Runtime configuration: every user-facing toggle, consolidated into one mutex-guarded struct with
//! sub-structs by concern. The debug UI reads/writes the whole struct; hooks copy out the field(s)
//! they need at the top of a detour. Live engine-interface state (the current eye, frame counters,
//! the trace arm-flag) does NOT live here -- see [`crate::stereo::StereoState`] and the per-subsystem
//! runtime statics.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{headpose::HeadPoseConfig, hud::HudConfig, vr::VrConfig};

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
    pub body_ik: BodyIkConfig,
    pub vr: VrConfig,
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
            body_ik: BodyIkConfig::new(),
            vr: VrConfig::new(),
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
    /// Invalidate the terrain tessellation constant-buffer cache between the two eyes, so eye 1
    /// re-uploads it with its own projection. The terrain blocks cache the baked view-projection keyed
    /// on the render frame number, which [`restore_frame_counters`](Self::restore_frame_counters) pins
    /// across both eyes, so eye 1 otherwise reuses eye 0's projection for the distant tessellated
    /// terrain -- harmless in flatscreen stereo (both eyes share the projection) but a sheared horizon
    /// wedge in VR (the per-eye off-axis projections differ). Only meaningful while
    /// `restore_frame_counters` is on. See [`crate::hooks::game`].
    pub invalidate_terrain_cb: bool,
    /// Reconstruct the screen-space passes' clip-to-view inverse from the true off-axis projection
    /// while rendering a VR eye. The reconstruction passes (SSR, deferred clustered lighting, SSAO,
    /// screen-space subsurface, atmospheric scattering, depth of field) rebuild it with
    /// [`Matrix4::PerspectiveFovInverse`](jc3gi::types::math::Matrix4), which can only encode a
    /// *symmetric* frustum -- exact for the flatscreen stereo center projection but wrong, and
    /// mirror-opposite between eyes, under VR's off-axis projection, so specular and reflections on
    /// shiny surfaces (car paint, chrome) diverge grossly per eye. The override replaces the symmetric
    /// inverse with the exact inverse of the per-eye off-axis projection. VR only; a no-op on
    /// flatscreen frames. See [`crate::hooks::graphics_engine`].
    pub reconstruct_offaxis_inverse: bool,
    /// Exclude the atmospheric-scattering pass from the
    /// [`reconstruct_offaxis_inverse`](Self::reconstruct_offaxis_inverse) override. That pass
    /// reconstructs the whole screen (sky included) and samples the sun shadow cascade over it; the
    /// off-axis inverse is correct for finite geometry but at the far plane its off-centre shear
    /// dominates and is mirror-opposite between the eyes, so the sky reconstruction swims with head
    /// roll and crosses the cascade boundary -- the floating black crescent, and a contributor to the
    /// distant per-eye shadow flip. With this on, the atmospheric pass falls back to the engine's
    /// symmetric rebuild while the finite-geometry passes (specular, SSR) keep the off-axis override.
    pub offaxis_inverse_skip_atmospheric: bool,
    /// Widen the scene visibility-cull frustum to cover both eyes' off-axis frusta. The engine culls
    /// the scene (terrain, models, streaming) once per frame against the center camera's narrower,
    /// symmetric frustum, so geometry an eye can see past that frustum's edge is never drawn -- the
    /// black voids and pop-in at the outer edges of each eye in VR. This writes a symmetric union-FOV
    /// projection over the shared cull camera's `m_ProjectionF` (leaving the per-eye render projections
    /// untouched), so the cull covers everything either eye can see. VR only. See
    /// [`crate::hooks::graphics_engine`].
    pub widen_cull_frustum: bool,
    /// Extra fraction to expand the union-FOV cull frustum on every side, on top of the per-eye FOV
    /// union and the lateral eye-shift margin. The union already bounds both eyes' frusta, but the
    /// combined headset view is wide and the engine culls at a single interpolated pose, so under fast
    /// motion (especially flying) geometry can still pop in at the outer edges before the cull catches
    /// up. This pads each side's tangent outward -- `0.1` is 10% wider per side -- and, unlike the bare
    /// eye-shift margin, applies to the vertical axis too (which flying pitch shifts). Costs some
    /// over-draw of just-off-screen geometry. The resulting half-angle is clamped safely below 90° so a
    /// wide-FOV headset cannot push the tangent widen to a degenerate frustum. VR only; ignored when
    /// `widen_cull_frustum` is off.
    pub cull_fov_padding: f32,
    /// The FOV (degrees) the scene size-cull uses, overriding the mod's injected 90° on the main cull
    /// camera. BFBC runs a *screen-space size cull* separate from the frustum cull: it drops an object
    /// whose angular size falls below `tan(cullFov/2) · minScreenPercentage`. That threshold scales
    /// with `tan(FOV/2)`, and the mod forces a 90° camera FOV (`tan 45° = 1.0`) where flat JC3 runs
    /// ~50° (`tan ≈ 0.45`), so in VR the size cull is ~2× too aggressive -- small and distant geometry
    /// and individual vehicle sub-meshes are dropped at double the distance and "resolve" only as you
    /// approach. Writing a flatter FOV onto the cull camera's `m_FOVT1` (used *only* by the size and
    /// AO-volume culls, not the frustum or LOD) restores flat-equivalent density. Lower keeps more
    /// geometry (more overdraw); `0` leaves `m_FOVT1` untouched. VR only; gated by `widen_cull_frustum`.
    pub cull_size_fov_deg: f32,
    /// Disable BFBC software occlusion for the main view. On top of the frustum cull, the engine tests
    /// each object against occluder silhouette frustums cast from the *single centre viewpoint*, so
    /// geometry an offset eye could peek past an occluder's edge is still culled for both eyes -- and
    /// the frustum widen re-includes edge occluders (`m_RemoveOccluderPlanesOutsideFrustum`),
    /// concentrating the loss at the wide peripheries. This drops the occluder frustums for the main
    /// cull camera (leaving only the widened camera frustum) by setting `m_FrustumCount` to 1 in the
    /// frustum-cull params after the engine builds them, so only view-frustum culling remains. Costs
    /// some overdraw of centre-occluded geometry; defensible in VR where centre-viewpoint occlusion is
    /// geometrically wrong for both offset eyes. VR only.
    pub disable_bfbc_occlusion: bool,
    /// Make the landscape terrain patch system cull against the binocular union camera. Terrain
    /// patches are decided by a *separate* landscape system that culls against its own
    /// `STerrainPatchSystem.m_TerrainCamera` (a per-frame copy of the centre camera), not the occluder
    /// manager's cull camera the frustum widen above touches -- so widening that camera does nothing
    /// for terrain, and the narrow centre fit leaves bottom/edge patch holes when flying. This detours
    /// `TerrainPatchSystemUpdate` and, after the engine refreshes `m_TerrainCamera`, stamps the union
    /// projection onto it and rebuilds its view-projection and six frustum planes
    /// (`Camera::UpdateFrustum`), so the terrain patch set covers everything either eye can see. Once
    /// per frame; only terrain visibility reads that camera. VR only.
    pub widen_terrain_cull: bool,
    /// Widen the sun-shadow cascade *fit* frustum to cover both eyes. The engine fits the cascaded
    /// shadow map once per frame to the centre camera's narrow `m_ProjectionF`, so the wider, laterally
    /// shifted VR eyes see distant/peripheral geometry that falls outside the fitted coverage box --
    /// where shadows clamp to the atlas border or a wrong texel, differently per eye (distant shadows
    /// disagree between the eyes) and crawling as the fit boundary re-quantizes under motion. This
    /// scoped-widens only the two FOV-scale terms (`m_ProjectionF` data[0]/data[5]) of the active
    /// camera to the union FOV around `ShadowManager::UpdateRender`, so the cascades cover both eyes;
    /// the near/far/split terms are left untouched. Complements
    /// [`fix_shadow_cascade_anchor`](Self::fix_shadow_cascade_anchor) (which re-anchors the *sampling*;
    /// this fixes the *coverage*). Costs some shadow resolution (cascades cover more world area). VR
    /// only; no-op on flatscreen.
    pub widen_shadow_fit: bool,
    /// Stabilize the sun-shadow cascade fit against head *orientation*, so shadows don't change when
    /// you only rotate your view. The engine pushes each cascade box's centre forward along the active
    /// camera's forward vector (`m_TransformT1` row 2), so tilting the head slides the cascade centre --
    /// the near cascade re-covers a different area at a different texel density, and shadows (including
    /// the player's own) visibly shift, re-quantize, and shrink/grow with head pitch. This horizontalizes
    /// that forward vector (yaw-only, projected onto the ground) around `ShadowManager::UpdateRender`, so
    /// the cascade centre follows heading but not head pitch/roll -- shadows stay put as you look around.
    /// The box *size* (sphere-based) and *orientation* (sun-fixed) are already view-independent. VR only.
    pub stabilize_shadow_fit: bool,
    /// Recreate the froxel volumetric-fog block's coarse volumetric-depth buffer at full render
    /// resolution instead of half. The fog block bilaterally upsamples that coarse buffer, and VR's
    /// wide FOV magnifies its grid into the blocky tiles around lights and explosions (issue #8). The
    /// hook no-ops the two width/height halving multiplies in
    /// [`RenderBlockTypeFogVolume::ResizeTextures`](jc3gi::graphics_engine::render_block::RenderBlockTypeFogVolume)
    /// around the call, leaving the full-res colour and volume textures untouched. `ResizeTextures`
    /// only runs when the fog textures are recreated, so this takes effect at the next resolution
    /// change, not the instant it is toggled. Costs fog fill rate and memory. **Default off** (not
    /// headset-verified; a coarse-buffer format the shader assumes to be half-res could misregister).
    pub fog_full_res: bool,
    /// Route particles to the full-resolution transparent pass instead of the low-resolution particle
    /// pass, by clearing the particle block type's
    /// [`m_LowResRendering`](jc3gi::graphics_engine::render_block::RenderBlockTypeParticle::m_LowResRendering)
    /// and `m_ForceLowResRendering` flags. VR's wide FOV magnifies the low-res particle grid into the
    /// same tiles as the fog (issue #8). The engine's full-res transparent pass always draws, so
    /// particles route rather than vanish; still, this is the riskiest lever. **Default off** and needs
    /// live A/B — if some particle family does not survive the reroute it could look wrong or drop out,
    /// and it costs transparent-pass fill rate. Applied one frame ahead (routing runs before the pass
    /// draws); reverted to the engine's setting when turned off.
    pub particles_full_res: bool,
    /// Render volumetric spot-light cones at full resolution instead of quarter resolution, by scoping
    /// the engine's `enable_low_res_spot_light_volume` global to `false` around the per-frame light
    /// gather ([`LightManager::CopyLightsToUpdate`](jc3gi::graphics_engine::light_manager::LightManager)).
    /// The engine's own full-resolution branch then runs (main render setup, cone low-res flag cleared),
    /// removing the coarse spot-light-volume tiles VR's wide FOV magnifies (issue #8). The lowest-risk
    /// of the three resolution levers (an engine-supported path), but still costs cone fill rate and is
    /// not headset-verified. **Default off.**
    pub spotlight_full_res: bool,
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
            invalidate_terrain_cb: true,
            reconstruct_offaxis_inverse: true,
            offaxis_inverse_skip_atmospheric: true,
            widen_cull_frustum: true,
            cull_fov_padding: 0.4,
            cull_size_fov_deg: 50.0,
            disable_bfbc_occlusion: true,
            widen_terrain_cull: true,
            widen_shadow_fit: true,
            stabilize_shadow_fit: true,
            fog_full_res: false,
            particles_full_res: false,
            spotlight_full_res: false,
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
    /// Hide the player's head by collapsing its facial bones' skinning matrices in non-shadow
    /// passes (see `hooks::graphics_engine::render_block`): the whole head — face, eyes, hair,
    /// and any gear weighted to facial bones — contracts to a point inside the collar, while the
    /// shadow passes see the real palette, so the shadow keeps its head.
    pub hide_head_draws: bool,
    /// The legacy head-hide: scale the HEAD bone and a facial-bone list to 0.001. Kept as a
    /// fallback; superseded by `hide_head_draws` (the scale approach also removed the head from
    /// the shadow, and its unscaled child bones leaked the eyes into view).
    pub hide_head_scale: bool,
}
impl CameraConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            // Both offsets default to zero now that the head is properly hidden: with
            // use_eye_matrices on (the default), the camera arm is the measured neck-to-eye arm
            // from the animated eye bones and head_offset is a correction on top of it; with it
            // off, head_offset is the whole arm from the neck pivot.
            body_offset: glam::Vec3::ZERO,
            head_offset: glam::Vec3::ZERO,
            use_eye_matrices: true,
            blurs_enabled: false,
            always_use_t1: false,
            hide_head_draws: true,
            hide_head_scale: false,
        }
    }
}

/// Headset-driven upper-body IK: drive the player's spine and head toward the headpose target by
/// feeding the engine's own HumanIK `MAIN` pass an effector target for the head bone, so the body
/// leans, ducks, and turns to follow where the player looks. Queued pre-solve in
/// [`crate::hooks::character`] (see `docs/engine/humanik.md`); the `UpdatePropEffects` head-bone override
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
    /// Suppress the vehicle reversing look-behind animation (`ACT_REVERSE` /
    /// `ACT_REVERSE_MOTORBIKE` into the `S_REVERSE_*` states): the acts are dropped at
    /// `Character::QueueAct` for the local player, so Rico keeps facing forward while reversing --
    /// with a player-driven head, looking behind is the player's job, and the forced body turn is
    /// discomforting.
    pub suppress_reverse_look: bool,
    /// Suppress the head-driven body turn during a jump. The airborne actuator
    /// (`NStateTask_MovementJumpTask::Update`) faces the body at the weapon-aim target while
    /// [`m_AimingWeapon`](jc3gi::character::character::AimState::m_AimingWeapon) is set, and in VR
    /// that target follows the HMD gaze -- so turning your head yaws your body mid-jump with no stick
    /// input. This clears the aim bit around the jump update for the local player while the head is
    /// decoupled (the VR source), routing the jump through its non-aiming fallback (current forward
    /// plus stick-gated steer). Restored immediately after. See `crate::hooks::input::locomotion`.
    pub suppress_air_aim_facing: bool,
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
            suppress_reverse_look: true,
            suppress_air_aim_facing: true,
        }
    }
}

/// FSR anti-aliasing / upscaling settings. When `enabled`, FSR runs in place of the engine's SMAA
/// (which is suppressed); off restores the engine AA. See `docs/mod/fsr.md`.
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
    /// RE-exact (see `docs/mod/fsr.md`); only FSR's expected sign/Y direction is empirical -- a wrong sign
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
