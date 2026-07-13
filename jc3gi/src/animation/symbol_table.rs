#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The animation-system event-id (act) symbol table: maps act names (`ACT_*`) to the sequential
/// runtime ids that the animation state machines and [`Character::QueueAct`](character::character::Character::QueueAct)
/// use. Ids are assigned in registration order as names are first encountered (by loaded animation
/// data or the `NCharacter` id globals), so an act's id is only resolvable at runtime.
pub struct EventIdSymbolTable {}
impl EventIdSymbolTable {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418083080usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl EventIdSymbolTable {
    pub const string_to_id_ADDRESS: usize = 0x140413020;
    /// Resolves an act name to its runtime id. Registers the name with a fresh id when it is not
    /// yet present (asserting if the table has been locked), so resolving a name that no loaded
    /// animation data uses yields an id that no queued act will ever carry.
    pub unsafe fn string_to_id(&mut self, name: *const u8) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, name: *const u8) -> i32 = ::std::mem::transmute(
                Self::string_to_id_ADDRESS,
            );
            f(self as *mut Self as _, name)
        }
    }
}
impl EventIdSymbolTable {
    /// The on-foot / land-vehicle reversing act: drives the `S_REVERSE_*` states in
    /// `rico_base.afsmb`, where Rico turns to look over his shoulder while backing up.
    pub const ACT_REVERSE: &str = "ACT_REVERSE";
    /// The motorbike variant of [`ACT_REVERSE`](EventIdSymbolTable::ACT_REVERSE).
    pub const ACT_REVERSE_MOTORBIKE: &str = "ACT_REVERSE_MOTORBIKE";
    /// The one-off idle-variation act: transitions the on-foot idle from `S_IDLE` to
    /// `S_IDLE_ONE_OFF` in `rico_base.afsmb`, playing a single idle-break animation (a weight shift,
    /// a look-around, a fidget) before returning to `S_IDLE`. `NStateTask_InputLocoIdleTask::Update`
    /// queues it via [`Character::QueueAct`](character::character::Character::QueueAct) once a
    /// per-character idle countdown (reset to a random ~8.5-12 s each time the character moves)
    /// elapses while the character is standing still, calm, and not attached to a vehicle.
    pub const ACT_TO_IDLE_ONE_OFF: &str = "ACT_TO_IDLE_ONE_OFF";
}
impl std::convert::AsRef<EventIdSymbolTable> for EventIdSymbolTable {
    fn as_ref(&self) -> &EventIdSymbolTable {
        self
    }
}
impl std::convert::AsMut<EventIdSymbolTable> for EventIdSymbolTable {
    fn as_mut(&mut self) -> &mut EventIdSymbolTable {
        self
    }
}
