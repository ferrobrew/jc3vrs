#![cfg_attr(any(), rustfmt::skip)]
#[allow(unused_imports)]
use crate::character::character::Character;
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
