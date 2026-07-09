//! The Debug tab: the render-trace dump, the stereo render fixes, the per-eye diagnostics/bisection
//! levers, and the engine post-FX gates. Only locks CONFIG. (FSR lives in the Render tab.)

use crate::{config, debug::trace};

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
        if ui.button("120 frames").clicked() {
            start_trace = Some(120);
        }
        let remaining = trace::active_frames();
        if remaining > 0 {
            ui.label(format!("tracing... {remaining} frames left"));
        } else if trace::TraceState::last_path().is_some() {
            ui.label("dumped");
        } else {
            ui.label("(writes next to the DLL)");
        }
    });
    ui.checkbox(
        &mut cfg.stereo.diagnose_rt_hashes,
        "Hash engine RTs per eye into the trace (run with cameras off)",
    );
    ui.separator();

    // The stereo render corrections -- normally on; toggle off to reproduce the artifact each fixes.
    egui::CollapsingHeader::new("Stereo fixes")
        .default_open(true)
        .show(ui, |ui| {
            ui.checkbox(
                &mut cfg.stereo.fix_shadow_cascade_anchor,
                "Sun-shadow cascade anchor (the visible per-eye shadow mismatch; A/B via Present eye 0)",
            );
            ui.checkbox(
                &mut cfg.stereo.dedupe_post_block,
                "Dedupe world post block (eye 1 otherwise runs the post chain + FSR twice)",
            );
            ui.checkbox(
                &mut cfg.stereo.restore_render_camera,
                "Restore pristine render camera after draws (hygiene; no observed effect)",
            );
            ui.checkbox(
                &mut cfg.stereo.unjitter_shadow_fit,
                "Unjitter shadow fit (defensive; the fit reads the unjittered active camera)",
            );
            ui.checkbox(
                &mut cfg.stereo.drain_draw_fragment,
                "Drain draw-dispatch fragment between eyes (open-world crash fix)",
            );
            ui.checkbox(
                &mut cfg.stereo.restore_frame_counters,
                "Restore frame counters between eyes (fixes jitter/parity flicker)",
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
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut cfg.stereo.patch_shadow_pcf_hash,
                    "Patch sun-shadow PCF screen-hash (kills per-eye shimmer + foliage grain)",
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
                    "Patch jitter-unstable LOD dissolve (only matters with FSR jitter on)",
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
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut cfg.stereo.widen_cull_frustum,
                    "Widen scene cull frustum (covers both eyes; stops outer-edge void/pop-in)",
                );
                ui.add_enabled(
                    cfg.stereo.widen_cull_frustum,
                    egui::Slider::new(&mut cfg.stereo.cull_fov_padding, 0.0..=0.5)
                        .text("pad")
                        .fixed_decimals(2),
                )
                .on_hover_text(
                    "Extra fraction to widen the cull frustum on every side (incl. vertical); \
                     raise if geometry still pops in at the edges when flying",
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
                "Disable software occlusion (drops centre-viewpoint occluder culling; fixes \
                 peripheral culling an offset eye can see past)",
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
