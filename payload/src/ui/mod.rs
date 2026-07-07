//! egui debug UI: the tab bar and per-tab bodies.

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

pub fn egui_debug_window(ui: &mut egui::Ui, renderer: &mut egui_directx11::Renderer) {
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
