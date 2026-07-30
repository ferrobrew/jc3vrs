//! The resolution and shading-density levers: the engine's reduced-resolution fog/particle/
//! spotlight passes (issue #8's pixelation), and static foveated rendering (#29).

use crate::{config, ui::render::passes::render_pass_combo_box};

pub(super) fn section(ui: &mut egui::Ui, cfg: &mut config::Config) {
    // Resolution levers for issue #8's pixelation/large-tile artifact around lights and explosions:
    // the engine's reduced-resolution fog/particle/spotlight passes, whose coarse grids VR's wide
    // FOV magnifies. All default off (not headset-verifiable; particles can hide content).
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
            ui.label("Foveated pass range:");
            render_pass_combo_box(ui, "First", &mut cfg.foveation.foveal_first_pass);
            ui.label("..=");
            render_pass_combo_box(ui, "Last", &mut cfg.foveation.foveal_last_pass);
        });
        ui.checkbox(
            &mut cfg.foveation.debug_show_mask,
            "Debug: paint dropped pixels magenta (visualize the mask)",
        );
        ui.label(
            "Drops a dithered radial fraction of peripheral pixels before shading, then \
             reconstructs them. Off by default; needs in-headset tuning.",
        );
    });
}
