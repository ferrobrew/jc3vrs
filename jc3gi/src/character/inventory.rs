#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The character's equipment inventory (`CInventory`), embedded in `CCharacter`
/// ([`Character::m_Inventory`](character::character::Character::m_Inventory)). Only the
/// grappling-hook slot is mapped.
pub struct Inventory {
    _field_0: [u8; 192],
    /// The character's grappling hook, or null before the inventory is populated.
    /// `CCharacter::GetGrapplingHook` returns a reference to this field.
    pub m_GrapplingHook: crate::types::shared_ptr::SharedPtr<
        crate::equipment::grappling_hook::GrapplingHook,
    >,
}
fn _Inventory_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xD0], Inventory>([0u8; 0xD0]);
    }
    unreachable!()
}
impl Inventory {}
impl std::convert::AsRef<Inventory> for Inventory {
    fn as_ref(&self) -> &Inventory {
        self
    }
}
impl std::convert::AsMut<Inventory> for Inventory {
    fn as_mut(&mut self) -> &mut Inventory {
        self
    }
}
