#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A `std::string` in the MSVC layout: the small-string-optimization buffer/pointer union
/// (`_Bx`), then the length (`_Mysize`) and capacity (`_Myres`). The string is stored inline in
/// [`buffer`](String::buffer) while `capacity < 16`; otherwise the first eight bytes of the
/// buffer are a heap pointer to the character data.
pub struct String {
    /// `_Bx`: the inline character buffer, or (heap case) a `char*` in the first eight bytes.
    pub buffer: [u8; 16],
    /// `_Mysize`: the length in bytes, excluding the NUL terminator.
    pub size: u64,
    /// `_Myres`: the allocated capacity in bytes, excluding the NUL terminator. `>= 16` means the
    /// data lives on the heap.
    pub capacity: u64,
}
fn _String_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], String>([0u8; 0x20]);
    }
    unreachable!()
}
impl String {}
impl std::convert::AsRef<String> for String {
    fn as_ref(&self) -> &String {
        self
    }
}
impl std::convert::AsMut<String> for String {
    fn as_mut(&mut self) -> &mut String {
        self
    }
}
#[allow(dead_code)]
impl String {
    /// The string bytes, resolving the small-string optimization.
    ///
    /// # Safety
    /// The string must be live and unmodified for the borrow: in the heap case the returned
    /// slice aliases the allocation the first eight buffer bytes point to.
    pub unsafe fn as_bytes(&self) -> &[u8] {
        let data: *const u8 = if self.capacity < 16 {
            self.buffer.as_ptr()
        } else {
            unsafe {
                ::core::ptr::read_unaligned(self.buffer.as_ptr() as *const *const u8)
            }
        };
        unsafe { ::core::slice::from_raw_parts(data, self.size as usize) }
    }
}
