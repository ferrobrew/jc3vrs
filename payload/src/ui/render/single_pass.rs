//! Single-pass stereo (experimental): the bring-up levers, the shader-rewrite census, and the
//! instanced eye-parity exposure readout.

use crate::{
    config,
    hooks::graphics_engine::shader,
    stereo::single_pass::{self, Capability},
};

pub(super) fn section(ui: &mut egui::Ui, cfg: &mut config::Config) {
    // Single-pass stereo (experimental, off by default): render the G-buffer geometry once, emitting
    // both eyes via instancing + SV_ViewportArrayIndex routing, instead of the double-draw. The
    // pipeline is under construction; the census dry-run below is safe (no rendering change) and
    // reports how the vertex-shader rewriter fares against the game's real shader set.
    ui.collapsing("Single-pass stereo (experimental)", |ui| {
        // All-in/all-out toggle: flip the whole work-in-progress configuration together, so the one
        // button is the normal way to turn the feature on or off. The individual levers below stay for
        // bring-up. Reloads either way -- on to apply the patches, off to restore the pristine shaders.
        let enabled = cfg.stereo.single_pass.enabled;
        let toggle_label = if enabled {
            "⚡ Disable single-pass stereo"
        } else {
            "⚡ Enable single-pass stereo"
        };
        if ui
            .button(toggle_label)
            .on_hover_text(
                "Flips the whole feature -- single-pass, dual-eye, collapse, double-wide, the \
                 reprojected scene families, terrain, tree impostors, and the bark/foliage/occluder \
                 re-issue -- on or off together, along with native resolution. Clears the census \
                 dry-run and reloads the shaders.",
            )
            .clicked()
        {
            let on = !enabled;
            cfg.stereo.single_pass.enabled = on;
            cfg.stereo.single_pass.dual_eye = on;
            cfg.stereo.single_pass.collapse = on;
            cfg.stereo.single_pass.double_wide = on;
            cfg.stereo.single_pass.reproject = on;
            cfg.stereo.single_pass.reproject_camera_only = on;
            cfg.stereo.single_pass.terrain = on;
            cfg.stereo.single_pass.tree_impostors = on;
            cfg.stereo.single_pass.bark = on;
            cfg.stereo.single_pass.foliage = on;
            cfg.stereo.single_pass.occluder = on;
            cfg.stereo.single_pass.patch_dryrun = false;
            cfg.vr.native_resolution = on;
            if on && cfg.far_field.mode == crate::far_field::FarFieldMode::Share {
                cfg.far_field.mode = crate::far_field::FarFieldMode::Collect;
            }
            shader::request_reload();
        }
        ui.separator();
        ui.checkbox(
            &mut cfg.stereo.single_pass.enabled,
            "Enable single-pass stereo (experimental)",
        )
        .on_hover_text(
            "Master switch. Renders the G-buffer once with stereo-rewritten vertex shaders. \
             Forced inert without the DXVK viewport-routing capability (below).",
        );
        ui.checkbox(
            &mut cfg.stereo.single_pass.patch_dryrun,
            "Census only (dry-run: patch + tally, do not substitute -- safe)",
        )
        .on_hover_text(
            "Runs the vertex-shader stereo rewrite on every shader at creation and counts the \
             outcomes, without changing rendering. Validates the rewriter against real shaders.",
        );
        ui.checkbox(
            &mut cfg.stereo.single_pass.dump_vs_name_census,
            "Dump the vertex-shader name census on the next shader reload",
        )
        .on_hover_text(
            "Records each vertex shader's name against its rewrite class and writes them to the \
             session directory when the shaders next reload. The census only sees shaders created \
             while it is on, so turn it on, visit the area whose families you want, and reload.",
        );
        ui.separator();
        ui.label("Enable top-to-bottom; all are needed together for a clean image:");
        ui.add_enabled_ui(cfg.stereo.single_pass.enabled, |ui| {
            ui.checkbox(
                &mut cfg.stereo.single_pass.dual_eye,
                "Dual-eye: distinct per-eye cb13 + eye-half viewports + instance doubling",
            )
            .on_hover_text(
                "Makes the eyes diverge. On its own (no double-wide, no collapse) each eye \
                 renders into half of a per-eye target -- squished; a bisection step.",
            );
            ui.add_enabled_ui(cfg.stereo.single_pass.dual_eye, |ui| {
                ui.checkbox(
                    &mut cfg.stereo.single_pass.double_wide,
                    "Double-wide render target (full per-eye resolution)",
                )
                .on_hover_text(
                    "Renders the scene targets at 2x per-eye width so each eye-half is full \
                     resolution instead of squished. Needs Collapse and native resolution on.",
                );
                let collapse_changed = ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.collapse,
                        "Collapse to a single game.Draw walk (the actual perf win; riskiest)",
                    )
                    .on_hover_text(
                        "One walk renders both eyes: centered camera, no between-eye restore, the \
                         back buffer split into the two eye textures. Without double-wide each eye \
                         is squished/half-filled; unpatched geometry and the HUD reach the left eye \
                         only. Turns the far-field Share mode off -- the two are exclusive. \
                         Reloads shaders, since some rewrites only apply while the collapse is on.",
                    )
                    .changed();
                if collapse_changed {
                    // Some shader rewrites are gated on the collapse being active -- the screen-space
                    // decal depth-UV bias is only correct against a double-wide target -- so a shader
                    // created under one setting is wrong under the other. Only the reload retracts or
                    // applies them.
                    shader::request_reload();
                }
                if collapse_changed
                    && cfg.stereo.single_pass.collapse
                    && cfg.far_field.mode == crate::far_field::FarFieldMode::Share
                {
                    cfg.far_field.mode = crate::far_field::FarFieldMode::Collect;
                }
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.reproject,
                        "Reproject the no-cb0 scene families (NPCs, props, buildings, roads)",
                    )
                    .on_hover_text(
                        "Rewrites the baked-WVP scene shaders (characters, props, buildings, roads) \
                         to post-multiply their clip by the per-eye M_eye, instead of leaving them \
                         double-drawn. Reloads shaders to apply. Sky/UI/post are excluded.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.reproject_camera_only,
                        "...including the ones claimed on a cb0[4] reference alone",
                    )
                    .on_hover_text(
                        "Extends the reprojection to allowlisted families the cb0 remap claims on a \
                         camera-position reference that is not their position path: generaljc3, \
                         landmark, layered and layeredblend all read cb0[4] for a LOD fade and build \
                         clip from a baked cb1. Without it they get viewport routing but no per-eye \
                         clip, so both eye halves are drawn from the collapsed centre viewpoint. \
                         Requires the reprojection above. Reloads shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.terrain,
                        "Single-pass the tessellated terrain (VS -> HS -> DS)",
                    )
                    .on_hover_text(
                        "Rides the eye index through the terrain tessellation pipeline: the vertex \
                         shader writes it on the free TEXCOORD3.z lane, the hull forwards it, and the \
                         domain reprojects its clip by the per-eye M_eye. Covers the DrawIndexed \
                         terrain passes; the GPU-indirect near passes stay double-drawn. Reloads \
                         shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.tree_impostors,
                        "Single-pass the tree impostors (far-distance billboards)",
                    )
                    .on_hover_text(
                        "Reprojects the treeimpostor* vertex shaders by the per-eye M_eye, like the \
                         scene families. They draw non-instanced with no GPU-indirect path, so this \
                         covers them completely. Reloads shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                ui.label("Render-block re-issue:");
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.bark,
                        "Bark (tree trunks/branches)",
                    )
                    .on_hover_text(
                        "Re-issues CRenderBlockBark's Draw/DrawZ once per eye with its baked cb1 \
                         view-projection reprojected by M_eye. Covers its plain/instanced/\
                         GPU-indirect draw kinds, and declines the cb0 remap on the vegetationbark* \
                         shaders (which read cb0[4] for shading, not position). Reloads shaders to \
                         apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(&mut cfg.stereo.single_pass.foliage, "Foliage (grass)")
                    .on_hover_text(
                        "Re-issues CRenderBlockFoliage's Draw/DrawZ once per eye with its baked cb2 \
                         view-projection reprojected by M_eye, and declines the cb0 remap on the \
                         vegetationfoliage* shaders (which read cb0[4] only as a wind-noise origin). \
                         Does not fix the separate forward-lighting black-grass issue. Reloads \
                         shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                ui.checkbox(
                    &mut cfg.stereo.single_pass.occluder,
                    "Occluder (depth-prime boxes)",
                )
                .on_hover_text(
                    "Re-issues CRenderBlockOccluder's DrawZ once per eye with its baked cb1 \
                     view-projection reprojected by M_eye, priming each eye's depth with its own \
                     projection.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.instanced_per_eye,
                    "Already-instanced draws per eye (buildings, vegetation)",
                )
                .on_hover_text(
                    "Re-issues a DrawIndexedInstanced whose patched shader is bound once per eye, \
                     with both cb13 eye slots and both viewport slots pinned to that eye, so the \
                     game's own instance ids stop being read as an eye parity. Off, the batch is \
                     split alternately between the eyes -- the building flicker.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.indirect_per_eye,
                    "GPU-indirect draws per eye (near terrain patches, foliage)",
                )
                .on_hover_text(
                    "Re-issues a DrawIndexedInstancedIndirect / DrawInstancedIndirect once per eye \
                     with both viewport slots pinned to that eye's half. Nothing detoured these entry \
                     points, so the near tessellating terrain patches and the foliage inherited the \
                     full double-wide viewport and were stretched 2x horizontally -- which reads as \
                     them sliding across the screen at twice the camera's rate.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.reconstruct_per_eye,
                    "Deferred lighting resolve per eye (sun shadows)",
                )
                .on_hover_text(
                    "Runs the deferred clustered-lighting resolve twice, each run scissor-masked to \
                     one eye's half of the double-wide target and reconstructing depth with that \
                     eye's own basis. Off, the one fullscreen quad covers both halves with one eye's \
                     basis, and the error turns with the camera -- the sun shadows slide across the \
                     screen. Needs the off-axis reconstruction override on.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.nvwater_per_eye,
                    "WaveWorks water per eye (parallax)",
                )
                .on_hover_text(
                    "Gives the NvWater* blocks a per-eye view. Their screen-space reads were always \
                     fine; what is missing is parallax -- the vertex shader transforms from a baked \
                     matrix in its own constant buffer, so both eyes get the collapsed centre view. \
                     Flat water at the wrong depth looks right on the mirror and wrong in the headset.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.ssdecal_geometry_per_eye,
                    "...and reproject the decal box geometry",
                )
                .on_hover_text(
                    "Sub-option of the decal fix: reprojects the decal box's baked transform per eye \
                     so its screen coverage has parallax too. The reconstruction fix alone stops the \
                     sliding; this adds depth to it.",
                );
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.ssdecal_per_eye,
                        "Screen-space decals per eye",
                    )
                    .on_hover_text(
                        "Re-uploads each eye's reconstruction basis for the SSDecal block and biases \
                         its projective depth-fetch UV into that eye's half of the double-wide \
                         buffer. Off, the decal reconstructs its surface from the wrong part of the \
                         depth buffer and the error moves with the camera. Reloads shaders to apply: \
                         the depth-fetch bias is a shader rewrite, and until the reload the already \
                         rewritten permutations keep reading it.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                ui.checkbox(&mut cfg.stereo.single_pass.ssao_per_eye, "SSAO per eye")
                    .on_hover_text(
                        "Another of the seven depth-reconstruction passes. Hazardous: SSAO advances a \
                         temporal history per invocation, so a per-eye re-issue double-advances it.",
                    );
                ui.checkbox(&mut cfg.stereo.single_pass.ssr_per_eye, "SSR per eye")
                    .on_hover_text(
                        "Same reconstruction defect. Hazardous: SSR ray-marches a scene capture taken \
                         earlier in the frame, so a second run consumes what the first did.",
                    );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.subsurface_per_eye,
                    "Subsurface skin per eye",
                )
                .on_hover_text(
                    "Same reconstruction defect. This block rebuilds the inverse twice, once per blur \
                     axis, so both runs have to be masked.",
                );
                ui.checkbox(&mut cfg.stereo.single_pass.dof_per_eye, "Depth of field per eye")
                    .on_hover_text(
                        "The last consumer of the reconstruction basis. A post pass rather than a \
                         render block, so it may want the basis substituted rather than the pass \
                         re-issued.",
                    );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.collapse_viewport_follows_target,
                    "Eye viewports follow the bound target (clouds, smoke, spotlight volumetrics)",
                )
                .on_hover_text(
                    "Splits the viewport of whatever render target the engine currently has bound, instead of always splitting the scene's double-wide one. The low-resolution clouds, particles, and spot-light cones render into a shared quarter-resolution buffer (half per axis); handing those draws a full-scene viewport magnifies them 2x about the target's origin and crops them, which is also a 2x motion gain -- clouds and smoke sliding at twice the camera's rate. A no-op everywhere else, since the two viewports agree outside those passes.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.slot13_per_eye,
                    "Non-indexed geometry per eye (decals, roads, skidmarks)",
                )
                .on_hover_text(
                    "Re-issues a non-indexed Draw once per eye, with both viewport and cb13 eye slots pinned to that eye, for the passes known to submit geometry this way. Off, every slot-13 draw gets the whole double-wide viewport -- right for a fullscreen triangle, but it stretches decals and road layers 2x horizontally, which is a 2x motion gain and reads as them sliding over the world. Default off until the pass allowlist is confirmed: read the 'slot-13 by pass' census in the log to see which passes actually arrive here.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.atmospheric_per_eye,
                    "Atmospheric scattering per eye (sun shadows, aerial perspective)",
                )
                .on_hover_text(
                    "The same split for the atmospheric-scattering pass, which reconstructs the \
                     whole screen -- sky included -- and ray-marches the sun cascade and aerial \
                     perspective over it. It is the second consumer of the reconstruction basis, so \
                     with only the deferred resolve split this pass paints the sliding error back \
                     over the fixed one. Needs the off-axis reconstruction override on.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.water_uv_per_eye,
                    "Water reflection UVs per eye (legacy non-WaveWorks water)",
                )
                .on_hover_text(
                    "Re-issues the Water*/WaterBox* blocks' Draw once per eye with their screen-UV \
                     matrix biased into that eye's half of the double-wide target. Those shaders \
                     sample reflection/refraction/depth through a projective TEXCOORD1 normalized \
                     over one eye's viewport while the buffers are double-wide, so each eye reads \
                     the whole two-eye image across its water -- a 2x stretch, and a 2x motion gain, \
                     which reads as the reflections sliding. Only affects the lower water-quality \
                     settings; the NvWater WaveWorks path samples by pixel coordinate and is fine.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.clustered_per_eye,
                    "Clustered light grid per eye (local lights on forward materials)",
                )
                .on_hover_text(
                    "Builds the 64-pixel froxel light grid once per eye, each run masked to that \
                     eye's half of the tile grid with that eye's projection and tile bounds, and the \
                     second run's clear suppressed so the halves compose. Off, the grid is built with \
                     eye 0's projection against the double-wide tile count, so local lights land in \
                     the wrong tiles for both eyes -- on the ~20 forward-lit families that sample it \
                     (foliage, glass, particles, blended materials) as well as the deferred resolve. \
                     Needs the per-eye resolve and the off-axis tile-bounds fix on. Declines itself \
                     if the eye seam does not fall on a tile boundary.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.clustered_per_eye_light_view,
                    "...and assign its lights from each eye's position",
                )
                .on_hover_text(
                    "Folds the eye's world offset into the translation row of the light-assignment \
                     view matrix, so each eye's half is assigned from that eye rather than from the \
                     collapsed centre camera. Sub-option of the above, on its own flag because the \
                     difference may be below the 64-pixel tile granularity. Positional offset only; \
                     per-eye display canting is not applied.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.uniform_viewport_slots,
                    "Uniform viewport slots outside the G-buffer",
                )
                .on_hover_text(
                    "Puts both viewport slots back to one region once the G-buffer range ends, so an \
                     instanced draw with a patched shader in a pass that is not eye-split (shadows, \
                     reflections, post) keeps its odd-numbered instances instead of routing them into \
                     the other eye's half. Slot 0 is never changed.",
                );
            });
        });

        let capability = single_pass::probe_if_needed();
        ui.label(match capability {
            Capability::Supported => "Viewport routing: supported ✓",
            Capability::Unsupported => "Viewport routing: UNSUPPORTED (single-pass will stay inert)",
            Capability::Unprobed => "Viewport routing: not yet probed (no device seen)",
        });

        let (patched, no_refs, deferred, errored) = (
            single_pass::patched_count(),
            single_pass::no_refs_count(),
            single_pass::deferred_count(),
            single_pass::errored_count(),
        );
        if patched + no_refs + deferred + errored == 0 {
            ui.label("Census: 0 shaders seen -- enable a mode above, then click Reload shaders.");
        } else {
            ui.label(format!(
                "Census: {patched} patched, {no_refs} no per-eye refs, {deferred} instance-id deferred, {errored} errored"
            ));
            if errored > 0 {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "errored > 0: a shader the offline corpus did not cover -- check the log",
                );
            }
        }
        let (hs_forwarded, ds_reprojected) = single_pass::terrain_counts();
        ui.label(format!(
            "Terrain: {hs_forwarded} hull forwarded, {ds_reprojected} domain reprojected"
        ));
        let s = single_pass::substitution_stats();
        ui.label(format!(
            "Recorded VS (doubled): {} | CreateVertexShader: pending {}, re-acquired [patched {}, already-cb13 {}, no-refs {}, err {}]",
            s.recorded_vs,
            s.cvs_pending,
            s.cvs_reacq_patched,
            s.cvs_reacq_cb13,
            s.cvs_reacq_no_refs,
            s.cvs_reacq_err,
        ));
        instanced_exposure_readout(ui);
        ui.horizontal(|ui| {
            if ui.button("Reload shaders").clicked() {
                shader::request_reload();
            }
            ui.label("re-runs the census over a fresh shader-creation pass");
        });
    });
}

/// Whether the config asks for the single-pass collapse, ignoring the runtime gates
/// ([`crate::stereo::single_pass::collapse_active`] also requires the device capability and a live
/// session). The UI needs the *intent*, so a mutually-exclusive option greys out as soon as the boxes
/// are ticked rather than only once the collapse is running.
pub(super) fn collapse_configured(cfg: &config::Config) -> bool {
    cfg.stereo.single_pass.enabled
        && cfg.stereo.single_pass.dual_eye
        && cfg.stereo.single_pass.collapse
}

/// How much of the collapse's frame the already-instanced eye-parity case covers, and how much of that
/// the per-eye re-issue is handling. The case: an instanced draw's instance ids are the game's own, so
/// the patched shader's `SV_InstanceID & 1` reads them as an eye parity and the batch would be split
/// between the eyes (or, at one instance, land in the left eye alone). *Handled* draws were re-issued
/// once per eye; *exposed* ones still carry the artifact. The per-shader list says which families are
/// paying the extra submissions.
fn instanced_exposure_readout(ui: &mut egui::Ui) {
    let report = single_pass::instanced_exposure();
    if report.frames == 0 {
        ui.label("Instanced eye-parity: no single-pass geometry frames measured yet.");
        return;
    }
    let last = report.last_frame;
    ui.label(format!(
        "Instanced eye-parity: {} handled, {} exposed of {} DrawIndexedInstanced last frame | mean \
         over {} frames: {:.1} handled ({:.1} instances) + {:.1} exposed ({:.1} single-instance, \
         {:.1} split batches) of {:.1}, peak {} instances",
        last.handled,
        last.affected,
        last.total,
        report.frames,
        report.mean_handled,
        report.mean_handled_instances,
        report.mean_affected,
        report.mean_affected_single_instance,
        report.mean_affected_multi_instance,
        report.mean_total,
        report.peak_instances,
    ))
    .on_hover_text(
        "Draws where a patched vertex shader was bound inside the G-buffer range under the collapse, \
         excluding the mod's own promoted and per-eye re-issued draws. Handled = re-issued once per \
         eye with cb13 and the viewport pinned to that eye; exposed = still split by instance parity, \
         which is what the toggle above turns into handled.",
    );
    ui.label(format!(
        "By range: in-range {} patched + {} unpatched | out-of-range {} patched ({} instances) + {} \
         unpatched | mean out-of-range patched over {} frames: {:.1} draws, {:.1} instances",
        last.handled + last.affected,
        last.in_range_unpatched,
        last.out_of_range_patched,
        last.out_of_range_patched_instances,
        last.out_of_range_unpatched,
        report.frames,
        report.mean_out_of_range_patched,
        report.mean_out_of_range_patched_instances,
    ))
    .on_hover_text(
        "Every DrawIndexedInstanced of the frame, split by whether a patched vertex shader was bound \
         and whether the draw was inside the G-buffer range. Only the in-range patched draws are \
         eye-split; the out-of-range patched ones still write SV_ViewportArrayIndex from instance \
         parity, so they need both viewport slots to hold the same region.",
    );
    let offenders = single_pass::instanced_offenders(8);
    if offenders.is_empty() {
        return;
    }
    ui.collapsing("Instanced patched-shader draws by shader (sampled)", |ui| {
        ui.label(
            "Attributing a draw to a shader takes a lock, so it runs only on diagnostic frames. \
             These counts are a sample and are not comparable in absolute terms with the exhaustive \
             totals above -- read them for the ranking between shaders, not the magnitudes.",
        );
        for offender in offenders {
            let name = offender
                .name
                .unwrap_or_else(|| format!("<unnamed {:#x}>", offender.shader));
            ui.label(format!(
                "{name}: in-range {} draws / {} instances, out-of-range {} draws / {} instances",
                offender.draws,
                offender.instances,
                offender.out_of_range_draws,
                offender.out_of_range_instances
            ));
        }
    });
}
