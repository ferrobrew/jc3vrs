#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The overlay UI (`COverlayUI`): the `CUIBase` hosting the `MCI_cursor` mouse-cursor clip, the
/// `MCI_black` fade, and the FPS readout inside the shared UI movie. The cursor is doubled:
/// while visible the engine both shows the OS arrow cursor (via `CGraphicsEngine::SetCursor`)
/// and repositions the movie's `MCI_cursor` clip every mouse move
/// ([`SetMouseCursorPosition`](OverlayUI::SetMouseCursorPosition)), so a cursor is also drawn
/// into the movie's own render target.
pub struct OverlayUI {
    _field_0: [u8; 20],
    /// The `CUIBase` activation state; `2` is active. [`SetMouseCursorPosition`](OverlayUI::SetMouseCursorPosition)
    /// and `CUIManager::MousePointerVisibility` no-op unless it is `2`.
    pub m_Current: u32,
    _field_18: [u8; 264],
    /// The managed [`Value`] for the movie's `MCI_cursor` clip, bound by `GetMember` when the
    /// overlay activates.
    pub m_MouseCursor: crate::ui::scaleform::Value,
    /// The cursor-visibility refcount. [`ShowMouseCursor`](OverlayUI::ShowMouseCursor) increments
    /// it, [`HideMouseCursor`](OverlayUI::HideMouseCursor) decrements it; the 0-to-1 and 1-to-0
    /// transitions switch `CGraphicsEngine`'s active cursor between `Arrow` and `None` (which also
    /// controls the cursor clip to the client rect).
    pub m_MouseCursorShowRefCount: i32,
    _field_154: [u8; 4],
}
fn _OverlayUI_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x158], OverlayUI>([0u8; 0x158]);
    }
    unreachable!()
}
impl OverlayUI {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418223704usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl OverlayUI {
    pub const ShowMouseCursor_ADDRESS: usize = 0x140DFB960;
    /// Increments [`m_MouseCursorShowRefCount`](OverlayUI::m_MouseCursorShowRefCount); the 0-to-1
    /// transition sets `CGraphicsEngine::SetCursor(Arrow)`.
    pub unsafe fn ShowMouseCursor(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::ShowMouseCursor_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const HideMouseCursor_ADDRESS: usize = 0x140DFB920;
    /// Decrements [`m_MouseCursorShowRefCount`](OverlayUI::m_MouseCursorShowRefCount) (warning if
    /// already zero); at zero it sets `CGraphicsEngine::SetCursor(None)`.
    pub unsafe fn HideMouseCursor(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::HideMouseCursor_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const SetMouseCursorPosition_ADDRESS: usize = 0x140E4F260;
    /// Moves the `MCI_cursor` clip to `(x, y)` in movie stage coordinates (the space
    /// `CUIManager::GetMovieSpaceMouseCursor` produces) via a `DisplayInfo` X/Y write. No-ops
    /// unless the overlay is active.
    pub unsafe fn SetMouseCursorPosition(&mut self, x: f32, y: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, x: f32, y: f32) = ::std::mem::transmute(
                Self::SetMouseCursorPosition_ADDRESS,
            );
            f(self as *mut Self as _, x, y)
        }
    }
}
impl std::convert::AsRef<OverlayUI> for OverlayUI {
    fn as_ref(&self) -> &OverlayUI {
        self
    }
}
impl std::convert::AsMut<OverlayUI> for OverlayUI {
    fn as_mut(&mut self) -> &mut OverlayUI {
        self
    }
}
