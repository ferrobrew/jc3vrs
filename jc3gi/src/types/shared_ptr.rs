#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A `boost::shared_ptr<T>`: the owned object pointer (`px`) and its reference-count control block
/// (`pn`). Sixteen bytes on this build. Use `as_ref` / `as_mut` to borrow the pointee, or
/// [`exists`](SharedPtr::exists) to test it.
pub struct SharedPtr<T> {
    /// The owned object (`px`), or null. Prefer `as_ref` / `as_mut` over dereferencing this directly.
    pub px: *mut T,
    /// The shared reference-count control block (`pn`).
    pub pn: *mut ::std::ffi::c_void,
}
impl<T> SharedPtr<T> {}
#[allow(dead_code)]
impl<T> SharedPtr<T> {
    /// `true` if the owned pointer is non-null.
    pub fn exists(&self) -> bool {
        !self.px.is_null()
    }
    /// Borrow the pointee, or `None` if the pointer is null.
    ///
    /// # Safety
    /// `px` must point to a live `T` for the duration of the borrow.
    pub unsafe fn as_ref(&self) -> Option<&T> {
        if self.px.is_null() { None } else { unsafe { Some(&*self.px) } }
    }
    /// Mutably borrow the pointee, or `None` if the pointer is null.
    ///
    /// # Safety
    /// `px` must point to a live `T` with no other live borrows for the duration.
    pub unsafe fn as_mut(&mut self) -> Option<&mut T> {
        if self.px.is_null() { None } else { unsafe { Some(&mut *self.px) } }
    }
}
