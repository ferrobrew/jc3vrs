//! The stereo correctness levers, grouped by the engine subsystem each corrects. Normally all on;
//! each one is toggleable so the artifact it fixes can be reproduced.

use crate::{config, hooks::graphics_engine::shader};

pub(super) fn section(ui: &mut egui::Ui, cfg: &mut config::Config) {
    // The stereo render corrections, grouped by subsystem -- normally on; toggle off to reproduce
    // the artifact each fixes. Collapsed by default to keep the tab scannable. (The investigation
    // probes live in the Diagnostics tab.)
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
            &mut cfg.stereo.defer_frame_tail,
            "Defer the frame tail to a worker (overlap next sim with the GPU tail)",
        )
        .on_hover_text(
            "Moves the final drain, VR blit/submit, and mirror onto a tail thread so the next \
             frame's sim starts immediately. A/B with the profiler's 'GPU idle' number.",
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
            &mut cfg.stereo.share_water_simulation,
            "Share the WaveWorks ocean step across eyes (one simulation kick per frame)",
        )
        .on_hover_text(
            "Without this the second eye advances the ocean simulation again and renders a later \
             sea state, so the sun-glint sparkle decorrelates between the eyes (issue #47).",
        );
        ui.checkbox(
            &mut cfg.stereo.per_eye_water_reflection,
            "Per-eye water reflection (re-mirror + re-render the planar reflection per eye)",
        )
        .on_hover_text(
            "The water samples its reflection map at each pixel's own screen position, so a map \
             rendered from one mirrored camera only matches one eye (issue #47). Costs one extra \
             reflection render per frame.",
        );
        ui.checkbox(
            &mut cfg.stereo.disable_screen_space_water_reflection,
            "Disable screen-space water reflection (A/B for the per-eye water mismatch)",
        )
        .on_hover_text(
            "Forces the water onto the full-reflection binding. If the eyes then agree, the \
             screen-space reflection sampling is the seam (issue #47). Restores the engine value \
             when unchecked.",
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
            &mut cfg.stereo.relax_terrain_patch_hull_culls,
            "Relax terrain hull culls (drops the per-patch back-facing + frustum discards baked for \
             one camera; fixes black terrain patch gaps)",
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

    ui.collapsing("Shader patches", |ui| {
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut cfg.stereo.patch_shadow_pcf_hash,
                "Sun-shadow PCF screen-hash (kills per-eye shimmer + foliage grain)",
            );
            let patched = shader::patched_count();
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
            let patched = shader::dissolve_patched_count();
            ui.label(if patched == 0 {
                "(0 patched -- click Reload shaders)".to_string()
            } else {
                format!("({patched} sites patched)")
            });
        });
        ui.horizontal(|ui| {
            if ui.button("Reload shaders").clicked() {
                shader::request_reload();
            }
            ui.label(
                "re-creates all shaders so the shader patches take effect (F11 toggles + reloads)",
            );
        });
    });
}
