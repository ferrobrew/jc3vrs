//! Persisting the OpenXR instance and session across inject/uninject cycles: the handle stashes in the
//! game process environment, the reuse-or-create acquire paths bring-up runs, and the leak-on-persist
//! teardown paths. See [`VrConfig::persist_instance`] for why the runtime forces this.

use anyhow::Context as _;
use openxr as xr;
use openxr::sys::Handle as _;

use crate::vr::{VrConfig, session::Session};

/// Acquire an OpenXR instance: reuse a persisted handle if `persist` is set and one is stashed and
/// still live, otherwise create a fresh one. A stashed handle that fails to re-wrap or validate (the
/// runtime dropped it) is cleared and falls back to a fresh create.
pub(super) fn acquire_instance(
    entry: &xr::Entry,
    extensions: &xr::ExtensionSet,
    persist: bool,
) -> anyhow::Result<xr::Instance> {
    if persist && let Some(raw) = stashed_handle(INSTANCE_STASH_VAR) {
        match unsafe { reuse_instance(entry, raw, extensions) } {
            Ok(instance) => {
                tracing::info!(target: "vr", handle = format_args!("{raw:#x}"), "reused the persisted OpenXR instance");
                return Ok(instance);
            }
            Err(e) => {
                tracing::warn!(target: "vr", "the persisted OpenXR instance is unusable ({e:#}); creating a fresh one");
                clear_handle(INSTANCE_STASH_VAR);
            }
        }
    }
    entry
        .create_instance(
            &xr::ApplicationInfo {
                application_name: "jc3vr",
                application_version: 0,
                engine_name: "jc3vr",
                engine_version: 0,
                api_version: xr::Version::new(1, 0, 0),
            },
            extensions,
            &[],
        )
        .context("vr: creating the OpenXR instance")
}

/// Persist an OpenXR instance across inject cycles: stash its handle in the game process's
/// environment and leak the wrapper so its `Drop` never calls `xrDestroyInstance`, keeping the handle
/// live for the process lifetime for a later reinject to reuse. Consumes the instance so the two
/// halves (stash the handle, suppress the destroy) cannot be split. See [`VrConfig::persist_instance`].
pub(super) fn stash_instance(instance: xr::Instance) {
    stash_handle(INSTANCE_STASH_VAR, instance.as_raw().into_raw());
    std::mem::forget(instance);
}

/// Acquire an OpenXR session: reuse a persisted session if `cfg.persist_instance` is set and one is
/// stashed and still valid, otherwise create a fresh one. A stale stashed session is cleared and
/// falls back to a fresh create.
pub(super) fn acquire_session(
    instance: &xr::Instance,
    system: xr::SystemId,
    cfg: &VrConfig,
) -> anyhow::Result<Session> {
    if cfg.persist_instance
        && let Some(raw) = stashed_handle(SESSION_STASH_VAR)
    {
        match unsafe { reuse_session(instance, raw) } {
            Ok(session) => {
                tracing::info!(target: "vr", handle = format_args!("{raw:#x}"), "reused the persisted OpenXR session");
                return Ok(session);
            }
            Err(e) => {
                tracing::warn!(target: "vr", "the persisted OpenXR session is unusable ({e:#}); creating a fresh one");
                clear_handle(SESSION_STASH_VAR);
            }
        }
    }
    Session::create(instance, system, cfg)
}

/// Persist a session across inject cycles: destroy its recreatable children (swapchain, reference
/// space) but keep the session handle alive — stash it and leak the wrapper — *without* ending it, so
/// a reinject can re-wrap and resume it (an ended session cannot be resumed). Consumes the session.
pub(super) fn persist_session(session: Session) {
    let Session {
        handle,
        frame_wait,
        frame_stream,
        local,
        swapchain,
        running: _,
    } = session;
    // Destroy the cheap-to-recreate children; the reinject rebuilds them on the reused session.
    drop(swapchain);
    drop(local);
    // The frame waiter/stream hold session references but issue no XR destroy; dropping them just
    // releases those references (the leaked handle below keeps the session alive).
    drop(frame_wait);
    drop(frame_stream);
    stash_handle(SESSION_STASH_VAR, handle.as_raw().into_raw());
    std::mem::forget(handle);
}

/// Clear both persisted handles. Called when the runtime is genuinely stopped (not persisted for a
/// reinject) — a session loss, or `vr.enabled` turned off — so a later bring-up starts fresh.
pub(super) fn clear_persisted() {
    clear_handle(INSTANCE_STASH_VAR);
    clear_handle(SESSION_STASH_VAR);
}

/// The Windows process environment variables that persist the OpenXR instance and session handles
/// across inject/uninject cycles (see [`VrConfig::persist_instance`]). The payload DLL unmaps on
/// uninject, so a payload static cannot survive; the game process's environment block does. The
/// runtime allows only a small number of instances *and* sessions per process, so a reinject must
/// reuse both rather than create new ones.
const INSTANCE_STASH_VAR: windows::core::PCWSTR = windows::core::w!("jc3vr_xr_instance");
const SESSION_STASH_VAR: windows::core::PCWSTR = windows::core::w!("jc3vr_xr_session");

/// Store a handle value in the game process's environment under `var`, as hex.
fn stash_handle(var: windows::core::PCWSTR, raw: u64) {
    let value: Vec<u16> = format!("{raw:#x}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let _ = windows::Win32::System::Environment::SetEnvironmentVariableW(
            var,
            windows::core::PCWSTR(value.as_ptr()),
        );
    }
}

/// Read a persisted handle value from the game process's environment, if set and non-zero.
fn stashed_handle(var: windows::core::PCWSTR) -> Option<u64> {
    let mut buf = [0u16; 32];
    let len = unsafe {
        windows::Win32::System::Environment::GetEnvironmentVariableW(var, Some(&mut buf))
    };
    if len == 0 || len as usize >= buf.len() {
        return None;
    }
    let text = String::from_utf16_lossy(&buf[..len as usize]);
    u64::from_str_radix(text.trim().trim_start_matches("0x"), 16)
        .ok()
        .filter(|&v| v != 0)
}

/// Clear a persisted handle (a stale one that failed to reuse).
fn clear_handle(var: windows::core::PCWSTR) {
    unsafe {
        let _ = windows::Win32::System::Environment::SetEnvironmentVariableW(
            var,
            windows::core::PCWSTR::null(),
        );
    }
}

/// Re-wrap a persisted OpenXR instance handle: load the extension function table for it and confirm
/// it is live. See [`VrConfig::persist_instance`].
///
/// # Safety
/// `raw` must be an instance handle that was created with `extensions` and has not been destroyed.
unsafe fn reuse_instance(
    entry: &xr::Entry,
    raw: u64,
    extensions: &xr::ExtensionSet,
) -> anyhow::Result<xr::Instance> {
    let handle = xr::sys::Instance::from_raw(raw);
    let exts = unsafe { xr::InstanceExtensions::load(entry, handle, extensions) }
        .context("loading extensions for the persisted instance")?;
    let instance = unsafe { xr::Instance::from_raw(entry.clone(), handle, exts) }
        .context("wrapping the persisted instance handle")?;
    // Confirm the handle is actually live before committing to it.
    instance
        .properties()
        .context("querying the persisted instance")?;
    Ok(instance)
}

/// Re-wrap a persisted session handle: regenerate the frame waiter/stream (`Session::from_raw`) and
/// recreate the LOCAL reference space (which also validates the session still exists). The session
/// was persisted while `FOCUSED` (never ended), so `running` starts true; the swapchain is recreated
/// lazily on the first frame as usual.
///
/// # Safety
/// `raw` must be a D3D11 session handle created on `instance`, not currently inside a frame, and not
/// destroyed.
unsafe fn reuse_session(instance: &xr::Instance, raw: u64) -> anyhow::Result<Session> {
    let handle = xr::sys::Session::from_raw(raw);
    // An empty drop guard: the real one keeps the graphics device alive, but we share the game's
    // device, which outlives every VR session.
    let (session, frame_wait, frame_stream) =
        unsafe { xr::Session::<xr::D3D11>::from_raw(instance.clone(), handle, Box::new(())) };
    let local = session
        .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
        .context("recreating the LOCAL reference space for the persisted session")?;
    Ok(Session {
        handle: session,
        frame_wait,
        frame_stream,
        local,
        swapchain: None,
        running: true,
    })
}
