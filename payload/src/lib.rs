use std::{ffi::c_void, sync::OnceLock};

use parking_lot::Mutex;
use windows::Win32::{
    Foundation::HMODULE,
    Graphics::{
        Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
            ID3D11ShaderResourceView, ID3D11Texture2D,
        },
        Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC},
    },
    System::{LibraryLoader::DisableThreadLibraryCalls, SystemServices::DLL_PROCESS_ATTACH},
    UI::Input::KeyboardAndMouse::{VK_F5, VK_F6},
};

use crate::egui_impl::EguiState;

pub mod egui_impl;
pub mod module;
pub mod util;

mod hooks;
mod logging;

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(module: HMODULE, reason: u32, _unk: *mut c_void) -> bool {
    if reason == DLL_PROCESS_ATTACH {
        unsafe {
            DisableThreadLibraryCalls(module).ok();
            module::set(module);
        };
    }
    true
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn run(_: *mut c_void) {
    initialize_startup();
}

/// Called when the DLL is loaded
fn initialize_startup() {
    std::panic::set_hook(Box::new(|info| {
        let payload = info.payload();

        #[allow(clippy::manual_map)]
        let payload = if let Some(s) = payload.downcast_ref::<&str>() {
            Some(&**s)
        } else if let Some(s) = payload.downcast_ref::<String>() {
            Some(s.as_str())
        } else {
            None
        };

        let location = info.location().map(|l| l.to_string());
        let backtrace = std::backtrace::Backtrace::capture();

        tracing::error!(
            panic.payload = payload,
            panic.location = location,
            panic.backtrace = tracing::field::display(backtrace),
            "A panic occurred",
        );
    }));

    logging::install();
    tracing::info!("JC3VRS startup");
    hooks::install();
}

/// Called to undo `initialize_startup` and eject
fn shutdown_startup() {
    tracing::info!("Uninstalling hooks");
    hooks::uninstall();

    // Wait to ensure we're clear of the blast radius of the hooks
    std::thread::sleep(std::time::Duration::from_millis(100));

    tracing::info!("Ejecting");
    logging::uninstall();
    module::exit();
}

/// Called when we're on the game thread for the first time
fn initialize_from_game() -> anyhow::Result<()> {
    static INITIALIZED: OnceLock<bool> = OnceLock::new();
    if INITIALIZED.get().is_some() {
        return Ok(());
    }
    INITIALIZED.set(true).unwrap();

    EguiState::install()?;
    tracing::info!("Initialized in game thread");

    Ok(())
}

/// Called to undo `initialize_from_game`; called once shutdown is triggered
fn shutdown_from_game() {
    if let Some(egui_state) = EguiState::get().as_mut() {
        let mut state = EGUI_DEBUG_RENDER_STATE.lock();
        state.uninstall(&mut egui_state.egui_renderer);
    }
    EguiState::uninstall();
}

/// Request that we shut down and exit
fn shutdown() {
    static SHUTDOWN: OnceLock<bool> = OnceLock::new();
    if SHUTDOWN.get().is_some() {
        return;
    }
    SHUTDOWN.set(true).unwrap();

    tracing::info!("Shutting down");
    shutdown_from_game();
    std::thread::spawn(shutdown_startup);
}

fn update() {
    if let Err(e) = initialize_from_game() {
        tracing::error!("Failed to initialize in game thread, shutting down: {e:?}");
        shutdown();
        return;
    }

    let panic = std::panic::catch_unwind(|| {
        if util::is_pressed(VK_F5) {
            shutdown();
            return;
        }

        if let Some(egui_state) = EguiState::get().as_mut() {
            if util::is_pressed(VK_F6) {
                egui_state.toggle_game_input_capture();
            }

            egui_state.run(|ctx, renderer| {
                egui::Window::new("Debug").show(ctx, |ui| egui_debug_window(ui, renderer));
            });
        }
    });
    if let Err(e) = panic {
        let panic_msg = e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()));
        tracing::error!("Panic in update, shutting down: {panic_msg:?}");
        shutdown();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EguiTab {
    Render,
    Camera,
    Game,
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
            for candidate_tab in [EguiTab::Render, EguiTab::Camera, EguiTab::Game] {
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
            egui_debug_render(ui, renderer);
        }
        EguiTab::Camera => {
            egui_debug_camera(ui);
        }
        EguiTab::Game => {
            egui_debug_game(ui);
        }
    }
}

struct EguiDebugRenderState {
    target_texture: Option<(ID3D11Texture2D, egui::TextureId)>,
}
impl EguiDebugRenderState {
    const fn new() -> Self {
        Self {
            target_texture: None,
        }
    }

    fn prepare_if_necessary(&mut self, renderer: &mut egui_directx11::Renderer) {
        if self.target_texture.is_some() {
            return;
        }
        unsafe {
            let Some(ge) = jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() else {
                return;
            };

            let Some(device) = ge.m_Device.as_mut() else {
                return;
            };

            let Some(back_buffer) = device.m_BackBuffer.as_ref() else {
                return;
            };

            let mut texture: Option<ID3D11Texture2D> = None;
            // TODO: recreate on resize
            // TODO: figure out why this lcoks up / crashes over time
            if let Err(e) = device.m_Device.CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: back_buffer.m_Width as u32,
                    Height: back_buffer.m_Height as u32,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as _,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                },
                None,
                Some(&mut texture),
            ) {
                tracing::error!("Failed to create texture: {e:?}");
                return;
            }
            let Some(texture) = texture else {
                tracing::error!("Failed to create texture");
                return;
            };

            let mut srv: Option<ID3D11ShaderResourceView> = None;
            if let Err(e) = device
                .m_Device
                .CreateShaderResourceView(&texture, None, Some(&mut srv))
            {
                tracing::error!("Failed to create shader resource view: {e:?}");
                return;
            }
            let Some(srv) = srv else {
                tracing::error!("Failed to create shader resource view");
                return;
            };

            self.target_texture = Some((texture, renderer.register_user_texture(srv)));
        }
    }

    pub fn texture(&self) -> Option<&ID3D11Texture2D> {
        self.target_texture.as_ref().map(|(texture, _)| texture)
    }

    fn uninstall(&mut self, renderer: &mut egui_directx11::Renderer) {
        if let Some((_, texture_id)) = self.target_texture.take() {
            renderer.unregister_user_texture(texture_id);
        }
    }
}
static EGUI_DEBUG_RENDER_STATE: Mutex<EguiDebugRenderState> =
    Mutex::new(EguiDebugRenderState::new());

fn egui_debug_render(ui: &mut egui::Ui, renderer: &mut egui_directx11::Renderer) {
    let mut state = EGUI_DEBUG_RENDER_STATE.lock();
    state.prepare_if_necessary(renderer);

    if let Some((_, texture_id)) = state.target_texture
        && let Some((width, height)) = unsafe {
            jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get()
                .and_then(|ge| ge.m_MainColorBuffer.as_mut())
                .map(|mcb| (mcb.m_Width as usize, mcb.m_Height as usize))
        }
    {
        let size = egui::vec2(width as f32, height as f32);
        ui.add(egui::Image::new(egui::ImageSource::Texture(
            egui::load::SizedTexture {
                id: texture_id,
                size: size / 4.0,
            },
        )));
    }
}

fn egui_debug_camera(ui: &mut egui::Ui) {
    let mut cs = hooks::camera::CAMERA_SETTINGS.lock();
    ui.checkbox(&mut cs.enabled, "Enabled");
    ui.checkbox(&mut cs.always_use_t1, "Always use T1");
    ui.checkbox(&mut cs.blurs_enabled, "Blurs");
    ui.checkbox(&mut cs.use_eye_matrices, "Use eye matrices");

    ui.add_enabled_ui(!cs.use_eye_matrices, |ui| {
        use egui::Slider;
        ui.add(Slider::new(&mut cs.head_offset.x, -1.0..=1.0).text("Head X"));
        ui.add(Slider::new(&mut cs.head_offset.y, -1.0..=1.0).text("Head Y"));
        ui.add(Slider::new(&mut cs.head_offset.z, -1.0..=1.0).text("Head Z"));

        ui.add(Slider::new(&mut cs.body_offset.x, -1.0..=1.0).text("Body X"));
        ui.add(Slider::new(&mut cs.body_offset.y, -1.0..=1.0).text("Body Y"));
        ui.add(Slider::new(&mut cs.body_offset.z, -1.0..=1.0).text("Body Z"));
    });
}

fn egui_debug_game(ui: &mut egui::Ui) {
    unsafe {
        let Some(game) = jc3gi::game::Game::get() else {
            return;
        };
        let Some(clock) = jc3gi::clock::Clock::get() else {
            return;
        };

        ui.heading("Game");
        ui.label(format!("Update frequency: {}Hz", game.m_UpdateFrequency));
        ui.label(format!("Update flags: {:X}", game.m_UpdateFlags));
        ui.label(format!(
            "Interpolation method: {:X}",
            game.m_InterpolationMethod
        ));
        {
            let mut interpolation_override = game.m_InterpolationOverride;
            let before = interpolation_override;
            egui::ComboBox::from_label("Interpolation override")
                .selected_text(interpolation_override.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut interpolation_override, -1, "Really None");
                    ui.selectable_value(&mut interpolation_override, 0, "None");
                    ui.selectable_value(&mut interpolation_override, 1, "1");
                    ui.selectable_value(&mut interpolation_override, 2, "2");
                    ui.selectable_value(&mut interpolation_override, 3, "3");
                });

            if before != interpolation_override
                && let Some(mut patcher) = hooks::patcher()
            {
                patcher.patch(
                    &mut game.m_InterpolationOverride as *mut _ as usize,
                    &interpolation_override.to_le_bytes(),
                );
            }
        }
        patchbox(ui, "Decouple enabled", &mut game.m_DecoupleEnabled);

        ui.heading("Clock");
        ui.label(format!("FPS: {}", clock.m_FPS));
        ui.label(format!("SPF: {}", clock.m_SPF));
        ui.label(format!("Real FPS: {}", clock.m_RealFPS));
        ui.label(format!("Real SPF: {}", clock.m_RealSPF));
        ui.label(format!("Update speed: {}", clock.m_UpdateSpeed));
        ui.label(format!("Force to FPS: {}", clock.m_ForceToThisFPS));
        ui.label(format!("Force to SPF: {}", clock.m_ForceToThisSPF));
        patchbox(ui, "Stop", &mut clock.m_Stop);
        patchbox(ui, "Force to FPS", &mut clock.m_ForceToFps);
    }
}

fn patchbox(ui: &mut egui::Ui, label: &str, value: *mut bool) {
    let mut enabled = unsafe { *value };
    if ui.checkbox(&mut enabled, label).changed()
        && let Some(mut patcher) = hooks::patcher()
    {
        unsafe {
            patcher.patch(value as *const _ as usize, &[if enabled { 1 } else { 0 }]);
        }
    }
}
