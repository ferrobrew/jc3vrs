//! egui debug UI: the tab bar and per-tab bodies, plus the startup build banner.

use std::{sync::OnceLock, time::Instant};

use parking_lot::Mutex;

pub mod camera;
pub mod debug;
pub mod environment;
pub mod game;
pub mod hud;
pub mod render;
pub mod vr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EguiTab {
    Render,
    Hud,
    Debug,
    Camera,
    Game,
    Environment,
    Vr,
}
impl std::fmt::Display for EguiTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
static EGUI_TAB: Mutex<EguiTab> = Mutex::new(EguiTab::Render);
impl EguiTab {
    fn ui(ui: &mut egui::Ui) {
        let mut tab = EGUI_TAB.lock();
        ui.horizontal(|ui| {
            for candidate_tab in [
                EguiTab::Render,
                EguiTab::Hud,
                EguiTab::Debug,
                EguiTab::Camera,
                EguiTab::Game,
                EguiTab::Environment,
                EguiTab::Vr,
            ] {
                if ui
                    .add(
                        egui::Button::new(candidate_tab.to_string())
                            .selected(*tab == candidate_tab),
                    )
                    .clicked()
                {
                    *tab = candidate_tab;
                }
            }
        });
    }

    fn get() -> EguiTab {
        *EGUI_TAB.lock()
    }
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
    EguiTab::ui(ui);

    match EguiTab::get() {
        EguiTab::Render => {
            render::egui_debug_render(ui, renderer);
        }
        EguiTab::Hud => {
            hud::egui_debug_hud(ui, renderer);
        }
        EguiTab::Debug => {
            debug::egui_debug_debug(ui);
        }
        EguiTab::Camera => {
            camera::egui_debug_camera(ui);
        }
        EguiTab::Game => {
            game::egui_debug_game(ui);
        }
        EguiTab::Environment => {
            environment::render(ui);
        }
        EguiTab::Vr => {
            vr::egui_debug_vr(ui);
        }
    }
}
