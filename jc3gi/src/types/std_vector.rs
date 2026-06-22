#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A `std::vector<T>` (MSVC layout): the `_Myfirst` / `_Mylast` / `_Myend` pointers. This build's
/// vectors measure 0x20 -- a trailing allocator pointer beyond the three -- so the type is sized to
/// match (e.g. when used as a fixed array element).
pub struct Vector<T> {
    /// `_Myfirst`: start of the element array.
    pub begin: *mut T,
    /// `_Mylast`: one past the last live element.
    pub end: *mut T,
    /// `_Myend`: one past the allocated capacity.
    pub capacity_end: *mut T,
    _field_18: [u8; 8],
}
impl<T> Vector<T> {}
#[allow(dead_code)]
impl<T> Vector<T> {
    /// Number of live elements.
    pub fn len(&self) -> usize {
        if self.begin.is_null() {
            0
        } else {
            (self.end as usize - self.begin as usize) / ::core::mem::size_of::<T>()
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The live elements as a slice.
    ///
    /// # Safety
    /// `begin..end` must point to `len()` live `T`s for the borrow.
    pub unsafe fn as_slice(&self) -> &[T] {
        if self.begin.is_null() {
            &[]
        } else {
            unsafe { ::core::slice::from_raw_parts(self.begin, self.len()) }
        }
    }
}
