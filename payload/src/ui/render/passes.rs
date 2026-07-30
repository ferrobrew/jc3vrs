//! Render-pass id presentation: the engine's debug name for a pass, and the pass picker the
//! range-valued options use.

/// The engine's debug name for a pass id, via `GetRenderPassName`.
pub(super) fn render_pass_name(id: i16) -> &'static str {
    if !(0..0x9D).contains(&i32::from(id)) {
        return "(out of range)";
    }
    // SAFETY: the id is in the enum's verified range, and the engine returns static strings.
    unsafe {
        let pass = std::mem::transmute::<i32, jc3gi::graphics_engine::render_engine::RenderPassId>(
            i32::from(id),
        );
        let ptr = jc3gi::graphics_engine::render_engine::GetRenderPassName(pass);
        if ptr.is_null() {
            return "(null)";
        }
        std::ffi::CStr::from_ptr(ptr.cast())
            .to_str()
            .unwrap_or("(non-utf8)")
    }
}

/// A dropdown for selecting a [`RenderPassId`] from the scene-pass range, showing each variant's
/// hex value and the engine's debug name.
pub(super) fn render_pass_combo_box(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut jc3gi::graphics_engine::render_engine::RenderPassId,
) {
    use jc3gi::graphics_engine::render_engine::RenderPassId;
    // List the scene passes: the G-buffer and main-scene range that foveation operates on.
    const SCENE_PASSES: [RenderPassId; 99] = [
        RenderPassId::RP_ROAD_STENCIL,
        RenderPassId::RP_TERRAINPATCH_DETAIL_MID,
        RenderPassId::RP_TERRAINPATCH_DETAIL_LOW,
        RenderPassId::RP_TERRAINPATCH_BASEMESH_TESSELLATE_NEAR,
        RenderPassId::RP_TERRAINPATCH_BASEMESH_NEAR,
        RenderPassId::RP_TERRAINPATCH_BASEMESH_TESSELLATE_FAR,
        RenderPassId::RP_TERRAINPATCH_BASEMESH_FAR,
        RenderPassId::RP_TERRAINPATCH_BASEMESH_TESSELLATE_COLOR,
        RenderPassId::RP_TERRAINPATCH_BASEMESH_COLOR,
        RenderPassId::RP_TERRAIN_APPLY_NEAR_DETAILED,
        RenderPassId::RP_TERRAIN_APPLY_NEAR,
        RenderPassId::RP_TERRAIN_APPLY_FAR,
        RenderPassId::RP_MODELS_DYNAMIC,
        RenderPassId::RP_MODELS_DYNAMIC_MASK_DAMAGE_POST_EFFECT,
        RenderPassId::RP_MODELS_STATIC,
        RenderPassId::RP_MODELS_REFLECTION,
        RenderPassId::RP_UNDERWATER_VEGETATION,
        RenderPassId::RP_VEGETATION_OPAQUE,
        RenderPassId::RP_VEGETATIONFINS,
        RenderPassId::RP_VEGETATIONGROUP,
        RenderPassId::RP_VEGETATIONGROUP2,
        RenderPassId::RP_TERRAIN_FOREST,
        RenderPassId::RP_CREATURES,
        RenderPassId::RP_UNDERWATER_FOG_GRADIENT,
        RenderPassId::RP_Z_LOCK,
        RenderPassId::RP_ROAD_JUNCTION,
        RenderPassId::RP_ROAD_LAYERS,
        RenderPassId::RP_ROAD_JUNCTION_OPAQUE,
        RenderPassId::RP_DOWNSAMPLE_DEPTH,
        RenderPassId::RP_DECALS,
        RenderPassId::RP_SCREEN_SPACE_DECALS,
        RenderPassId::RP_SCREEN_SPACE_ROAD_DECALS,
        RenderPassId::RP_LAST_GBUFFER,
        RenderPassId::RP_REFLECTIVE_WATER_PLANES,
        RenderPassId::RP_AO_VOLUMES,
        RenderPassId::RP_SSAO,
        RenderPassId::RP_SCREEN_SPACE_REFLECTIONS,
        RenderPassId::RP_GLOBAL_ILLUMINATION,
        RenderPassId::RP_SCREEN_SPACE_SUBSURFACE_SKIN,
        RenderPassId::RP_DEFERRED_LIGHTS,
        RenderPassId::RP_DEBUG_GI,
        RenderPassId::RP_LINES,
        RenderPassId::RP_OCCLUDERS_DEBUG,
        RenderPassId::RP_BILLBOARD,
        RenderPassId::RP_OCCLUSION_QUERY,
        RenderPassId::RP_LAST_OPAQUE,
        RenderPassId::RP_STARS,
        RenderPassId::RP_SUN,
        RenderPassId::RP_MOON,
        RenderPassId::RP_SKYBOX,
        RenderPassId::RP_SKY_GRADIENT,
        RenderPassId::RP_FOG_GRADIENT,
        RenderPassId::RP_DEBUG_TRANSPARENCY,
        RenderPassId::RP_UNDERWATER_CLOUDS,
        RenderPassId::RP_UNDERWATER_VEGETATION_TRANSPARENT,
        RenderPassId::RP_COPY_FRAMEBUFFER,
        RenderPassId::RP_WATER,
        RenderPassId::RP_POST_WATER,
        RenderPassId::RP_SKIDMARKS,
        RenderPassId::RP_PRE_CLOUDS,
        RenderPassId::RP_LENSFLARE,
        RenderPassId::RP_POST_CLOUDS,
        RenderPassId::RP_APPLY_CLOUDS,
        RenderPassId::RP_VEGETATION_TRANSPARENT_AOIT,
        RenderPassId::RP_FOG_VOLUME_GENERATE,
        RenderPassId::RP_FOG_VOLUME_UPSAMPLE,
        RenderPassId::RP_FOG_VOLUME_APPLY,
        RenderPassId::RP_MASK_WATER,
        RenderPassId::RP_MODELS_TRANSPARENT,
        RenderPassId::RP_VEGETATION_TRANSPARENT,
        RenderPassId::RP_VEGETATION_POST_DRAW,
        RenderPassId::RP_BB_RAIN,
        RenderPassId::RP_MODELS_GLINT,
        RenderPassId::RP_WATER_GODRAYS,
        RenderPassId::RP_BULLETS,
        RenderPassId::RP_CONTRAILS,
        RenderPassId::RP_GROUNDHAZE,
        RenderPassId::RP_PARTICLE_RIBBON,
        RenderPassId::RP_MODEL_HALO_POST,
        RenderPassId::RP_PARTICLE_LOWRES,
        RenderPassId::RP_SPOTLIGHT_VOLUMETRICS,
        RenderPassId::RP_WINDOW_DECALS,
        RenderPassId::RP_MODELS_REFRACT,
        RenderPassId::RP_PARTICLE_GENERAL,
        RenderPassId::RP_PARTICLE_DISTORT,
        RenderPassId::RP_PARTICLE_LOWRES_OVERLAY,
        RenderPassId::RP_SCENE_CAPTURE,
        RenderPassId::RP_Z_FINAL_TRANSPARENT,
        RenderPassId::RP_CLEAR_SCREEN_SPACE_SUBSURFACE_SKIN,
        RenderPassId::RP_CLEAR_STENCIL,
        RenderPassId::RP_GHOST_EFFECT,
        RenderPassId::RP_OUTLINE_MASK,
        RenderPassId::RP_OUTLINE_EFFECT,
        RenderPassId::RP_OUTLINE_EFFECT_NO_DEPTH,
        RenderPassId::RP_OUTLINE_EFFECT_BLUR,
        RenderPassId::RP_FINAL_TRANSPARENT,
        RenderPassId::RP_PARTICLE_ONSCREEN,
        RenderPassId::RP_POSTEFFECTS,
        RenderPassId::RP_LAST_MAIN,
    ];
    let pass_label =
        |pass: RenderPassId| format!("{:#04X}: {}", pass as i32, render_pass_name(pass as i16));
    egui::ComboBox::from_label(label)
        .selected_text(pass_label(*value))
        .show_ui(ui, |ui| {
            for pass in SCENE_PASSES {
                ui.selectable_value(value, pass, pass_label(pass));
            }
        });
}
