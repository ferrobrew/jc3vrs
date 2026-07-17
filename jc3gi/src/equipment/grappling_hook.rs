#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The grappling-hook equipment item (`CGrapplingHook`, a `CGameObject`): owns the wires, the
/// reel/tether state machine, and the cached fire target. A character's hook is held by its
/// inventory ([`Inventory::m_GrapplingHook`](character::inventory::Inventory::m_GrapplingHook)).
pub struct GrapplingHook {
    _field_0: [u8; 564],
    /// The current state-machine state; see [`GrapplingHookState`]. The state records the *last*
    /// reel outcome as much as the current one: leaving a `GHS_REELED_*` attachment does not
    /// reliably return it to [`GHS_INACTIVE`](GrapplingHookState::GHS_INACTIVE) — readers such as
    /// `IsHookAimingState` and `ShouldShowAttachedUI` therefore combine it with
    /// [`m_ActiveWire`](GrapplingHook::m_ActiveWire) rather than trusting it alone.
    pub m_State: crate::equipment::grappling_hook::GrapplingHookState,
    _field_238: [u8; 56],
    /// The countdown (seconds) armed when a hook fire is committed, within which the fire
    /// animation must emit its release track message before the fire is abandoned. Set to `2.0`
    /// alongside [`m_WaitingForGrappleFire`](GrapplingHook::m_WaitingForGrappleFire) by the fire
    /// dispatchers (`FireHookTowardsPos`, `TryFireHook`).
    pub m_WaitForFireSignalTimeout: f32,
    /// Whether a grapple-hook fire is committed and awaiting the fire animation's release track
    /// message: set when the fire dispatches (before the fire act plays and the wire spawns),
    /// cleared when the hook releases or the fire times out. The fire act's directional alignment
    /// of the character runs within this window.
    pub m_WaitingForGrappleFire: bool,
    /// The dual-tether counterpart of
    /// [`m_WaitingForGrappleFire`](GrapplingHook::m_WaitingForGrappleFire); the two are cleared
    /// together as one word.
    pub m_WaitingForTetherFire: bool,
    _field_276: [u8; 338],
    /// The wire currently driving the reel/attach flow, or null when none is live. The state
    /// readers null-check this before interpreting [`m_State`](GrapplingHook::m_State).
    pub m_ActiveWire: crate::types::shared_ptr::SharedPtr<
        crate::equipment::grappling_hook::GrapplingHookWire,
    >,
}
fn _GrapplingHook_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x3D8], GrapplingHook>([0u8; 0x3D8]);
    }
    unreachable!()
}
impl GrapplingHook {}
impl std::convert::AsRef<GrapplingHook> for GrapplingHook {
    fn as_ref(&self) -> &GrapplingHook {
        self
    }
}
impl std::convert::AsMut<GrapplingHook> for GrapplingHook {
    fn as_mut(&mut self) -> &mut GrapplingHook {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The grappling hook's high-level state machine (`CGrapplingHook::m_State`). `SetState` is the
/// mutator; the reel tasks (`NStateTask_ReelIn`) and the reeled-in controller
/// (`NReeledInController`) drive the transitions. A reel-to-target starts in
/// [`GHS_REELING_IN`](GrapplingHookState::GHS_REELING_IN) -- during which
/// `NReeledInController::RotateToGrappleTarget` rotates the character's root toward the target as
/// the reel pulls them in -- and completes into one of the `GHS_REELED_*` attachment states
/// depending on the landing surface and pose. `GHS_CUSTOM_ACTIVE_WIRE` is a wire held open outside
/// the reel flow (tethers).
pub enum GrapplingHookState {
    GHS_INITIALIZING = 0isize as _,
    GHS_INACTIVE = 1isize as _,
    GHS_REELING_IN = 2isize as _,
    GHS_REELED_ATTACHED = 3isize as _,
    GHS_REELED_HANG = 4isize as _,
    GHS_REELED_UPSIDEDOWN = 5isize as _,
    GHS_REELED_STUNT = 6isize as _,
    GHS_CUSTOM_ACTIVE_WIRE = 7isize as _,
}
fn _GrapplingHookState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], GrapplingHookState>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// A single grapple wire (`CGrapplingHookWire`, a `CGameObject`): two `CWireEnd`s (near/device and
/// far/hook), the physics constraint, and the wire rendering. Only referenced through
/// [`GrapplingHook::m_ActiveWire`] here; the layout is unmapped.
pub struct GrapplingHookWire {}
impl GrapplingHookWire {}
impl std::convert::AsRef<GrapplingHookWire> for GrapplingHookWire {
    fn as_ref(&self) -> &GrapplingHookWire {
        self
    }
}
impl std::convert::AsMut<GrapplingHookWire> for GrapplingHookWire {
    fn as_mut(&mut self) -> &mut GrapplingHookWire {
        self
    }
}
