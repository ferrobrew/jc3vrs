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
    #[serde(default)]
    pub foveation: FoveationConfig,
    #[serde(default)]
    pub far_field: FarFieldConfig,
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
            foveation: FoveationConfig::new(),
            far_field: FarFieldConfig::new(),
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
    /// Render the view-independent pre-passes once and reuse them for the second eye (issue #30): on
    /// eye 1, `PreDraw` skips the reflection proxies, cloud shadows, the sun-shadow cascade atlas, and
    /// the water simulation (all driven by the sun / reflection / world cameras, writing separate
    /// persistent targets), reusing eye 0's output. Halves the second eye's pre-pass cost, and renders
    /// the shared sun-shadow atlas once per frame instead of twice -- which also removes the per-eye
    /// shadow flicker (issue #31). Requires [`restore_frame_counters`](Self::restore_frame_counters) so
    /// both eyes share the shadow-atlas parity slot; a no-op without it. VR/stereo only.
    pub share_prepasses: bool,
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
    /// Defer the frame tail -- the final dispatch's draw-thread drain, the VR eye blit and submit,
    /// and the desktop mirror -- onto a dedicated tail thread, so the next frame's sim tick runs on
    /// the main thread while the draw thread finishes eye 1 and the GPU drains its tail (the
    /// frame-boundary starvation bubble the profiler measures as "GPU idle"). Safe by
    /// construction: the next frame's `vr::frame_begin` blocks on the VR runtime lock, which the
    /// tail releases only after the blit is recorded, so nothing downstream can race the in-flight
    /// draw or overwrite the capture textures early. Falls back to the inline tail while the F10
    /// capture or a render trace is active (both need the eyes drained on the main thread). The
    /// mirror moves before the XR submit in this mode, costing it ~0.4 ms of HMD submit latency.
    /// On by default: validated in VR (both eyes correct, mirror UI intact) with a measured mean
    /// and p95 frame-time win.
    pub defer_frame_tail: bool,
    /// Correct the sun-shadow cascade anchor per eye. The cascaded shadow map is fit to the shared
    /// center camera, but the material shaders anchor the cascade lookup at the *per-eye* camera
    /// position (`cb0[4]`), so each eye's shadow is shifted by `M * (eyePos - centerPos)` -- the visible
    /// per-eye sun-shadow mismatch (edge/length/strength differing between eyes, only with disparity).
    /// This adds `M * delta` to the cascade transform translation to re-anchor the lookup at center. The
    /// directly visible stereo-shadow fix; A/B by flipping `present_eye_0` with it on/off.
    pub fix_shadow_cascade_anchor: bool,
    /// Diagnostic: during a stereo frame, record each eye's per-pass GPU-op count and log a diff --
    /// which passes draw identically between eyes (replayable from one walk) and which diverge (the
    /// per-eye special-casing burden). The feasibility probe for single-pass stereo; see
    /// [`crate::debug::stereo_diff`]. Logs on target `"stereo_diff"`.
    pub diagnose_stereo_draw_diff: bool,
    /// Diagnostic: hash a curated set of engine render targets after each eye's Draw and record the
    /// per-eye hashes into the active render trace. Run with `cameras` off (both eyes share one
    /// camera) so any RT whose two eyes' hashes differ is being accumulated across the two Draws --
    /// the "stronger in one eye" bug. See [`crate::debug::rt_hash`].
    pub diagnose_rt_hashes: bool,
    /// Diagnostic: while a render trace collects, encode eye 0's final `BackBufferLinear` to a PNG each
    /// frame into the trace's `traces/<stamp>/` folder (alongside `trace.ndjson`), named by frame index
    /// so the sequence reassembles and aligns 1:1 with the per-frame trace events. For localizing
    /// per-frame visual artifacts (e.g. the shadow flicker) the numeric trace cannot place spatially.
    /// Heavy (a full readback + PNG encode per frame); only meaningful during a manual trace.
    pub diagnose_rt_screenshots: bool,
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
    /// it (snapshot before eye 0, restore before eye 1) makes both eyes sample the same slot. **On by
    /// default** -- measured to cut the per-eye MainColor brightness gap substantially.
    pub restore_ssao_history: bool,
    /// Restore the global-illumination cascade index (`CGISolver::m_CascadeToUpdate`, reached via the
    /// `CLightManager` singleton) between the two stereo eyes. It toggles which LPV cascade is refreshed
    /// each GI draw, so eye 0 and eye 1 leave the two cascades in different freshness states -- the other
    /// half of the per-eye MainColor divergence. Snapshot before eye 0, restore before eye 1 so eye 1
    /// refreshes the same cascade. **On by default** -- pairs with
    /// [`restore_ssao_history`](Self::restore_ssao_history) to remove the per-eye MainColor divergence.
    pub restore_gi_cascade: bool,
    /// Patch the screen-space PCF rotation hash out of the sun-shadow shaders at creation, so both
    /// eyes use the same unrotated 38-tap PCF (removes the per-eye shadow shimmer + foliage grain).
    /// Applies only to shaders created after the hook installs; trigger a shader reload (e.g. change
    /// shadow quality) if injected mid-session. See [`crate::hooks::graphics_engine::shader`].
    pub patch_shadow_pcf_hash: bool,
    /// Patch the jitter-unstable material LOD dissolve out of the vegetation shaders at creation.
    /// Their screen-door dissolve pattern is keyed to the interpolated clip-space position (not
    /// `SV_Position`), so a camera jitter slides the whole pattern sub-pixel every frame and
    /// mid-fade geometry flips coverage coherently. Bytecode-real, but it was not the issue-10
    /// flicker and only matters while [`FsrConfig::jitter`](FsrConfig::jitter) is on, so it
    /// defaults off with the jitter. The patch makes the dissolve's discard unreachable (LOD
    /// transitions pop instead of dissolving); same reload caveat as
    /// [`patch_shadow_pcf_hash`](Self::patch_shadow_pcf_hash).
    pub patch_lod_dissolve: bool,
    /// Master switch for single-pass stereo (experimental; see `docs/mod/single-pass-stereo.md`).
    /// Instead of the double-draw (two full `game.Draw` walks, one per eye), render the G-buffer
    /// geometry once with the vertex shaders patched to emit both eyes via instancing +
    /// `SV_ViewportArrayIndex` routing into a double-wide render target. Off by default: the whole
    /// pipeline is under construction and validated against the double-draw oracle. Requires the DXVK
    /// viewport-routing capability (see [`crate::stereo::single_pass::capability`]); forced off if
    /// absent.
    pub single_pass: bool,
    /// Requires [`single_pass`](Self::single_pass): make the two eyes actually diverge. Fills `cb13` with **distinct** per-eye view-projections (slot 0 = eye 0, slot 1 =
    /// eye 1) instead of both = the current view, splits the bound viewport into left/right **halves**
    /// for `SV_ViewportArrayIndex` routing instead of two identical copies, and **doubles** the
    /// instance count of the G-buffer geometry draws (so `SV_InstanceID & 1` selects the eye). On its
    /// own -- without [`single_pass_double_wide`](Self::single_pass_double_wide) and the collapse --
    /// this renders each eye into half of a per-eye-sized target (squished), so it is a bring-up /
    /// bisection step, not a finished look.
    pub single_pass_dual_eye: bool,
    /// Requires [`single_pass_collapse`](Self::single_pass_collapse) and
    /// [`vr.native_resolution`](crate::config::VrConfig::native_resolution)): re-create the scene
    /// render targets at **2× per-eye width** so each eye's half is full resolution instead of a
    /// squished half of a per-eye target. Drives the engine render resolution
    /// ([`crate::vr::engine_render_resolution`]) via the same deferred `ApplyResize` the per-eye
    /// native-resolution path uses; the XR swapchain and per-eye capture textures stay per-eye width,
    /// so the collapse's capture split copies each full-width half straight into its eye texture.
    pub single_pass_double_wide: bool,
    /// Requires [`single_pass_dual_eye`](Self::single_pass_dual_eye): **collapse**
    /// the per-eye double-draw to a single `game.Draw` walk -- the actual draw-submission win. One
    /// walk produces both eyes (via the dual-eye `cb13` + viewport routing + instance doubling); the
    /// render camera stays centered, the between-eye snapshot/restore is dropped, and the capture
    /// splits the one back buffer into both eye textures. Works without
    /// [`single_pass_double_wide`](Self::single_pass_double_wide) (each eye-half is then squished);
    /// with it, each half is full resolution. The riskiest step; last to enable during bring-up.
    pub single_pass_collapse: bool,
    /// Census-only mode for [`single_pass`](Self::single_pass): run the vertex-shader stereo rewrite
    /// on every shader at creation and tally the outcomes (patched / no per-eye references / errored)
    /// **without** substituting the patched bytecode, so rendering is unchanged. Safe to inject: it
    /// validates the DXBC rewriter against the game's real shader set and reports the true census in
    /// the debug UI, before the rest of the single-pass pipeline is wired up.
    pub single_pass_patch_dryrun: bool,
    /// Reproject the no-`cb0` scene-geometry families (skinned characters/NPCs, props, buildings,
    /// roads, ...) for single-pass, instead of leaving them double-drawn. When on, a vertex shader
    /// with no per-eye `cb0` operand whose name is on the reprojection allowlist is rewritten to
    /// post-multiply its own clip position by the per-eye `M_eye` (see
    /// [`crate::stereo::single_pass`]); NDC writers (sky, UI, post) are excluded by the allowlist.
    /// Requires [`single_pass`](Self::single_pass); independent of the others so it can be A/B'd.
    pub single_pass_reproject: bool,
    /// Extend [`single_pass_reproject`](Self::single_pass_reproject) to the allowlisted families the
    /// `cb0` remap claims on a **camera-position** reference alone. The remap's candidacy test is
    /// "references one of `cb0[{4, 29..32}]`", but `cb0[4]` is a camera position a shader may read for
    /// a view vector or a distance fade while taking its clip from a baked matrix -- `generaljc3` reads
    /// it for a LOD fade and builds clip from `cb1[0..3]`. Claimed by the remap, such a family gets
    /// viewport routing but no per-eye clip, so under the collapse's centred render camera *both* eye
    /// halves are drawn from the centre viewpoint and the family sits at a rigid half-IPD offset from
    /// its surroundings -- visible in one eye, not only in stereo. This routes it to the reprojection
    /// it should have had, at no extra draw cost. Only ever moves a shader between two per-eye
    /// transforms: an unallowlisted family keeps the remap. Requires
    /// [`single_pass_reproject`](Self::single_pass_reproject). On by default.
    ///
    /// The families it reaches in the shipped bundle are `generaljc3`, `landmark`, `layered` and
    /// `layeredblend` -- one shared body, confirmed from the bytecode rather than the name.
    ///
    /// Caveat worth an A/B: all four apply a depth bias *after* their projection
    /// (`o0.z += cb2[0].x · o0.w`), and that bias is folded into the clip position the reprojection
    /// then transforms by `M_eye`, rather than left as a post-projection offset. `M_eye` is near
    /// identity, so the bias survives approximately; if decal or shadow z-fighting appears on those
    /// families, this is the first thing to switch off.
    pub single_pass_reproject_camera_only: bool,
    /// Single-pass the tessellated base terrain (VS → HS → DS): the vertex shader originates the eye
    /// index on the free `TEXCOORD3.z` lane, the hull shader forwards it, and the domain shader reads
    /// it to reproject its clip by the per-eye `M_eye` and route to the eye's viewport. Covers the
    /// `DrawIndexed` terrain passes (far/color/shadow); also gates the render-block re-issue that
    /// reprojects the GPU-indirect terrain-detail pass (see
    /// `payload/src/hooks/graphics_engine/terrain.rs`). Requires [`single_pass`](Self::single_pass);
    /// independent so it can be A/B'd against the models.
    pub single_pass_terrain: bool,
    /// Single-pass the far-distance tree impostors (`CTreeImpostorRB`): the impostor vertex shader
    /// writes its clip position from the global billboard view-projection and draws non-instanced, with
    /// no GPU-indirect path sharing it, so the reprojection rewrite plus instance-doubling covers it
    /// completely (unlike the other vegetation families, whose dominant draw is GPU-indirect). When on,
    /// the `treeimpostor*` vertex shaders take the same `M_eye` post-multiply as the reprojected scene
    /// families. Requires [`single_pass`](Self::single_pass); independent so it can be A/B'd.
    pub single_pass_tree_impostors: bool,
    /// Single-pass the tree-trunk/branch render block (`CRenderBlockBark`, "VegetationBark"). Its vertex
    /// shader reads a CPU-baked world-view-projection from `cb1` (not `cb0`) and draws via one of three
    /// kinds (plain, CPU-instanced, or GPU-indirect), so it can't ride the reprojection rewrite; instead
    /// the block's `Draw`/`DrawZ` is re-issued once per eye with the baked `cb1` reprojected by that eye's
    /// `M_eye`. While this is on, the `vegetationbark*` vertex shaders are also declined by the `cb0`
    /// remap, which would otherwise claim them on a `cb0[4]` camera-position reference that is not their
    /// position path. Requires the collapse; independent so it can be A/B'd. On by default: without it
    /// the trunks render at the centre view in both eyes, i.e. at zero disparity, which reads as them
    /// drifting in world space as the camera moves.
    pub single_pass_bark: bool,
    /// Single-pass the grass/foliage render block (`CRenderBlockFoliage`, "VegetationFoliage"). Its vertex
    /// shader reads a baked view-projection from `cb2` (registers 4..7); the block's `Draw` and `DrawZ`
    /// are re-issued once per eye with that `cb2` copy reprojected by `M_eye`. The dominant grass path is
    /// GPU-indirect, so re-issue (not instance-doubling) is the only option. While this is on, the
    /// `vegetationfoliage*` vertex shaders are also declined by the `cb0` remap, which would otherwise
    /// claim them on the `cb0[4]` reference their wind-noise lookup makes. Does not address the separate
    /// forward-clustered-lighting black-in-VR issue. Requires the collapse. On by default, for the same
    /// reason as [`single_pass_bark`](Self::single_pass_bark).
    pub single_pass_foliage: bool,
    /// Single-pass the occluder depth-prime render block (`CRenderBlockOccluder`). Its non-instanced path
    /// bakes a world-view-projection into `cb1`; the block's `DrawZ` is re-issued once per eye with `cb1`
    /// reprojected by `M_eye`, so each eye's depth is primed with its own projection. Requires the
    /// collapse. **Blind-implemented, unvalidated.**
    pub single_pass_occluder: bool,
    /// Re-issue an **already-instanced** draw once per eye under the collapse, instead of letting the
    /// patched vertex shader read the game's own instance ids as an eye parity. A `DrawIndexedInstanced`
    /// with a patched shader bound in the G-buffer range sends instance `i` to eye `i & 1` -- half the
    /// batch per eye, or the left eye alone at one instance. The instance count cannot simply be
    /// promoted (per-instance vertex-buffer stepping is indexed by the instance id, so doubling it reads
    /// past the instance data), so each eye gets its own submission with both `cb13` eye slots and both
    /// viewport slots pinned to that eye. Requires the collapse. On by default: it fixes a visible
    /// artifact (the buildings flickering), and costs ~130 extra draw submissions of ~20k per frame.
    pub single_pass_instanced_per_eye: bool,
    /// Re-issue a **GPU-indirect** draw once per eye under the collapse, so it stops inheriting
    /// whatever viewport the previous draw left bound. `DrawIndexedInstancedIndirect` and
    /// `DrawInstancedIndirect` are how the near tessellating terrain patches
    /// (`CRenderBlockTerrainPatch` passes 56-57) and the foliage block submit, and neither entry point
    /// was detoured -- so those draws rasterised eye 0's per-eye projection into the full double-wide
    /// viewport, a 2x horizontal stretch that reads as the geometry sliding across the screen at twice
    /// the camera's rate. The counts live in a GPU buffer and cannot be doubled, so each eye gets its
    /// own submission with both viewport slots pinned to that eye's half. Like the unpatched
    /// `DrawIndexed` re-issue, the geometry is then present and correctly sized in both eyes but has no
    /// parallax. Requires the collapse. On by default: it fixes a confirmed visible artifact.
    pub single_pass_indirect_per_eye: bool,
    /// Run the deferred clustered-lighting resolve once per eye under the collapse, each run masked to
    /// that eye's half of the double-wide target and reconstructing with that eye's own basis.
    ///
    /// The resolve is a fullscreen quad that rebuilds world positions from depth through the
    /// `ViewProjInv` [`reconstruct_offaxis_inverse`](Self::reconstruct_offaxis_inverse) substitutes,
    /// and samples the sun-shadow cascade over them. Collapsed, that single quad covers both eye halves
    /// while the basis describes one eye's frustum: the left half gets that eye's frustum compressed 2x
    /// horizontally and the right half a basis unrelated to it. The error is a function of the view
    /// matrix, so it moves as the camera turns -- the shadows slide across the screen instead of
    /// staying on the world. Splitting the pass is the only fix; one draw cannot carry two bases.
    ///
    /// Each run is masked with a **scissor**, not a half viewport, so the quad keeps its
    /// one-to-one NDC-to-pixel mapping and its G-buffer sampling stays correct; the half's basis comes
    /// from folding the full-target-NDC to eye-NDC remap into the substituted inverse. The whole render
    /// block is re-issued, so its light-assignment phase runs twice (identically) as well. Requires the
    /// collapse and `reconstruct_offaxis_inverse`. On by default: sliding shadows are a worse defect
    /// than the extra light-assignment pass is a cost, and the flag remains for an A/B in the headset.
    pub single_pass_reconstruct_per_eye: bool,
    /// The same per-eye split for the **atmospheric-scattering / aerial-perspective** pass, the other
    /// fullscreen consumer of the reconstruction basis.
    ///
    /// That pass reconstructs the whole screen from depth -- sky included -- and ray-marches the sun
    /// shadow cascade and aerial perspective over the reconstructed positions, so under the collapse it
    /// carries exactly the defect
    /// [`single_pass_reconstruct_per_eye`](Self::single_pass_reconstruct_per_eye) describes, and
    /// splitting only the deferred resolve leaves this pass painting the same sliding error back over
    /// it. Requires `reconstruct_offaxis_inverse`; on by default, for the same reason.
    pub single_pass_atmospheric_per_eye: bool,
    /// Bias the legacy (non-WaveWorks) water blocks' screen-space reflection/refraction lookup into
    /// each eye's half of the double-wide target, by re-issuing their `Draw` once per eye.
    ///
    /// The `Water*` family -- selected at the lower water-quality settings, in place of the
    /// `NvWater*` WaveWorks path -- does not sample its reflection, refraction, and depth buffers by
    /// pixel coordinate. Its block type stages a world→screen-UV matrix on vertex `cb1` once per
    /// pass (the view-projection with the NDC→UV `x·0.5 + w·0.5` already folded in), the vertex
    /// shader hands the result on as a projective `TEXCOORD1`, and the pixel shader divides by `w`.
    /// That UV is normalized over the **viewport**, i.e. over one eye's half, while the buffers it
    /// indexes are the whole double-wide target -- so each eye reads the entire two-eye image
    /// stretched across its water, and since the error is a fixed 2x scale it is a 2x motion gain
    /// too: the reflections slide over the water as the camera moves.
    ///
    /// The correction is one more bias per eye, `u' = (u + eye) · 0.5`, composed onto the rows the
    /// type staged -- no shader change, because the water vertex shaders take their clip position
    /// from the global `cb0` that the collapse already handles. It needs the per-eye re-issue only to
    /// know which eye it is for. Deliberately *not* reprojected by `M_eye`: the geometry still
    /// rasterizes from the collapsed centre view, so the UV must describe where it actually landed.
    ///
    /// Requires the collapse. On by default, but the hardest of the re-issues to A/B in practice: the
    /// affected family may not even be on screen at the water-quality setting in use, which makes a
    /// headset comparison the only way to tell the fix from a no-op, and unlike the other re-issues
    /// this one recomputes a constant the engine staged rather than transforming it in flight.
    pub single_pass_water_uv_per_eye: bool,
    /// Give the WaveWorks water blocks (`NvWater*`) a per-eye view under the collapse, so the water
    /// surface has parallax instead of being one eye's view shown to both.
    ///
    /// These blocks are not affected by the projective-UV defect
    /// ([`single_pass_water_uv_per_eye`](Self::single_pass_water_uv_per_eye)): their shaders derive
    /// the screen UV from `SV_Position` and the inverse screen size, which is already consistent with
    /// a double-wide target. Their defect is the other one — the vertex shader writes clip position
    /// from a baked model-view-projection in its own constant buffer rather than from the render
    /// context, so the collapse's per-eye machinery never reaches it and both eyes see the collapsed
    /// centre view. Flat water at the wrong depth reads as correct in a screenshot and wrong in a
    /// headset, which is why it survived this long. On by default -- the flag stays so a suspected regression can be
    /// isolated without rebuilding.
    pub single_pass_nvwater_per_eye: bool,
    /// Reproject the screen-space decal *box geometry* per eye, on top of
    /// [`single_pass_ssdecal_per_eye`](Self::single_pass_ssdecal_per_eye), which fixes only where the
    /// decal reconstructs from.
    ///
    /// The block bakes a world-view-projection into its vertex constants, so like the water blocks its
    /// geometry never sees the collapse's per-eye transform: the decal lands on the right surface in
    /// both eyes but its screen coverage has no parallax. Separate flag because the reconstruction fix
    /// is the one that stops the sliding, and this one only adds depth to it. On by default.
    pub single_pass_ssdecal_geometry_per_eye: bool,
    /// Give **geometry** drawn through the non-indexed `Draw` entry point (D3D11 context vtable slot
    /// 13) the same per-eye re-issue the indexed path gives its draws.
    ///
    /// Slot 13 is overwhelmingly how the fullscreen passes submit their triangle, so the collapse
    /// resets the viewport to the whole double-wide target for it. But the four decal blocks, the road
    /// layers, and the skidmarks submit ordinary world geometry non-indexed, and pinning the full
    /// viewport rasterises them across both eye halves, stretched 2x horizontally about the target
    /// centre. A 2x horizontal stretch is also a 2x horizontal *motion* gain, so they sweep across the
    /// screen at twice the camera's rate -- decals sliding over the world.
    ///
    /// Unlike the indirect and indexed paths this cannot be decided from the draw alone, since a
    /// fullscreen triangle and a decal box arrive identically; it is decided by an allowlist of passes
    /// known to carry geometry, derived by enumerating every caller of the two engine wrappers that
    /// reach the slot and taking pass membership from the pass-creation sites. On by default. The
    /// list stays an allowlist rather than a heuristic because misclassifying a fullscreen pass as
    /// geometry is a visibly wrong frame, while missing a geometry pass is only the status quo -- so
    /// a pass is added when it is evidenced, never on suspicion.
    pub single_pass_slot13_per_eye: bool,
    /// Derive the collapse's eye-half viewports from the render target the engine currently has
    /// bound, rather than always from the scene's double-wide viewport.
    ///
    /// Several passes redirect their draws to a **reduced-resolution off-screen target**: the shared
    /// quarter-resolution buffer (half per axis) that the low-resolution clouds, the low-resolution
    /// particles, and the volumetric spot-light cones all render into, and the downsampled depth
    /// buffer. The collapse's viewport split is otherwise target-blind — it re-derives the halves from
    /// the recorded scene viewport before every draw in the range — so a draw into a half-sized target
    /// receives a viewport twice that target's dimensions. Its content is magnified 2x about the
    /// target's origin and cropped, and since the error is a fixed scale it is a 2x motion gain too:
    /// clouds and smoke sweep past at twice the camera's rate.
    ///
    /// The composes need no matching change. They stretch the whole low-resolution texture over the
    /// whole double-wide target with a baked UV, so a texture holding a left-half/right-half image
    /// lands as a left-half/right-half screen image on its own — and they must **not** be re-issued
    /// per eye, since both blend rather than overwrite.
    ///
    /// On by default. Everywhere except those passes the two records hold the same viewport and this is
    /// a no-op, so turning it off isolates it cleanly.
    pub collapse_viewport_follows_target: bool,
    /// Run the **SSAO** pass once per eye under the collapse, like the deferred resolve and the
    /// atmospheric scattering.
    ///
    /// SSAO is one of the seven depth-reconstruction passes (cross-reference
    /// `CMatrix4f::PerspectiveFovInverse`; the set is closed), so collapsed it reconstructs the whole
    /// double-wide target from one eye's basis. **Hazard:** unlike the two already split, it carries a
    /// temporal history it advances per invocation, so re-issuing the whole block double-advances
    /// state that is not idempotent, so the split saves and restores the history indices around the second
    /// run. On by default; turn it off if ambient occlusion looks wrong.
    pub single_pass_ssao_per_eye: bool,
    /// Run the **screen-space reflection** pass once per eye under the collapse.
    ///
    /// Same reconstruction defect as [`single_pass_ssao_per_eye`](Self::single_pass_ssao_per_eye).
    /// **Hazard:** SSR ray-marches a scene-colour capture taken earlier in the frame, so a second run
    /// would consume state the first already consumed -- which it does not: the block copies scene colour
    /// into its own target and writes nothing back, so a second run reproduces the capture. On by
    /// default.
    pub single_pass_ssr_per_eye: bool,
    /// Run the **screen-space subsurface-scattering** (skin) pass once per eye under the collapse.
    ///
    /// Same reconstruction defect. This block calls the inverse **twice**, once per blur axis, so a
    /// per-eye split would have to mask both -- but they are in mutually exclusive branches, so exactly one
    /// fires per draw. On by default.
    pub single_pass_subsurface_per_eye: bool,
    /// Apply the per-eye reconstruction basis to the **depth-of-field** post pass under the collapse.
    ///
    /// `DOFUtil::GetViewProjInverse` is the seventh and last consumer of the reconstruction basis. DoF
    /// is a post pass rather than a render block, so it may want the basis substituted rather than the
    /// pass re-issued: its compute prologue is basis-independent, so it runs once whole-target while the
    /// closing draw is split. On by default.
    pub single_pass_dof_per_eye: bool,
    /// Give **screen-space decals** a per-eye block intercept under the collapse.
    ///
    /// `CRenderBlockSSDecal` is not a `PerspectiveFovInverse` consumer: its type-level `Setup` builds
    /// its own reconstruction basis inline and uploads it to fragment `cb1[0..3]`, and its pixel shader
    /// derives the depth-fetch UV *projectively* from its own clip position — normalized over one eye
    /// while the depth buffer is double-wide. So the decal reconstructs from the wrong surface, and the
    /// error moves with the camera.
    ///
    /// Both halves need fixing together: re-upload that eye's basis per eye, **and** bias the shader's
    /// projective UV into that eye's half of the buffer. Note the same `uv` feeds the reconstruction
    /// matrix (which wants the per-eye value) and the depth fetch (which wants the double-wide one), so
    /// they must stay separate. On by default.
    pub single_pass_ssdecal_per_eye: bool,
    /// Requires [`single_pass_reconstruct_per_eye`](Self::single_pass_reconstruct_per_eye) and
    /// [`fix_clustered_light_frustum`](Self::fix_clustered_light_frustum): build the clustered
    /// (froxel) light grid **per eye** as well, instead of building it once with eye 0's projection
    /// against the double-wide tile count.
    ///
    /// The grid is a 64-pixel tile lattice over the framebuffer; collapsed, it is twice as wide as
    /// the frustum whose projection fills it, so every local light lands in the wrong tiles and the
    /// forward-lit families that sample it (foliage, glass, particles, blended materials -- ~20
    /// render-block types) are lit wrongly in both eyes. The resolve re-issue already runs the whole
    /// block once per eye; this makes each run's light assignment cover only that eye's half of the
    /// tile grid, with that eye's projection and tile bounds.
    ///
    /// The two halves compose because the assignment blend is commutative, the halves are disjoint,
    /// and the compaction phase is per-tile-local -- so long as the second run's whole-target clear
    /// is suppressed, which is what makes the grid end the frame valid for **both** eyes. That
    /// matters well outside the block: foliage samples the grid a frame early, in the G-buffer range.
    ///
    /// Declines itself (leaving the un-split behaviour) when the double-wide render width is not a
    /// multiple of 128, i.e. when the eye seam does not fall on a tile boundary.
    ///
    /// On by default: the un-split grid mislights every forward-lit family, and the flag remains so
    /// the two can be compared in the headset.
    pub single_pass_clustered_per_eye: bool,
    /// Requires [`single_pass_clustered_per_eye`](Self::single_pass_clustered_per_eye): also assign
    /// each eye's lights from **that eye's** position rather than from the collapsed (cyclopean)
    /// camera's.
    ///
    /// The light-assignment vertex shader transforms proxies that the CPU has already made relative
    /// to the render camera's world position, and under the collapse that is the centre head pose --
    /// the per-eye offset lives in the patched vertex shaders, not in the render context. Folding the
    /// eye's world offset into the translation row of the uploaded view matrix restores it. The
    /// per-eye display canting is *not* applied, only the positional offset.
    ///
    /// On by default, as the more faithful assignment. Whether the difference is visible at 64-pixel
    /// tile granularity is a judgement call, which is why it stays its own flag: turn it off to A/B
    /// against [`single_pass_clustered_per_eye`](Self::single_pass_clustered_per_eye) alone.
    pub single_pass_clustered_per_eye_light_view: bool,
    /// Keep both viewport slots bound to the same region outside the G-buffer range, so a patched
    /// vertex shader's `SV_ViewportArrayIndex = SV_InstanceID & 1` resolves to the same place whichever
    /// parity it computes. The rewrite writes that index unconditionally -- the bytecode cannot tell
    /// which pass it is in -- but only the G-buffer geometry ever binds an eye-half pair, and the
    /// collapse's per-draw split leaves the two slots holding *different* halves after the range ends.
    /// Until the next engine viewport bind, every odd-numbered instance of an already-instanced draw
    /// then rasterises into the other half. Slot 0 is never touched, so this can only make dropped
    /// instances reappear, never move what already rendered. Requires the collapse. On by default.
    pub single_pass_uniform_viewport_slots: bool,
    /// Close a leaked G-buffer range from the **draw thread**, in the dispatch prologue
    /// (`RenderEngine::PreDraw`), instead of from the game thread at frame start.
    ///
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
    /// Relax the volumetric-patch terrain's two view-dependent *hull* culls in stereo (issue #40).
    /// The tessellated terrain discards whole patches inside its hull shader: a back-patch cull that
    /// tests every patch's facing against a single direction (the render camera's forward axis, with
    /// only ~17.5 deg of slack), and a frustum cull against the render camera's view-projection baked
    /// once per frame -- so one frustum, and one view axis, serve both eyes. Neither approximation
    /// survives a headset's field of view: patches 45+ deg off-axis that squarely face an eye fall
    /// beyond the facing threshold, and under the single-pass collapse the baked frustum is the
    /// centred camera's 90 deg-vertical one, which an eye can see past vertically (and, with display
    /// cant, laterally). A discarded patch is drawn in no pass and the coarser tile over the same
    /// footprint is separately LOD-clipped, so the gap resolves as a world-locked black patch that
    /// flips with head rotation. This clears the type's two enable flags around its own constant bake
    /// (restoring them immediately after), so only the uploaded constants change. Costs the
    /// tessellation of the margin patches, bounded by the CPU-side patch cull. VR only.
    pub relax_terrain_patch_hull_culls: bool,
    /// Widen the *active camera's* cull frustum to cover both eyes. Model instances re-cull each render
    /// block against the camera manager's active-camera frustum planes (`CModelInstance::AddToRender` ->
    /// `CCamera::IsBoxVisible`), a second gate the scene-cull widen above does not touch -- so large
    /// buildings pop out at the combined-eye edge even with the scene cull widened (see
    /// `docs/engine/model-culling.md`). This detours `Camera::UpdateFrustum` and, for the active camera,
    /// rebuilds its six frustum planes from the union projection (restoring `m_ViewProjection` so
    /// rendering is untouched), which also fixes the instant-hide-instead-of-fade pop, road meshes, and
    /// far lights that read the same planes. Once per frame; VR only.
    pub widen_model_cull: bool,
    /// Widen the spawn system and character occlusion BFBC frustums to cover both eyes. Both
    /// `CSpawnSystem::Update` and `CGameWorldObjectManager::ProcessCharacterOcclusion` build their BFBC
    /// cull frustums against the camera manager's active camera (not the occluder manager's cull camera),
    /// so the scene-cull widen does not reach them. This widens the active camera's projection for those
    /// calls (with a full save/restore so the per-eye render projections are untouched), so the spawn
    /// visibility gate and character occlusion cull account for the combined VR eye frusta. VR only.
    pub widen_spawn_cull: bool,
    /// Scale factor for spawn system budget limits. The spawn system enforces per-resource-def max
    /// object counts (loaded from `settings/spawn_budget_pools.bin`); when the count is exceeded it
    /// despawns the highest-rank object, which under VR's wider FOV can pop vehicles/NPCs in front of
    /// the player. This multiplies every budget entry by the given factor (e.g. `2.0` doubles all
    /// limits) via a one-time patch on first `CSpawnSystem::Update` call (auto-reverted on uninject).
    /// `1.0` leaves them unchanged. Read once at startup; requires restart to change.
    pub spawn_budget_scale: f32,
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
    /// Refresh every sun-shadow cascade every frame, defeating the engine's update-pattern amortisation.
    /// The engine re-fits and re-renders cascade level L only every `2^L` frames (via
    /// `ShadowManager::m_CascadeUpdateLevels`), copying each cascade's fit forward between refreshes, so
    /// the coarse cascades hold still then snap to a new texel alignment periodically. Flatscreen's T2X
    /// averages those snaps away, but the mod forces SMAA 1x (no temporal history), so they surface as
    /// sun-shadow flicker (issue #31). Zeroing the per-cascade levels forces all cascades to update every
    /// frame -- smooth, at the cost of redrawing the coarse cascades each frame. VR only.
    pub shadow_update_every_frame: bool,
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
    /// Diagnostic (issue #31 isolation, Test A): render each eye through a *symmetric* (zero-shear)
    /// off-axis frustum instead of the true asymmetric HMD one. The replacement preserves each eye's
    /// horizontal and vertical FOV *extent* but re-centres it, so the off-centre shear terms
    /// (`(tl+tr)`, `(td+tu)`) go to zero -- reproducing the known-good symmetric-stereo projection
    /// *inside* the full VR path (per-eye offset, double-draw, and the off-axis depth reconstruction all
    /// still run). The flicker occurs only under the asymmetric off-axis projection, not symmetric
    /// stereo, so this is the direct discriminator: if the flicker dies with the shear removed, the
    /// shear is the amplifier; if it survives, the reconstruction inverse (or another projection oddity)
    /// is. The headset image will look slightly stretched (the compositor still composites at the true
    /// asymmetric FOV) -- diagnostic only. VR only; no-op on flatscreen. **Default off.**
    pub symmetrize_eye_frusta: bool,
    /// Diagnostic (issue #31 isolation, Test B): render *both* eyes with eye 0's exact projection and
    /// world offset, so the two dispatches draw the identical off-axis view (mono in stereo). This
    /// removes all per-eye divergence while keeping the off-axis projection and its depth reconstruction,
    /// isolating "the two eyes disagree" from "each single off-axis frame wanders frame to frame": if
    /// the flicker survives two identical off-axis draws, it is not inter-eye divergence at all. VR only;
    /// no-op on flatscreen. **Default off.**
    pub mirror_eye0_to_both: bool,
    /// The multiplier for the terrain detail-tessellation GPU budget buffers — the fix for the
    /// world-locked black cliff-wall/cave-ceiling tiles (issue #40). The detail rock skin is built
    /// by a GPU pipeline that allocates vertices/indices/texels from fixed-size buffers with
    /// unbounded cursors; whatever overflows is silently dropped and the losing tiles render
    /// black. The shipped sizes fit the flatscreen FOV; VR's wide FOV admits more quads and
    /// oversubscribes them. The buffer-size immediates in the setup types' `Create` functions are
    /// patched to `shipped * scale` (auto-reverting on uninject) and the two setup types are
    /// re-created, automatically at the first frame start after injection and on demand from the
    /// debug UI. `1` leaves the shipped sizes (and skips the automatic apply). VR and flatscreen;
    /// **default 4.**
    pub terrain_detail_budget_scale: u32,
    /// Replace the symmetric froxel tile-bounds constants that `DrawClustered` uploads to the
    /// light-assignment fragment shader (cb1) with per-eye off-axis-derived values. The engine
    /// reconstructs a symmetric frustum from the vertical FOV and aspect ratio, which cannot encode
    /// the off-axis shift that VR per-eye projections introduce — so lights are assigned to the wrong
    /// 64-pixel tiles, producing blocky, screen-aligned lighting artifacts. The fix intercepts the
    /// cb1 upload during the light-assignment pass and replaces it with bounds computed from the
    /// per-eye projection matrix. VR only; a no-op when no VR frame is in flight. **Default on.**
    pub fix_clustered_light_frustum: bool,
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
            share_prepasses: true,
            gate_setup_render_frame_data: false,
            gate_hand_back_buffers: false,
            gate_eye1_dt: true,
            drain_draw_fragment: true,
            defer_frame_tail: true,
            fix_shadow_cascade_anchor: true,
            diagnose_stereo_draw_diff: false,
            diagnose_rt_hashes: false,
            diagnose_rt_screenshots: true,
            disable_ssao: false,
            ssao_eye0_only: false,
            restore_cb_ring: false,
            skip_ssr: false,
            skip_gi: false,
            skip_ao_volumes: false,
            skip_pass_range_enabled: false,
            skip_pass_range: (0x56, 0x56),
            restore_ssao_history: true,
            restore_gi_cascade: true,
            patch_shadow_pcf_hash: true,
            patch_lod_dissolve: false,
            single_pass: false,
            single_pass_dual_eye: false,
            single_pass_double_wide: false,
            single_pass_collapse: false,
            single_pass_patch_dryrun: false,
            single_pass_reproject: false,
            single_pass_reproject_camera_only: true,
            single_pass_terrain: false,
            single_pass_tree_impostors: false,
            single_pass_bark: true,
            single_pass_foliage: true,
            single_pass_occluder: false,
            single_pass_instanced_per_eye: true,
            single_pass_indirect_per_eye: true,
            single_pass_reconstruct_per_eye: true,
            single_pass_atmospheric_per_eye: true,
            single_pass_water_uv_per_eye: true,
            single_pass_nvwater_per_eye: true,
            single_pass_ssdecal_geometry_per_eye: true,
            single_pass_slot13_per_eye: true,
            collapse_viewport_follows_target: true,
            single_pass_ssao_per_eye: true,
            single_pass_ssr_per_eye: true,
            single_pass_subsurface_per_eye: true,
            single_pass_dof_per_eye: true,
            single_pass_ssdecal_per_eye: true,
            single_pass_clustered_per_eye: true,
            single_pass_clustered_per_eye_light_view: true,
            single_pass_uniform_viewport_slots: true,
            disable_sun_shadows: false,
            freeze_shadow_maps: false,
            dedupe_post_block: true,
            invalidate_terrain_cb: true,
            reconstruct_offaxis_inverse: true,
            widen_cull_frustum: true,
            cull_fov_padding: 0.4,
            cull_size_fov_deg: 50.0,
            disable_bfbc_occlusion: true,
            widen_terrain_cull: true,
            relax_terrain_patch_hull_culls: true,
            widen_model_cull: true,
            widen_spawn_cull: true,
            spawn_budget_scale: 2.0,
            widen_shadow_fit: true,
            stabilize_shadow_fit: true,
            shadow_update_every_frame: false,
            fog_full_res: false,
            particles_full_res: false,
            spotlight_full_res: false,
            symmetrize_eye_frusta: false,
            mirror_eye0_to_both: false,
            fix_clustered_light_frustum: true,
            terrain_detail_budget_scale: 4,
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

/// The monoscopic far field (issue #32): partition each scene pass's sorted draw list into near
/// and far runs at a distance threshold, using the engine's own depth-bucket sort machinery, so
/// the far run can be skipped for dial-in today and rendered once and shared between the eyes
/// later. Off by default; see `payload/src/far_field.rs`.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FarFieldConfig {
    /// Master toggle: register the depth-bucket boundary on the passes in range and compute the
    /// near/far split (and the UI counters) each draw. Off restores each pass's stock single
    /// bucket. Off by default: `Share` mode currently produces severe compositing artifacts in
    /// VR (issue pending) and is opt-in until that is resolved.
    pub enabled: bool,
    /// The near/far boundary in metres (instance-centre distance to the sort camera). Large
    /// objects whose centre sits beyond this but whose extent reaches nearer are classified far,
    /// so keep it conservative.
    pub threshold_m: f32,
    /// The render-block type names (registry names, comma-separated) gated as inherently
    /// far-regime: their draws are skipped whenever the mode skips far, with no distance split.
    /// The volumetric terrain patches draw only distant terrain (near terrain hands off to other
    /// types as the camera approaches), and the tree impostors are the distant-tree
    /// representation; find further candidates with the Diagnostics tab's registry bisect.
    /// Under `Share`, gated types render only in the far dispatch's G-buffer range, so only
    /// opaque G-buffer types belong here — a transparent type (e.g. `Window`, the car/building
    /// glass) would vanish entirely, since its passes never run in the far dispatch.
    pub gated_types: String,
    /// What to do with the partition.
    pub mode: FarFieldMode,
}
impl FarFieldConfig {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            threshold_m: 250.0,
            gated_types: String::new(),
            mode: FarFieldMode::Share,
        }
    }
}
impl Default for FarFieldConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The default far-regime gated type list (validated by registry bisect: the volumetric terrain
/// patches and tree impostors/forest draw only the distant scene). Applied to
/// [`FarFieldConfig::gated_types`] at startup, since the const constructor cannot own a string.
pub const DEFAULT_FAR_FIELD_GATED_TYPES: &str =
    "VolumetricTerrainPatch, TreeImpostor, TerrainForest, Occluder";

/// What the far-field split does with the partition each draw.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum FarFieldMode {
    /// Compute the split and counters only; draw everything as stock.
    Collect,
    /// Skip the far run on both eyes: shows exactly what the threshold classifies as far (it
    /// vanishes), and measures the per-eye cost of the far field.
    SkipFar,
    /// Skip the near run on both eyes: shows the far field in isolation.
    SkipNear,
    /// Skip the far run on the second eye only: the sharing candidate's cost saving, with eye 1
    /// showing holes where the shared far field would composite.
    SkipFarEye1,
    /// The far-field share (issue #32 increment 2): a third, far-only dispatch renders the far field once
    /// at eye 0's pose; both near dispatches composite its captured G-buffer and render near-only
    /// on top. Requires stereo; falls back to `Collect` behaviour otherwise.
    Share,
}

/// Static foveated rendering (issue #29): a radial stencil mask drops a dithered fraction of the
/// peripheral pixels before the expensive scene passes shade them, and a fill-in pass reconstructs them
/// from their neighbours. On by default (validated in-headset); costs the mask + fill passes but saves
/// peripheral shading. See `docs/mod/foveation.md`.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FoveationConfig {
    /// Master toggle.
    pub enabled: bool,
    /// Foveal radius as a fraction of the half-diagonal from the per-eye foveal centre: inside this the
    /// scene renders at full rate (no pixels dropped).
    pub inner_fraction: f32,
    /// Radius (same units as [`inner_fraction`](Self::inner_fraction)) at which the peripheral drop
    /// reaches [`max_drop`](Self::max_drop); the drop ramps from zero at the inner radius to the max here.
    pub outer_fraction: f32,
    /// The maximum fraction of peripheral pixels dropped (and reconstructed), reached at
    /// [`outer_fraction`](Self::outer_fraction) and beyond. `0.5` drops half the far periphery.
    pub max_drop: f32,
    /// The stencil bit the mask tags dropped pixels with. Default `0x80` (bit 7): the engine's own
    /// stencil use is bits 0, 1, 5, 6, so bit 7 is free; change it if an effect corrupts with foveation
    /// on (a data-driven pass may write it).
    pub mask_bit: u32,
    /// The first [`RenderPassId`](jc3gi::graphics_engine::render_engine::RenderPassId) (inclusive) of the
    /// foveated shading range: the mask-write runs just before it, and the peripheral stencil test is
    /// forced on from here. Default `0x41` (`RP_MODELS_DYNAMIC`) -- after the depth prepass, so the
    /// dropped pixels keep full-resolution depth. A tuning knob; widen it toward the lighting passes to
    /// save more, narrow it if an effect misbehaves.
    pub foveal_first_pass: u32,
    /// The last [`RenderPassId`](jc3gi::graphics_engine::render_engine::RenderPassId) (inclusive) of the
    /// foveated shading range: the peripheral stencil test is forced through it, and the fill-in runs just
    /// after. Default `0x4B` (`RP_CREATURES`).
    pub foveal_last_pass: u32,
    /// Diagnostic: paint the dropped peripheral pixels magenta (in the fill-in pass) instead of
    /// reconstructing them, so the mask is directly visible. Off by default.
    pub debug_show_mask: bool,
}
impl FoveationConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            inner_fraction: 0.35,
            outer_fraction: 0.55,
            max_drop: 0.8,
            mask_bit: 0x80,
            foveal_first_pass: 0x41,
            foveal_last_pass: 0x4B,
            debug_show_mask: false,
        }
    }
}
impl Default for FoveationConfig {
    fn default() -> Self {
        Self::new()
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
    /// Suppress Rico's periodic idle fidget for the local player -- the weight-shifts and
    /// look-arounds the game plays while standing still. The idle input task
    /// (`NStateTask_InputLocoIdleTask`) queues `ACT_TO_IDLE_ONE_OFF` on an idle timer to drive the
    /// `S_IDLE` -> `S_IDLE_ONE_OFF` variation; with the head driven by the HMD and the body meant to
    /// hold the player's real pose, that motion reads as the body drifting on its own (issue #33).
    /// The act is dropped at `Character::QueueAct` for the local player, so Rico stays in the base
    /// `S_IDLE`; NPCs keep their fidgets. See `crate::hooks::character`.
    pub suppress_idle_fidget: bool,
    /// Suppress the idle *breathing* for the local player -- the subtle chest/shoulder motion the
    /// base idle clip (`S_IDLE`) plays while standing still, distinct from the periodic
    /// [`suppress_idle_fidget`](Self::suppress_idle_fidget) variations. Unlike the fidget there is no
    /// act to drop; instead the animation-clock advance is held (`dt = 0`) for the local player's
    /// controller while it is in `S_IDLE`, so the pose freezes at its current frame. Movement and
    /// every other state run at normal speed, and NPCs are untouched. See `crate::hooks::animation`.
    /// **Off by default** pending in-headset validation -- a held pose is a bigger visual change than
    /// dropping the fidget, and the freeze wants eyes-on before it ships on.
    pub suppress_idle_breathing: bool,
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
            suppress_idle_fidget: true,
            suppress_idle_breathing: false,
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
