//! The Debug tab: the render-trace dump, the stereo render fixes, the per-eye diagnostics/bisection
//! levers, and the engine post-FX gates. Only locks CONFIG. (FSR lives in the Render tab.)

use std::{
    collections::BTreeSet,
    sync::{
        Mutex,
        atomic::{AtomicI32, Ordering},
    },
};

use jc3gi::graphics_engine::{
    render_block::{
        RenderBlockTypeTerrain as BaseTerrain, RenderBlockTypeTerrainPatch as TerrainPatch,
    },
    render_engine::RenderBlockTypeRegistry,
};

use crate::{config, debug::trace};

/// The frame count for the editable "Dump N frames" trace button, persisted across UI frames.
static TRACE_FRAME_COUNT: AtomicI32 = AtomicI32::new(60);

pub fn egui_debug_debug(ui: &mut egui::Ui) {
    let mut cfg = config::CONFIG.lock();
    // Deferred so the trace button's start() doesn't re-lock CONFIG (it snapshots the config for the
    // manifest) while this guard is held -- parking_lot is not reentrant, so that self-deadlocks.
    let mut start_trace: Option<i32> = None;

    // Render trace at the top -- it dumps the next few frames' render events under whatever every
    // option below is set to (writes next to the DLL). `diagnose_rt_hashes` adds per-eye render-target
    // hashes to the dump. The long capture exists for periodic artifacts whose cadence exceeds the
    // short window (e.g. the one-frame exposure/shadow pulses of issue #10).
    ui.horizontal(|ui| {
        if ui.button("Dump render trace (4 frames)").clicked() {
            start_trace = Some(4);
        }
        let mut count = TRACE_FRAME_COUNT.load(Ordering::Relaxed);
        if ui.button(format!("Dump {count} frames")).clicked() {
            start_trace = Some(count);
        }
        if ui
            .add(egui::DragValue::new(&mut count).range(1..=600).speed(1))
            .changed()
        {
            TRACE_FRAME_COUNT.store(count, Ordering::Relaxed);
        }
        let remaining = trace::active_frames();
        if remaining > 0 {
            ui.label(format!("tracing... {remaining} frames left"));
        } else if trace::TraceState::last_path().is_some() {
            ui.label("dumped");
        } else {
            ui.label("(writes to traces/<stamp>/)");
        }
    });
    ui.checkbox(
        &mut cfg.stereo.diagnose_rt_hashes,
        "Hash engine RTs per eye into the trace (run with cameras off)",
    );
    ui.checkbox(
        &mut cfg.stereo.diagnose_rt_screenshots,
        "Dump per-eye frames into the trace (BackBufferLinear PNG per eye per frame)",
    );
    ui.separator();

    // The #31 flicker-isolation A/B levers -- all default off; enable one at a time to localize the
    // whole-terrain sun-shadow flicker. See `crate::config::StereoConfig`.
    egui::CollapsingHeader::new("Flicker isolation (#31)")
        .default_open(false)
        .show(ui, |ui| {
            ui.checkbox(
                &mut cfg.stereo.symmetrize_eye_frusta,
                "A: Symmetric eye frusta (zero shear, same FOV; if flicker dies, the off-axis shear is \
                 the amplifier)",
            )
            .on_hover_text(
                "Renders each eye through a symmetric (centred) frustum instead of the true asymmetric \
                 off-axis one, keeping the per-eye offset, double-draw, and depth reconstruction. The \
                 headset image will look slightly stretched -- diagnostic only.",
            );
            ui.checkbox(
                &mut cfg.stereo.mirror_eye0_to_both,
                "B: Both eyes = eye 0 (identical off-axis view twice; if flicker survives, it isn't \
                 inter-eye divergence)",
            )
            .on_hover_text(
                "Renders both eyes with eye 0's projection and offset (mono), removing all per-eye \
                 divergence while keeping the off-axis projection and its reconstruction.",
            );
            ui.checkbox(
                &mut cfg.stereo.freeze_render_camera,
                "C: Freeze render camera (pins m_TransformF/m_View; splits sun-driven vs camera-idle \
                 flicker)",
            )
            .on_hover_text(
                "Pins the game render camera to the pose captured when enabled, so the camera holds \
                 still while the sun keeps moving. Unlike Freeze pose (VR tab), this freezes the actual \
                 engine camera the shadow cascade fits from. The view locks in place -- diagnostic only.",
            );
        });
    ui.separator();

    // The stereo render corrections, grouped by subsystem -- normally on; toggle off to reproduce the
    // artifact each fixes. Collapsed by default to keep the tab scannable.
    ui.collapsing("Shadows", |ui| {
        ui.checkbox(
            &mut cfg.stereo.fix_shadow_cascade_anchor,
            "Cascade anchor (the visible per-eye shadow mismatch; A/B via Present eye 0)",
        );
        ui.checkbox(
            &mut cfg.stereo.widen_shadow_fit,
            "Widen fit FOV (cascades cover both eyes; fixes distant per-eye shadow disagreement + \
             crawl)",
        );
        ui.checkbox(
            &mut cfg.stereo.stabilize_shadow_fit,
            "Stabilize fit vs head tilt (yaw-only cascade centre; fixes shadows shifting/scaling \
             when you look around)",
        );
        ui.checkbox(
            &mut cfg.stereo.shadow_update_every_frame,
            "Update all cascades every frame (defeats 2^L amortization; #31 parity-flip probe)",
        )
        .on_hover_text(
            "Zeroes m_CascadeUpdateLevels so cascades 1-5 refresh every frame like cascade 0, instead \
             of on the amortized schedule. Diagnostic for the #31 parity ping-pong: capture at native \
             rate and check whether the ShadowState scale_blend.y 6<->1 flip flattens.",
        );
    });

    ui.collapsing("Depth reconstruction", |ui| {
        ui.checkbox(
            &mut cfg.stereo.reconstruct_offaxis_inverse,
            "Off-axis depth reconstruction (per-eye inverse for deferred/SS passes; fixes \
             specular/SSR/shadow reconstruction divergence)",
        );
    });

    ui.collapsing("Clustered lighting", |ui| {
        ui.checkbox(
            &mut cfg.stereo.fix_clustered_light_frustum,
            "Off-axis froxel tile bounds (replaces symmetric cb1 with per-eye projection-derived \
             bounds; fixes blocky 64px lighting tiles in VR)",
        );
    });

    ui.collapsing("Cross-eye consistency", |ui| {
        ui.checkbox(
            &mut cfg.stereo.dedupe_post_block,
            "Dedupe world post block (eye 1 otherwise runs the post chain + FSR twice)",
        );
        ui.checkbox(
            &mut cfg.stereo.drain_draw_fragment,
            "Drain draw-dispatch fragment between eyes (open-world crash fix)",
        );
        ui.checkbox(
            &mut cfg.stereo.restore_frame_counters,
            "Restore frame counters between eyes (fixes jitter/parity flicker)",
        );
        ui.add_enabled(
            cfg.stereo.restore_frame_counters,
            egui::Checkbox::new(
                &mut cfg.stereo.share_prepasses,
                "Share view-independent pre-passes across eyes (reflections, cloud shadows, \
                 sun-shadow atlas, water sim rendered once)",
            ),
        )
        .on_hover_text(
            "On eye 1, reuse eye 0's shadow atlas / reflection proxies / water sim instead of \
             re-rendering them. Requires 'Restore frame counters'. If distant reflections or \
             shadows look wrong in one eye, turn this off.",
        );
        ui.checkbox(
            &mut cfg.stereo.force_smaa_1x,
            "Force SMAA 1x (T2X's shared history ghosts across eyes)",
        );
        ui.checkbox(
            &mut cfg.stereo.force_ssao_first_pass,
            "Force SSAO first-pass per eye (stops cross-eye AO history blend)",
        );
        ui.checkbox(
            &mut cfg.stereo.restore_ssao_history,
            "Restore SSAO history between eyes (pin the AO temporal slot so both eyes match)",
        );
        ui.checkbox(
            &mut cfg.stereo.restore_gi_cascade,
            "Restore GI cascade between eyes (pin the LPV cascade so both eyes match)",
        );
    });

    ui.collapsing("Culling & geometry", |ui| {
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut cfg.stereo.widen_cull_frustum,
                "Widen scene cull frustum (covers both eyes; stops outer-edge void/pop-in)",
            );
            ui.add_enabled(
                cfg.stereo.widen_cull_frustum,
                egui::Slider::new(&mut cfg.stereo.cull_fov_padding, 0.0..=0.75)
                    .text("pad")
                    .fixed_decimals(2),
            )
            .on_hover_text(
                "Extra fraction to widen the cull frustum on every side (incl. vertical); raise if \
                 geometry still pops in at the edges when flying",
            );
        });
        ui.add(
            egui::Slider::new(&mut cfg.stereo.cull_size_fov_deg, 0.0..=90.0)
                .text("Size-cull FOV (deg)")
                .fixed_decimals(0),
        )
        .on_hover_text(
            "FOV the screen-space size cull uses (overrides the injected 90 deg on the cull \
             camera); lower keeps more small/distant geometry and vehicle parts. 0 = leave alone",
        );
        ui.checkbox(
            &mut cfg.stereo.disable_bfbc_occlusion,
            "Disable software occlusion (drops centre-viewpoint occluder culling; fixes peripheral \
             culling an offset eye can see past)",
        );
        ui.checkbox(
            &mut cfg.stereo.widen_terrain_cull,
            "Widen terrain patch cull (rebuild the cull frustum planes; fixes terrain patch holes \
             at the edges when flying)",
        );
        ui.checkbox(
            &mut cfg.stereo.widen_model_cull,
            "Widen model cull (active-camera frustum; fixes buildings popping at the edges)",
        );
        ui.checkbox(
            &mut cfg.stereo.invalidate_terrain_cb,
            "Invalidate terrain tess CB between eyes (forces eye 1 to re-upload its own off-axis \
             projection; fixes distant tessellated terrain sheared to eye 0)",
        );
    });

    // Live-writes the engine's own CRenderBlockTypeTerrainPatch debug flags (not config, not saved) to
    // bisect which patch cull blackens tessellated cliff patches in VR: a patch culled only in the
    // color pass keeps its Z-prepass depth but writes no G-buffer, so deferred lighting resolves it to
    // flat black. Back-patch culling is the prime suspect -- it keys off the shared render camera's
    // forward vector, so it culls identically in both eyes and only past a facing threshold ("at
    // incidental angles"). Flags persist until the terrain type is recreated (level reload).
    ui.collapsing("Terrain patch culling (engine, live)", |ui| {
        match unsafe { TerrainPatch::get() } {
            Some(patch) => {
                // Sanity/discriminator: NoDraw stops every terrain-patch draw. If the cliffs (and the
                // black patches with them) vanish, the writes land and the black patches are this
                // render block. If the cliffs vanish but the black patches remain, the black is a
                // different render block or a deferred artifact, not the patch shader.
                let mut no_draw = patch.m_NoDraw;
                if ui
                    .checkbox(
                        &mut no_draw,
                        "No draw (stop ALL terrain-patch draws — sanity/discriminator test)",
                    )
                    .changed()
                {
                    patch.m_NoDraw = no_draw;
                }
                let mut show_material = patch.m_ShowMaterial;
                if ui
                    .checkbox(
                        &mut show_material,
                        "Show material (swaps the patch fragment program; does the black change?)",
                    )
                    .changed()
                {
                    patch.m_ShowMaterial = show_material;
                }
                let mut back = patch.m_EnableBackPatchCulling;
                if ui
                    .checkbox(
                        &mut back,
                        "Back-patch culling (shared view dir; prime suspect for both-eye black \
                         patches)",
                    )
                    .changed()
                {
                    patch.m_EnableBackPatchCulling = back;
                }
                let mut frustum = patch.m_EnableFrustumPatchCulling;
                if ui
                    .checkbox(
                        &mut frustum,
                        "Frustum patch culling (baked off-axis view-projection)",
                    )
                    .changed()
                {
                    patch.m_EnableFrustumPatchCulling = frustum;
                }
                let mut detail = patch.m_EnableCullByDetail;
                if ui.checkbox(&mut detail, "Cull by detail").changed() {
                    patch.m_EnableCullByDetail = detail;
                }
                let mut show = patch.m_ShowDebugCulling;
                if ui
                    .checkbox(
                        &mut show,
                        "Show debug culling (rasterize culled patches with CULLFACE_NONE)",
                    )
                    .changed()
                {
                    patch.m_ShowDebugCulling = show;
                }
                ui.add(
                    egui::Slider::new(&mut patch.m_BackPatchCullThreshold, -1.0..=1.0)
                        .text("Back-patch threshold")
                        .fixed_decimals(3),
                );
                ui.add(
                    egui::Slider::new(&mut patch.m_DebugMode, 0..=5)
                        .text("Debug mode (0 = normal shading)"),
                )
                .on_hover_text(
                    "Selects an engine debug fragment program (LOD colours, tessellation overlays); \
                     capped conservatively to stay in the shader array",
                );
            }
            None => {
                ui.label("Terrain patch render type not initialized yet.");
            }
        }
    });

    // The base VolumetricTerrain block (CRenderBlockTerrain) -- dormant in the retail world (the live
    // terrain is the volumetric-patch system + the TerrainDetail rock skin), so these engine flags are
    // inert there; kept for other worlds. The #40 detail-budget controls sit at the bottom of this
    // section. Live engine flags, not saved; reset on level reload.
    ui.collapsing("Base terrain culling (walls; engine, live)", |ui| {
        match unsafe { BaseTerrain::get() } {
            Some(terrain) => {
                let mut back = terrain.m_EnableBackPatchCulling;
                if ui
                    .checkbox(
                        &mut back,
                        "Back-patch culling (shared view dir; PRIME suspect for black wall tiles)",
                    )
                    .changed()
                {
                    terrain.m_EnableBackPatchCulling = back;
                }
                let mut frustum = terrain.m_EnableFrustumPatchCulling;
                if ui
                    .checkbox(
                        &mut frustum,
                        "Frustum patch culling (baked off-axis view-projection)",
                    )
                    .changed()
                {
                    terrain.m_EnableFrustumPatchCulling = frustum;
                }
                let mut detail = terrain.m_EnableCullByDetail;
                if ui.checkbox(&mut detail, "Cull by detail").changed() {
                    terrain.m_EnableCullByDetail = detail;
                }
                let mut show = terrain.m_ShowDebugCulling;
                if ui
                    .checkbox(
                        &mut show,
                        "Show debug culling (rasterize culled patches with CULLFACE_NONE)",
                    )
                    .changed()
                {
                    terrain.m_ShowDebugCulling = show;
                }
                ui.add(
                    egui::Slider::new(&mut terrain.m_BackPatchCullThreshold, -1.0..=1.0)
                        .text("Back-patch threshold")
                        .fixed_decimals(3),
                );
                ui.separator();
                ui.label(
                    "Tessellation factors -- if steep/grazing tiles collapse to zero tess (holes) \
                     under the wide VR FOV, raising Edge / lowering MinSpacing should refill them:",
                );
                ui.add(
                    egui::Slider::new(&mut terrain.m_TessellationFactorEdge, 0.0..=64.0)
                        .text("Edge")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut terrain.m_TessellationFactorInner, 0.0..=64.0)
                        .text("Inner")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut terrain.m_TessellationFactorMinSpacing, 0.1..=64.0)
                        .text("MinSpacing (smaller = more tess)")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut terrain.m_TessellationFactorSphere, 0.0..=64.0)
                        .text("Sphere")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut terrain.m_TessellationFactorNormalDiff, 0.0..=64.0)
                        .text("NormalDiff")
                        .fixed_decimals(2),
                );
            }
            None => {
                ui.label("Base terrain render type not initialized yet.");
            }
        }
        ui.separator();
        ui.label(
            "Detail budget (the #40 black-tile fix: the GPU detail pipeline drops whatever \
             overflows its fixed vertex/index/texel buffers, and VR's wide FOV oversubscribes \
             them -- the losing tiles render black). Applied automatically at startup:",
        );
        ui.horizontal(|ui| {
            ui.add(
                egui::Slider::new(&mut cfg.stereo.terrain_detail_budget_scale, 1..=16)
                    .text("budget scale"),
            );
            if ui
                .button("Re-apply (patch sizes + recreate setup buffers)")
                .clicked()
            {
                crate::hooks::graphics_engine::terrain::request_detail_budget_apply();
            }
        });
        ui.checkbox(
            &mut cfg.stereo.force_terrain_hull_clip,
            "Force the water-clip hull type (type 2; ruled out for #40)",
        );
        ui.add(
            egui::Slider::new(&mut cfg.stereo.terrain_hull_clip_value, 0..=2)
                .text("Replacement clip type for type 2"),
        )
        .on_hover_text(
            "Clip type 2 is the below-water discard for base-LOD tiles when the camera is above \
             water -- not the LOD clip",
        );
    });

    // The engine's render-block-type registry: every registered type by name, with its engine-native
    // enable flag (CRenderPass::DoDraw skips disabled types in every pass). The definitive bisect for
    // "which render block draws this surface": disable types one at a time until the surface vanishes.
    ui.collapsing(
        "Render block types (engine registry; disable to bisect)",
        |ui| {
            show_render_block_type_registry(ui);
        },
    );

    ui.collapsing("Shader patches", |ui| {
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut cfg.stereo.patch_shadow_pcf_hash,
                "Sun-shadow PCF screen-hash (kills per-eye shimmer + foliage grain)",
            );
            let patched = crate::hooks::graphics_engine::shader::patched_count();
            ui.label(if patched == 0 {
                "(0 patched -- click Reload shaders)".to_string()
            } else {
                format!("({patched} sites patched)")
            });
        });
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut cfg.stereo.patch_lod_dissolve,
                "Jitter-unstable LOD dissolve (only matters with FSR jitter on)",
            );
            let patched = crate::hooks::graphics_engine::shader::dissolve_patched_count();
            ui.label(if patched == 0 {
                "(0 patched -- click Reload shaders)".to_string()
            } else {
                format!("({patched} sites patched)")
            });
        });
        ui.horizontal(|ui| {
            if ui.button("Reload shaders").clicked() {
                crate::hooks::graphics_engine::shader::request_reload();
            }
            ui.label(
                "re-creates all shaders so the shader patches take effect (F11 toggles + reloads)",
            );
        });
    });

    // Resolution levers for issue #8's pixelation/large-tile artifact around lights and explosions:
    // the engine's reduced-resolution fog/particle/spotlight passes, whose coarse grids VR's wide FOV
    // magnifies. All default off (not headset-verifiable; particles can hide content).
    ui.collapsing("Resolution (pixelation)", |ui| {
        ui.checkbox(
            &mut cfg.stereo.fog_full_res,
            "Fog volume full-res (coarse froxel depth buffer; applies at next resolution change)",
        )
        .on_hover_text(
            "No-ops the half-res multiplies in the fog block's ResizeTextures so the coarse \
             volumetric-depth buffer is recreated at full resolution. Most likely fix for the \
             light/explosion tiles. Only re-runs on a resolution change.",
        );
        ui.checkbox(
            &mut cfg.stereo.particles_full_res,
            "Particles full-res (route to the full-res transparent pass) -- RISKY, A/B live",
        )
        .on_hover_text(
            "Clears the particle block type's low-res routing flags so particles draw in the \
             full-res transparent pass. The full-res pass always draws, so particles reroute rather \
             than vanish -- but verify live: a family that does not survive the reroute could look \
             wrong. Applies one frame ahead.",
        );
        ui.checkbox(
            &mut cfg.stereo.spotlight_full_res,
            "Spotlight volumetrics full-res (engine's full-res branch)",
        )
        .on_hover_text(
            "Scopes g_EnableLowResSpotLightVolume off around the light gather so spot-light cones \
             render at full resolution into the main setup. Lowest-risk lever.",
        );
    });

    // Investigation levers -- normally off; used to isolate what differs between the eyes.
    ui.collapsing("Per-eye diagnostics", |ui| {
        ui.checkbox(
            &mut cfg.stereo.present_eye_0,
            "Present eye 0 (else eye 1) -- flip to compare each eye live",
        );
        ui.checkbox(
            &mut cfg.stereo.skip_ssr,
            "Skip SSR (drops screen-space reflections; tests the per-eye prev-scene feedback)",
        );
        ui.checkbox(
            &mut cfg.stereo.skip_gi,
            "Skip GI (drops global illumination; isolates the residual per-eye MainColor divergence)",
        );
        ui.checkbox(
            &mut cfg.stereo.skip_ao_volumes,
            "Skip AO volumes (depth-tested darkening volumes; suspect for the blob shadow flicker)",
        );
        ui.checkbox(
            &mut cfg.stereo.disable_sun_shadows,
            "Disable sun shadows (engine SetEnabled path; does the flicker survive with none?)",
        );
        ui.checkbox(
            &mut cfg.stereo.freeze_shadow_maps,
            "Freeze shadow maps (atlas keeps last contents; dies = content, survives = sampling)",
        );
        ui.horizontal(|ui| {
            ui.checkbox(&mut cfg.stereo.skip_pass_range_enabled, "Skip pass range");
            // The bounds stay editable while disarmed, so a target range can be preset and then
            // armed in one step instead of dragging through unsafe intermediate ranges live.
            let (lo, hi) = &mut cfg.stereo.skip_pass_range;
            // Each end clamps to the other so the range can never invert mid-drag.
            let hi_cap = *hi;
            ui.add(egui::DragValue::new(lo).range(0..=hi_cap).hexadecimal(2, false, false));
            ui.label("..=");
            let lo_cap = *lo;
            ui.add(egui::DragValue::new(hi).range(lo_cap..=156).hexadecimal(2, false, false));
        })
        .response
        .on_hover_text(
            "Bisect which render pass an artifact originates in (inclusive hex indices; \
             jc3gi's RenderPassId maps every index)",
        );
        ui.checkbox(
            &mut cfg.stereo.disable_ssao,
            "Disable SSAO (does the 'stronger in one eye' darkening vanish?)",
        );
        ui.checkbox(
            &mut cfg.stereo.ssao_eye0_only,
            "SSAO on eye 0 only (drop the second eye's screen AO)",
        );
        ui.checkbox(
            &mut cfg.stereo.restore_cb_ring,
            "Restore CB ring between eyes (pin RenderEngine +0x16C0; both eyes share CB slots)",
        );
    });

    ui.collapsing("Eye-1 gates (skip on second Draw)", |ui| {
        ui.checkbox(
            &mut cfg.exposure.gate,
            "Auto-exposure (SmoothedExposure + Histogram)",
        );
        ui.checkbox(
            &mut cfg.stereo.gate_eye1_dt,
            "Eye-1 dt=0 (world fade / sun / heat-haze step once per frame)",
        );
        ui.checkbox(
            &mut cfg.stereo.gate_setup_render_frame_data,
            "SetupRenderFrameData (per-batch list build, not the swap)",
        );
        ui.checkbox(
            &mut cfg.stereo.gate_hand_back_buffers,
            "HandBackBuffers (constant-buffer recycle)",
        );
    });

    ui.collapsing("Exposure A/B (pin m_CurrentExposure)", |ui| {
        ui.checkbox(
            &mut cfg.exposure.force,
            "Force exposure (pin after the engine's Update)",
        );
        ui.add(
            egui::Slider::new(&mut cfg.exposure.forced_value, 0.0..=0.5)
                .text("Forced exposure (~0.11 = non-stereo daylight)"),
        );
        ui.label(
            "A/B: enable in both stereo and non-stereo at the same value. Same brightness => the \
             darkening was the exposure loop; stereo still darker => a render path.",
        );
    });

    ui.collapsing("Foveation (#29, experimental)", |ui| {
        ui.checkbox(
            &mut cfg.foveation.enabled,
            "Enable static foveated rendering",
        );
        ui.add(
            egui::Slider::new(&mut cfg.foveation.inner_fraction, 0.0..=1.0)
                .text("Inner radius (fraction of half-diagonal, full-res inside)"),
        );
        ui.add(
            egui::Slider::new(&mut cfg.foveation.outer_fraction, 0.0..=1.5)
                .text("Outer radius (drop reaches max here)"),
        );
        ui.add(
            egui::Slider::new(&mut cfg.foveation.max_drop, 0.0..=1.0)
                .text("Max peripheral drop fraction"),
        );
        ui.horizontal(|ui| {
            ui.label("Foveated pass range (RenderPassId):");
            ui.add(egui::DragValue::new(&mut cfg.foveation.foveal_first_pass).range(0..=0xFF));
            ui.label("..=");
            ui.add(egui::DragValue::new(&mut cfg.foveation.foveal_last_pass).range(0..=0xFF));
        });
        ui.checkbox(
            &mut cfg.foveation.debug_show_mask,
            "Debug: paint dropped pixels magenta (visualize the mask)",
        );
        ui.label(
            "Drops a dithered radial fraction of peripheral pixels before shading, then reconstructs \
             them. Off by default; needs in-headset tuning.",
        );
    });

    ui.collapsing("Post-FX (reprojection passes, both eyes)", |ui| {
        ui.checkbox(
            &mut cfg.post_fx.skip_motion_blur,
            "Skip MotionBlur::Apply (whole pass)",
        );
        ui.checkbox(
            &mut cfg.post_fx.skip_motion_blur_recon,
            "Skip MotionBlur recon (if pass not skipped)",
        );
        ui.checkbox(
            &mut cfg.post_fx.dof_no_reproject,
            "DoF: plain composite, no reprojection (keeps picture)",
        );
        ui.checkbox(
            &mut cfg.post_fx.skip_dof,
            "Skip DepthOfField::Apply (washes out!)",
        );
    });

    ui.collapsing("Post-FX stages (skip to bisect)", |ui| {
        ui.checkbox(
            &mut cfg.post_fx.skip_histogram,
            "Exposure histogram (stalls auto-exposure)",
        );
        ui.checkbox(&mut cfg.post_fx.skip_glare, "Glare / bloom");
        ui.checkbox(&mut cfg.post_fx.skip_fade, "Fade");
        ui.checkbox(&mut cfg.post_fx.skip_sun_halo, "Sun halo");
        ui.checkbox(
            &mut cfg.post_fx.skip_player_damage,
            "Player-damage vignette",
        );
    });

    // Release CONFIG before start(), whose manifest snapshot re-locks it.
    drop(cfg);
    if let Some(frames) = start_trace {
        trace::TraceState::start(frames);
    }
}

/// A vtable-compatible `IRenderBlockType::IsEnabled` override that always reports disabled.
/// `CRenderPass::DoDraw` calls the virtual per type run, so pointing a type's vtable slot here
/// stops its draws in every pass.
unsafe extern "system" fn render_block_type_always_disabled(
    _this: *mut ::core::ffi::c_void,
) -> bool {
    false
}

/// The `IsEnabled` vtable slots currently patched to
/// [`render_block_type_always_disabled`], keyed by slot address.
static DISABLED_TYPE_SLOTS: Mutex<BTreeSet<usize>> = Mutex::new(BTreeSet::new());

/// Render the engine render-block-type registry: one row per registered type, name from the type's
/// own `GetTypeName`, and a checkbox that patches the type's `IsEnabled` vtable slot to a
/// return-false stub (restored on re-tick or uninject). The retail build compiles the engine's own
/// `Enable`/`Disable` to no-ops and `IsEnabled` to `return true`, but `CRenderPass::DoDraw` still
/// dispatches `IsEnabled` through the vtable, so the slot patch is the working kill switch.
fn show_render_block_type_registry(ui: &mut egui::Ui) {
    // SAFETY: the registry is static engine storage; entries are live type singletons registered at
    // startup and only removed at shutdown. The vtable slot write is an aligned qword store through
    // the patcher while the render thread may read it -- acceptable for a diagnostic toggle.
    unsafe {
        let Some(reg) = RenderBlockTypeRegistry::get() else {
            ui.label("registry not reachable");
            return;
        };
        let entries = reg.as_slice();
        if entries.is_empty() || entries.len() > 256 {
            ui.label(format!(
                "registry not initialized or invalid ({} entries)",
                entries.len()
            ));
            return;
        }
        ui.label(format!(
            "{} registered types; unticking disables the type's draws in every pass \
             (vtable IsEnabled patch; auto-reverts on uninject)",
            entries.len()
        ));
        let mut disabled_slots = DISABLED_TYPE_SLOTS.lock().unwrap();
        egui::ScrollArea::vertical()
            .id_salt("render_block_type_registry")
            .max_height(240.0)
            .show(ui, |ui| {
                for entry in entries {
                    let Some(ty) = entry.m_Type.as_mut() else {
                        continue;
                    };
                    let name = ty.get_type_name_str().unwrap_or("(unnamed)");
                    // The patch target: the address of the vtable's `IsEnabled` entry, with the
                    // field offset taken from the generated vftable type.
                    let slot = (&raw const (*ty.vftable()).IsEnabled) as usize;
                    let mut enabled = !disabled_slots.contains(&slot);
                    if ui
                        .checkbox(&mut enabled, format!("{name} ({:#010x})", entry.m_Hash))
                        .changed()
                    {
                        let Some(mut patcher) = crate::hooks::patcher() else {
                            continue;
                        };
                        if enabled {
                            patcher.unpatch(slot);
                            disabled_slots.remove(&slot);
                        } else {
                            let stub = render_block_type_always_disabled as *const () as usize;
                            patcher.patch(slot, &stub.to_le_bytes());
                            disabled_slots.insert(slot);
                        }
                    }
                }
            });
    }
}
