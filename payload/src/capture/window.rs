//! The standalone capture window: a separate HWND + DXGI swapchain the stereo composite presents
//! to, so the game's own window and present path are untouched.
//!
//! The window is created lazily on the first capture frame and destroyed on toggle-off or unmount.
//! It owns a `DXGI_SWAP_EFFECT_DISCARD` swapchain sized to `2 * eye_width x eye_height` (one half per
//! eye), matching the engine's own BitBlt model. The HWND lives on the game's main thread -- the same
//! thread that runs `graphics_flip` and the stereo Draw loop -- so creation, presenting, and
//! `DestroyWindow` all happen on one thread with no cross-thread HWND issues.
//!
//! The user toggles it off with F10 (Alt+F4 is ignored to keep teardown centralized in the capture
//! state machine, which destroys the window from `present_frame` rather than from inside its own
//! `WM_CLOSE`).

use std::sync::OnceLock;

use anyhow::Context as _;
use jc3gi::graphics_engine::device::Device;
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        Graphics::{
            Direct3D11::{ID3D11Device, ID3D11RenderTargetView},
            Dxgi::{
                Common::{
                    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_MODE_DESC, DXGI_MODE_SCALING_STRETCHED,
                    DXGI_MODE_SCANLINE_ORDER_PROGRESSIVE, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
                },
                DXGI_PRESENT, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_EFFECT_DISCARD, DXGI_USAGE,
                DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIFactory, IDXGISwapChain,
            },
        },
        UI::WindowsAndMessaging::{
            CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, HICON,
            IDC_ARROW, LoadCursorW, RegisterClassExW, WM_CLOSE, WM_DESTROY, WNDCLASSEXW,
            WS_EX_NOACTIVATE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        },
    },
    core::PCWSTR,
};

/// The window-class name. Registered once per process; the static atom guards against double
/// registration.
const CLASS_NAME: PCWSTR = windows::core::w!("JC3VRSCapture");
static CLASS_ATOM: OnceLock<u16> = OnceLock::new();

/// The capture window and its swapchain, sized to fit both eyes side by side.
pub(super) struct CaptureWindow {
    hwnd: HWND,
    swapchain: IDXGISwapChain,
    rtv: ID3D11RenderTargetView,
    /// Back-buffer dimensions in pixels (`width = 2 * eye_width`).
    size: (u32, u32),
}

// The HWND is a raw pointer wrapper, so it is not auto-`Send`. It is only ever touched on the game's
// main thread (create, present, destroy all happen there); the `Mutex<CaptureState>` requires `Send`
// for the static, so this is sound.
unsafe impl Send for CaptureWindow {}

impl CaptureWindow {
    /// Create the window, swapchain, and back-buffer RTV at `2 * eye_width x eye_height`.
    pub(super) fn new(device: &Device, eye_width: u32, eye_height: u32) -> anyhow::Result<Self> {
        let width = eye_width
            .checked_mul(2)
            .context("capture back-buffer width overflowed (2 * eye_width)")?;
        let height = eye_height;
        tracing::info!(target: "capture", "CaptureWindow::new: eye={}x{} back_buffer={}x{}", eye_width, eye_height, width, height);

        register_class()?;
        let hinstance = current_hinstance().context("getting the host module handle")?;
        let hwnd = create_hwnd(hinstance, width as i32, height as i32)?;
        let swapchain = create_swapchain(device, hwnd, width, height)?;
        let rtv = create_back_buffer_rtv(&device.m_Device, &swapchain)?;

        Ok(Self {
            hwnd,
            swapchain,
            rtv,
            size: (width, height),
        })
    }

    pub(super) fn size(&self) -> (u32, u32) {
        self.size
    }

    pub(super) fn rtv(&self) -> &ID3D11RenderTargetView {
        &self.rtv
    }

    /// Present the back buffer. `sync_interval = 1` for vsync, 0 for uncapped. Errors are logged by the
    /// caller; a failing present (e.g. after a mode change) usually means the swapchain needs to be
    /// rebuilt, which the caller does by recreating the window.
    pub(super) fn present(&self, sync_interval: u32) -> windows::core::Result<()> {
        unsafe {
            self.swapchain
                .Present(sync_interval, DXGI_PRESENT::default())
                .ok()
        }
    }

    /// Destroy the window and release the swapchain. Must be called on the thread that created the
    /// HWND (the game's main thread).
    pub(super) fn destroy(self) {
        tracing::info!(target: "capture", "CaptureWindow::destroy: DestroyWindow hwnd={:p}", self.hwnd.0);
        // The RTV and swapchain drop their COM refs here; DestroyWindow must come from the owning
        // thread. Drop order: views first, then swapchain, then the window.
        drop(self.rtv);
        drop(self.swapchain);
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Register the capture window class once per process. The atom is stored in a `OnceLock`; the
/// closure only runs on the first call and its result is reused. A zero atom is an error.
fn register_class() -> anyhow::Result<()> {
    if CLASS_ATOM.get().is_some() {
        return Ok(());
    }
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.context("loading IDC_ARROW")?;
    let wnd_class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(capture_wndproc),
        hInstance: current_hinstance()?,
        hCursor: cursor,
        hbrBackground: Default::default(), // NULL: no erase -- the swapchain covers the client area.
        lpszClassName: CLASS_NAME,
        hIcon: HICON::default(),
        hIconSm: HICON::default(),
        cbClsExtra: 0,
        cbWndExtra: 0,
        lpszMenuName: PCWSTR::null(),
    };
    let atom = unsafe { RegisterClassExW(&wnd_class) };
    if atom == 0 {
        let err = std::io::Error::last_os_error();
        // ERROR_CLASS_ALREADY_EXISTS (1410): the class was registered by a previous injection
        // and is still registered in this process. Safe to proceed — CreateWindowExW will
        // find it by name.
        if err.raw_os_error() != Some(1410) {
            return Err(anyhow::anyhow!("RegisterClassExW failed: {err}"));
        }
    }
    // Store the atom (or a non-zero placeholder on reinjection) so subsequent calls skip.
    CLASS_ATOM.set(atom.max(1)).ok();
    Ok(())
}

/// The capture window's procedure. Minimal: it ignores `WM_CLOSE` (F10 is the toggle, so Alt+F4
/// does nothing rather than racing the state machine's teardown) and lets `DefWindowProcW` handle
/// the rest. `WM_DESTROY` clears the live-HWND hint so the state machine can detect external
/// destruction.
unsafe extern "system" fn capture_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CLOSE => LRESULT(0),
        WM_DESTROY => {
            super::on_window_destroyed();
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Create a normal top-level window at the top-left of the primary monitor. `WS_EX_NOACTIVATE`
/// prevents the capture window from stealing focus from the game (JC3 pauses on
/// `WM_ACTIVATE(deactivate)`).
fn create_hwnd(hinstance: HINSTANCE, width: i32, height: i32) -> anyhow::Result<HWND> {
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE,
            CLASS_NAME,
            windows::core::w!("JC3VRS Capture"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            0,
            0,
            width,
            height,
            None,
            None,
            Some(hinstance),
            None,
        )
        .context("CreateWindowExW failed")?
    };
    Ok(hwnd)
}

/// Create a `DXGI_SWAP_EFFECT_DISCARD` swapchain on the same factory that owns the game's swapchain
/// (fetched via `GetParent`), so the two share present state. `BufferCount = 2` matches the engine.
fn create_swapchain(
    device: &Device,
    hwnd: HWND,
    width: u32,
    height: u32,
) -> anyhow::Result<IDXGISwapChain> {
    let factory: IDXGIFactory = unsafe { device.m_SwapChain.GetParent() }
        .context("getting the IDXGIFactory from the game's swapchain")?;
    let desc = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC {
            Width: width,
            Height: height,
            RefreshRate: DXGI_RATIONAL {
                Numerator: 0,
                Denominator: 0,
            }, // match the desktop
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_PROGRESSIVE,
            Scaling: DXGI_MODE_SCALING_STRETCHED,
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE(DXGI_USAGE_RENDER_TARGET_OUTPUT.0),
        BufferCount: 2,
        OutputWindow: hwnd,
        Windowed: windows::core::BOOL::default(),
        SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
        Flags: 0,
    };
    let mut swapchain: Option<IDXGISwapChain> = None;
    unsafe { factory.CreateSwapChain(&device.m_Device, &desc, &mut swapchain) }
        .ok()
        .context("IDXGIFactory::CreateSwapChain failed")?;
    swapchain.context("the capture swapchain was not created")
}

/// Build the render-target view over the swapchain's back buffer (index 0).
fn create_back_buffer_rtv(
    device: &ID3D11Device,
    swapchain: &IDXGISwapChain,
) -> anyhow::Result<ID3D11RenderTargetView> {
    use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
    let back_buffer = unsafe { swapchain.GetBuffer::<ID3D11Texture2D>(0) }
        .context("IDXGISwapChain::GetBuffer(0) failed")?;
    let mut rtv: Option<ID3D11RenderTargetView> = None;
    unsafe { device.CreateRenderTargetView(&back_buffer, None, Some(&mut rtv)) }
        .ok()
        .context("CreateRenderTargetView failed")?;
    rtv.context("the back-buffer RTV was not created")
}

/// The HMODULE/HINSTANCE of the host process (the JC3 exe). `GetModuleHandleW(None)` returns the
/// exe's handle, which is the same instance the game used to register its `"JC3"` class.
fn current_hinstance() -> anyhow::Result<HINSTANCE> {
    let module = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }
        .context("GetModuleHandleW(None) failed")?;
    Ok(HINSTANCE(module.0))
}
