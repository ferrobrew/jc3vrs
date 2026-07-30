//! The allocation-free line writer the crash handler formats through, and the raw log handle it
//! writes to. See the [`crate::crash`] module docs for why `format!`/`write!` and `std::io` are
//! unusable here.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::{
    Foundation::HANDLE, Storage::FileSystem::WriteFile, System::SystemInformation::GetSystemTime,
};

/// A fixed-capacity line builder that formats directly into a stack buffer, with no heap allocation
/// and no `core::fmt` machinery. See the crash module docs for why the crash handler cannot use
/// `format!`/`write!`. Appends silently truncate once the buffer is full.
pub(super) struct Line {
    buf: [u8; 512],
    len: usize,
}

impl Line {
    pub(super) fn new() -> Self {
        Self {
            buf: [0u8; 512],
            len: 0,
        }
    }

    /// Append raw bytes, truncating at capacity.
    pub(super) fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        let n = bytes.len().min(self.buf.len() - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&bytes[..n]);
        self.len += n;
        self
    }

    /// Append the bytes of `s`, truncating at capacity.
    pub(super) fn str(&mut self, s: &str) -> &mut Self {
        self.bytes(s.as_bytes())
    }

    /// Append a single byte, dropping it at capacity.
    pub(super) fn byte(&mut self, b: u8) -> &mut Self {
        if self.len < self.buf.len() {
            self.buf[self.len] = b;
            self.len += 1;
        }
        self
    }

    /// Append `value` as `0x`-prefixed uppercase hex, zero-padded to at least `width` digits.
    pub(super) fn hex(&mut self, value: u64, width: usize) -> &mut Self {
        self.str("0x");
        let mut digits = [0u8; 16];
        let mut n = 0;
        let mut v = value;
        loop {
            digits[n] = b"0123456789ABCDEF"[(v & 0xf) as usize];
            n += 1;
            v >>= 4;
            if v == 0 {
                break;
            }
        }
        for _ in n..width {
            self.byte(b'0');
        }
        for i in (0..n).rev() {
            self.byte(digits[i]);
        }
        self
    }

    /// Append `value` as decimal.
    pub(super) fn dec(&mut self, value: u64) -> &mut Self {
        let mut digits = [0u8; 20];
        let mut n = 0;
        let mut v = value;
        loop {
            digits[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        for i in (0..n).rev() {
            self.byte(digits[i]);
        }
        self
    }

    /// Append a newline and write the line straight to the crash log with a single `WriteFile`. No
    /// heap, no mutex, no `std::io` -- just the stored handle and the stack buffer.
    pub(super) fn flush(&mut self) {
        self.byte(b'\n');
        let raw = CRASH_LOG.load(Ordering::Relaxed);
        if raw == 0 {
            return;
        }
        let handle = HANDLE(raw as *mut std::ffi::c_void);
        // SAFETY: `handle` is the append-mode log handle from `install`; `self.buf[..self.len]` is a
        // valid slice. A failed write is ignored -- there is nowhere better to report it.
        unsafe {
            let _ = WriteFile(handle, Some(&self.buf[..self.len]), None, None);
        }
    }

    /// Append `value` as decimal, zero-padded to at least `width` digits.
    fn dec_pad(&mut self, value: u64, width: usize) -> &mut Self {
        let mut digits = 1;
        let mut v = value;
        while v >= 10 {
            digits += 1;
            v /= 10;
        }
        for _ in digits..width {
            self.byte(b'0');
        }
        self.dec(value)
    }
}

/// The raw file handle for crash-handler writes, opened once at [`crate::crash::install`] and never
/// closed (the process is dying). `0` means "not opened". Stored as an `isize` so it lives in an
/// atomic without a lock -- the handler must not touch `parking_lot` or `std::io`.
pub(super) static CRASH_LOG: AtomicIsize = AtomicIsize::new(0);

/// Append the current UTC wall-clock time as `YYYY-MM-DD HH:MM:SS.mmm UTC `. `GetSystemTime`
/// fills a plain struct with no allocation or locking, so this is safe from the handler, and the
/// UTC base matches the tracing subscriber's timestamps in `jc3vrs.log` for cross-correlation.
pub(super) fn stamp(line: &mut Line) {
    let time = unsafe { GetSystemTime() };
    line.dec_pad(time.wYear as u64, 4)
        .byte(b'-')
        .dec_pad(time.wMonth as u64, 2)
        .byte(b'-')
        .dec_pad(time.wDay as u64, 2)
        .byte(b' ')
        .dec_pad(time.wHour as u64, 2)
        .byte(b':')
        .dec_pad(time.wMinute as u64, 2)
        .byte(b':')
        .dec_pad(time.wSecond as u64, 2)
        .byte(b'.')
        .dec_pad(time.wMilliseconds as u64, 3)
        .str(" UTC ");
}
