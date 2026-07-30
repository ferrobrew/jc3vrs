//! A borrowed handle to the engine's immediate D3D context.
//!
//! Borrowed rather than cloned: the paths that take this run per draw, and an `AddRef`/`Release` pair
//! each time would be wasted on a context the engine owns for the process's life. Resolving it once
//! and passing the handle is also what distinguishes it from a resolve-per-call helper -- the same
//! reason the single-pass draw paths hold one across a whole re-issue.

use jc3gi::graphics_engine::graphics_engine::GraphicsEngine;
use windows::Win32::{
    Graphics::Direct3D11::ID3D11DeviceContext,
    System::Threading::{CRITICAL_SECTION, EnterCriticalSection, LeaveCriticalSection},
};

/// The engine's immediate context, borrowed for the duration of a render-thread operation.
///
/// Borrowed rather than cloned: the per-eye re-issues take this per draw, and cloning the
/// `ID3D11DeviceContext` would be an `AddRef`/`Release` pair each time on a path whose whole purpose
/// is cutting per-draw cost. The engine owns the context for the process's life, so a `'static`
/// borrow is sound for as long as the engine is up -- which the render thread guarantees.
#[derive(Clone, Copy)]
pub(crate) struct EngineContext(&'static jc3gi::graphics_engine::device::Context);

impl EngineContext {
    /// The engine's immediate context, or `None` if the device/context is not live yet.
    pub(crate) fn get() -> Option<Self> {
        // SAFETY: read on the render thread, where the engine device/context pointers are stable.
        unsafe {
            let ge = GraphicsEngine::get()?;
            let device = ge.m_Device.as_ref()?;
            Some(Self(device.m_Context.as_ref()?))
        }
    }

    /// Run `f` on the D3D immediate context under the engine's own context mutex, which every other
    /// path in the mod that touches the context also takes.
    pub(crate) fn with_lock<R>(self, f: impl FnOnce(&ID3D11DeviceContext) -> R) -> R {
        // SAFETY: `m_Mutex` is the engine's live critical section for this context. The guard releases
        // it on drop, so the critical section is left even if `f` unwinds.
        unsafe {
            EnterCriticalSection(self.0.m_Mutex);
            let _guard = CritSecGuard(self.0.m_Mutex);
            f(&self.0.m_Context)
        }
    }
}

/// RAII guard that leaves the engine's critical section on drop, so the lock is released even if the
/// closure it protects unwinds. The payload builds with `panic = "abort"`, so unwinding does not
/// cross FFI in practice, but the guard keeps the invariant honest if the panic strategy ever
/// changes or a catch-unwind is introduced.
struct CritSecGuard(*mut CRITICAL_SECTION);

impl Drop for CritSecGuard {
    fn drop(&mut self) {
        // SAFETY: the guard is only constructed after a matching `EnterCriticalSection` on the same
        // critical section, and is dropped exactly once.
        unsafe { LeaveCriticalSection(self.0) };
    }
}
