//! egui debug UI: the dockable tab tree and per-tab bodies, plus the startup build banner.
//!
//! The tabs live in an `egui_dock` tree so any tab can be dragged out into its own floating window
//! (e.g. Performance parked beside the game view while tuning Render features). The layout is
//! persisted next to the DLL so a customized arrangement survives relaunches.

use std::{sync::OnceLock, time::Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

pub mod camera;
pub mod diagnostics;
pub mod environment;
pub mod game;
pub mod hud;
pub mod performance;
pub mod previews;
pub mod render;
mod util;
pub mod vr;

/// The dockable tabs, in default display order. The serialized names are part of the persisted
/// layout file, so renaming a variant orphans its saved position (it falls back to being appended).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tab {
    Performance,
    Render,
    Previews,
    Diagnostics,
    Camera,
    Game,
    Hud,
    Environment,
    Vr,
}

impl Tab {
    /// Every tab, in the order the default layout lists them.
    const ALL: [Tab; 9] = [
        Tab::Performance,
        Tab::Render,
        Tab::Previews,
        Tab::Diagnostics,
        Tab::Camera,
        Tab::Game,
        Tab::Hud,
        Tab::Environment,
        Tab::Vr,
    ];
}

impl std::fmt::Display for Tab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

struct TabBodies<'a> {
    renderer: &'a mut egui_directx11::Renderer,
}

impl egui_dock::TabViewer for TabBodies<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        tab.to_string().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        match tab {
            Tab::Performance => performance::egui_debug_performance(ui),
            Tab::Render => render::egui_debug_render(ui),
            Tab::Previews => previews::egui_debug_previews(ui, self.renderer),
            Tab::Diagnostics => diagnostics::egui_debug_diagnostics(ui),
            Tab::Camera => camera::egui_debug_camera(ui),
            Tab::Game => game::egui_debug_game(ui),
            Tab::Hud => hud::egui_debug_hud(ui, self.renderer),
            Tab::Environment => environment::render(ui),
            Tab::Vr => vr::egui_debug_vr(ui),
        }
    }

    // No close buttons anywhere: a closed tab would be unrecoverable until relaunch, and the
    // point of the dock is rearranging, not hiding. Drag a window's tab back to re-dock it.
    fn closeable(&mut self, _tab: &mut Tab) -> bool {
        false
    }
}

/// The dock tree, created (from disk or the default) on first UI frame.
static DOCK_STATE: Mutex<Option<egui_dock::DockState<Tab>>> = Mutex::new(None);

/// The last layout JSON written to (or read from) disk, to skip redundant writes.
static LAST_SAVED_LAYOUT: Mutex<Option<String>> = Mutex::new(None);

/// Frames since the layout was last checked for persistence.
static FRAMES_SINCE_SAVE_CHECK: Mutex<u32> = Mutex::new(0);

/// How often (in UI frames) to check whether the layout changed and save it.
const SAVE_CHECK_INTERVAL_FRAMES: u32 = 60;

/// The dock-layout file, next to the DLL like the trace output.
fn layout_path() -> Option<std::path::PathBuf> {
    Some(crate::module::get_path()?.parent()?.join("ui_layout.json"))
}

/// Load the persisted layout, falling back to the default single-group layout. Tabs missing from
/// the persisted tree (added since it was saved, or orphaned by a rename) are appended so nothing
/// is unreachable.
fn load_or_default_dock_state() -> egui_dock::DockState<Tab> {
    let loaded = layout_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| {
            let state = serde_json::from_str::<egui_dock::DockState<Tab>>(&s).ok()?;
            *LAST_SAVED_LAYOUT.lock() = Some(s);
            Some(state)
        });
    let mut state = match loaded {
        Some(state) => state,
        None => egui_dock::DockState::new(Tab::ALL.to_vec()),
    };
    for tab in Tab::ALL {
        if state.find_tab(&tab).is_none() {
            state.main_surface_mut().push_to_first_leaf(tab);
        }
    }
    state
}

/// Persist the layout if it changed since the last write. Called every UI frame; does the
/// serialize-and-compare only every [`SAVE_CHECK_INTERVAL_FRAMES`] frames.
fn save_dock_state_if_changed(state: &egui_dock::DockState<Tab>) {
    {
        let mut frames = FRAMES_SINCE_SAVE_CHECK.lock();
        *frames += 1;
        if *frames < SAVE_CHECK_INTERVAL_FRAMES {
            return;
        }
        *frames = 0;
    }
    let Ok(json) = serde_json::to_string(state) else {
        return;
    };
    let mut last = LAST_SAVED_LAYOUT.lock();
    if last.as_deref() == Some(json.as_str()) {
        return;
    }
    let Some(path) = layout_path() else {
        return;
    };
    if let Err(e) = std::fs::write(&path, &json) {
        tracing::warn!("failed to write the UI layout to {}: {e}", path.display());
        return;
    }
    *last = Some(json);
}

/// The injection confirmation: a large banner announcing this payload's build stamp for the first
/// seconds after startup, on whatever surface the egui overlay renders to (the flat overlay, and
/// the VR floating panel). Its *absence* after running the inject script is the tell that the
/// injection failed and the game is still running a stale resident payload — the failure mode this
/// exists to make obvious from inside the headset.
pub fn startup_banner(ctx: &egui::Context) {
    static FIRST_FRAME: OnceLock<Instant> = OnceLock::new();
    let age = FIRST_FRAME
        .get_or_init(Instant::now)
        .elapsed()
        .as_secs_f32();
    if age > BANNER_SECONDS {
        return;
    }
    egui::Area::new(egui::Id::new("jc3vrs_build_banner"))
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 48.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(egui::Color32::from_rgba_unmultiplied(0, 96, 0, 230))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "jc3vrs injected — build {}",
                            crate::BUILD_STAMP
                        ))
                        .size(26.0)
                        .strong()
                        .color(egui::Color32::WHITE),
                    );
                });
        });
}

/// How long (seconds) the startup banner stays up.
const BANNER_SECONDS: f32 = 15.0;

pub fn egui_debug_window(ui: &mut egui::Ui, renderer: &mut egui_directx11::Renderer) {
    ui.label(
        egui::RichText::new(format!("Build {}", crate::BUILD_STAMP))
            .weak()
            .small(),
    );

    let mut state_slot = DOCK_STATE.lock();
    let state = state_slot.get_or_insert_with(load_or_default_dock_state);

    // Close buttons are disabled everywhere on top of the viewer's `closeable = false` (which
    // already gates the floating-window close buttons): a closed tab is unrecoverable until
    // relaunch.
    egui_dock::DockArea::new(state)
        .id(egui::Id::new("jc3vrs_dock"))
        .style(egui_dock::Style::from_egui(ui.style().as_ref()))
        .show_close_buttons(false)
        .show_leaf_close_all_buttons(false)
        .show_inside(ui, &mut TabBodies { renderer });

    save_dock_state_if_changed(state);
}
