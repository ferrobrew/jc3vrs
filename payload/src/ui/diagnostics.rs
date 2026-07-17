//! The Diagnostics tab: observation and bisect tooling — the render-trace dump, the per-eye A/B
//! isolation levers, live engine cull state, the render-block-type registry, the per-eye camera
//! snapshot diff, and the log filter. Feature and quality toggles live in the Render tab.

use std::{
    collections::BTreeSet,
    sync::{
        Mutex,
        atomic::{AtomicI32, Ordering},
    },
};

use jc3gi::{
    camera::camera::CameraState,
    graphics_engine::{
        render_block::{
            RenderBlockTypeTerrain as BaseTerrain, RenderBlockTypeTerrainPatch as TerrainPatch,
        },
        render_engine::RenderBlockTypeRegistry,
    },
};

use super::camera::matrix_grid;
use crate::{
    config,
    debug::{
        camera::{CAMERA_SNAPSHOTS, CameraSnapshot},
        trace,
    },
};

/// The frame count for the editable "Dump N frames" trace button, persisted across UI frames.
static TRACE_FRAME_COUNT: AtomicI32 = AtomicI32::new(60);

pub fn egui_debug_diagnostics(ui: &mut egui::Ui) {
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
    ui.collapsing("Flicker isolation (#31)", |ui| {
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
            ui.checkbox(
                &mut cfg.stereo.shadow_update_every_frame,
                "Update all shadow cascades every frame (defeats 2^L amortization; parity-flip probe)",
            )
            .on_hover_text(
                "Zeroes m_CascadeUpdateLevels so cascades 1-5 refresh every frame like cascade 0, instead \
                 of on the amortized schedule. Diagnostic for the #31 parity ping-pong: capture at native \
                 rate and check whether the ShadowState scale_blend.y 6<->1 flip flattens.",
            );
        });
    ui.separator();

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

    ui.collapsing("Per-eye render camera (differences in yellow)", |ui| {
        let (s0, s1) = {
            let g = CAMERA_SNAPSHOTS.lock();
            (g[0], g[1])
        };
        ui.columns(2, |cols| {
            show_camera_snapshot(&mut cols[0], "Eye 0", &s0, &s1);
            show_camera_snapshot(&mut cols[1], "Eye 1", &s1, &s0);
        });
    });

    ui.collapsing("Logging", |ui| {
        ui.label(match crate::logging::active_spec() {
            Some(spec) => format!("Active filter: {spec}"),
            None => "Active filter: (launch RUST_LOG, INFO floor)".to_string(),
        });
        let mut edit = FILTER_EDIT.lock();
        let mut error = FILTER_ERROR.lock();
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut *edit)
                .on_hover_text("RUST_LOG directive syntax, e.g. warn,vr=debug,coord_frame=debug.");
            if ui.button("Apply").clicked() {
                *error = crate::logging::set_filter(&edit).err();
            }
        });
        if let Some(e) = error.as_ref() {
            ui.colored_label(egui::Color32::LIGHT_RED, e);
        }
    });

    // Release CONFIG before start(), whose manifest snapshot re-locks it.
    drop(cfg);
    if let Some(frames) = start_trace {
        trace::TraceState::start(frames);
    }
}

/// The in-progress log-filter text, kept across frames until applied.
static FILTER_EDIT: parking_lot::Mutex<String> = parking_lot::Mutex::new(String::new());
/// The last log-filter apply error, shown until the next successful apply.
static FILTER_ERROR: parking_lot::Mutex<Option<String>> = parking_lot::Mutex::new(None);

fn show_camera_snapshot(
    ui: &mut egui::Ui,
    label: &str,
    snap: &CameraSnapshot,
    other: &CameraSnapshot,
) {
    ui.label(egui::RichText::new(label).strong());
    if !snap.valid {
        ui.label("(no capture)");
        return;
    }
    ui.label(format!("cam ptr: {:#x}", snap.camera_ptr));

    // The flag values come from the generated bitflags, so the table's bits can never drift from
    // the pyxis definition; only the display labels are local.
    const FLAG_NAMES: [(CameraState, &str); 6] = [
        (CameraState::m_UseOffCenter, "OffCenter"),
        (CameraState::m_ScreenshotSeriesRunning, "ScreenshotSeries"),
        (CameraState::m_Ortho, "Ortho"),
        (CameraState::m_ComputeView, "ComputeView"),
        (CameraState::m_DirtyProjection, "DirtyProj"),
        (CameraState::m_IsRenderCamera, "IsRenderCam"),
    ];
    let state = CameraState::from_bits_truncate(snap.state_bits);
    let active: Vec<&str> = FLAG_NAMES
        .iter()
        .filter(|(flag, _)| state.contains(*flag))
        .map(|(_, name)| *name)
        .collect();
    let flag_text = format!("flags {:#04x}: {}", snap.state_bits, active.join(" | "));
    if other.valid && snap.state_bits != other.state_bits {
        ui.colored_label(egui::Color32::YELLOW, flag_text);
    } else {
        ui.label(flag_text);
    }

    ui.label(format!(
        "offcenter: tiles={} x={} y={}",
        snap.offcenter_tiles, snap.offcenter_tile_x, snap.offcenter_tile_y
    ));
    ui.label(format!("fov={:.4}  aspect={:.4}", snap.fov, snap.aspect));
    ui.label(format!("near={:.3}  far={:.1}", snap.near, snap.far));
    ui.label(format!("size={}x{}", snap.width, snap.height));

    let other_proj = other.valid.then_some(&other.projection);
    let other_view = other.valid.then_some(&other.view);
    let other_vpf = other.valid.then_some(&other.view_proj_f);
    matrix_grid(
        ui,
        &format!("proj_{label}"),
        "m_Projection:",
        &snap.projection,
        other_proj,
    );
    matrix_grid(
        ui,
        &format!("view_{label}"),
        "m_View:",
        &snap.view,
        other_view,
    );
    matrix_grid(
        ui,
        &format!("vpf_{label}"),
        "m_ViewProjectionF:",
        &snap.view_proj_f,
        other_vpf,
    );
    let other_tf = other.valid.then_some(&other.transform);
    matrix_grid(
        ui,
        &format!("tf_{label}"),
        "m_TransformF:",
        &snap.transform,
        other_tf,
    );
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
