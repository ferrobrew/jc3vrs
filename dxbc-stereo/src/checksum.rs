//! The DXBC container checksum: a modified MD5 over the bytes past the 20-byte header.
//!
//! Editing a shader's bytecode leaves the container's stored hash stale, and the D3D stack under
//! Proton rejects a blob whose hash no longer matches -- so every bytecode patch must finish by
//! refreshing the checksum. The hash lives here rather than in the payload because it belongs to
//! the container format, not to any one hook.

/// Recompute the DXBC container checksum over a patched blob and write it into the 16-byte hash
/// field at offset `0x4`. The checksum is a modified MD5 (see [`dxbc_hash`]) over every byte after
/// the 20-byte header; a consumer that validates it rejects a blob whose stored hash no longer
/// matches the bytecode, so an in-place patch must refresh it. A blob too small to hold a header is
/// left untouched.
pub fn refresh_checksum(blob: &mut [u8]) {
    const HEADER_LEN: usize = 20;
    if blob.len() < HEADER_LEN {
        return;
    }
    let hash = dxbc_hash(&blob[HEADER_LEN..]);
    blob[4..HEADER_LEN].copy_from_slice(&hash);
}

/// The DXBC container hash: a modified MD5 over `data` (the bytes after the 20-byte header). It differs
/// from standard MD5 only in the final block -- the message bit length is prepended to the trailing
/// block rather than appended, and the closing dword is `(bits >> 2) | 1` -- so it cannot be produced
/// with a stock MD5 finaliser.
fn dxbc_hash(data: &[u8]) -> [u8; 16] {
    let mut state = MD5_INIT;
    let n = data.len();
    // The DXBC format stores a 32-bit bit-length, matching this u32 cast.
    let num_bits = (n as u32).wrapping_mul(8);
    let left_over = n % 64;
    let full = n - left_over;

    let mut i = 0;
    while i < full {
        let mut block = [0u8; 64];
        block.copy_from_slice(&data[i..i + 64]);
        md5_transform(&mut state, &block);
        i += 64;
    }
    let tail = &data[full..];
    let closing = ((num_bits >> 2) | 1).to_le_bytes();
    if left_over >= 56 {
        let mut block = [0u8; 64];
        block[..left_over].copy_from_slice(tail);
        block[left_over] = 0x80;
        md5_transform(&mut state, &block);
        let mut last = [0u8; 64];
        last[0..4].copy_from_slice(&num_bits.to_le_bytes());
        last[60..64].copy_from_slice(&closing);
        md5_transform(&mut state, &last);
    } else {
        let mut block = [0u8; 64];
        block[0..4].copy_from_slice(&num_bits.to_le_bytes());
        block[4..4 + left_over].copy_from_slice(tail);
        block[4 + left_over] = 0x80;
        block[60..64].copy_from_slice(&closing);
        md5_transform(&mut state, &block);
    }

    let mut out = [0u8; 16];
    for (chunk, word) in out.chunks_exact_mut(4).zip(state) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    out
}

const MD5_INIT: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];

/// Standard MD5 per-round left-rotation amounts.
#[rustfmt::skip]
const MD5_SHIFTS: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
    5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
    4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
    6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// Standard MD5 per-round additive constants, `floor(abs(sin(i + 1)) * 2^32)`.
#[rustfmt::skip]
const MD5_K: [u32; 64] = [
    0xd76a_a478, 0xe8c7_b756, 0x2420_70db, 0xc1bd_ceee, 0xf57c_0faf, 0x4787_c62a, 0xa830_4613, 0xfd46_9501,
    0x6980_98d8, 0x8b44_f7af, 0xffff_5bb1, 0x895c_d7be, 0x6b90_1122, 0xfd98_7193, 0xa679_438e, 0x49b4_0821,
    0xf61e_2562, 0xc040_b340, 0x265e_5a51, 0xe9b6_c7aa, 0xd62f_105d, 0x0244_1453, 0xd8a1_e681, 0xe7d3_fbc8,
    0x21e1_cde6, 0xc337_07d6, 0xf4d5_0d87, 0x455a_14ed, 0xa9e3_e905, 0xfcef_a3f8, 0x676f_02d9, 0x8d2a_4c8a,
    0xfffa_3942, 0x8771_f681, 0x6d9d_6122, 0xfde5_380c, 0xa4be_ea44, 0x4bde_cfa9, 0xf6bb_4b60, 0xbebf_bc70,
    0x289b_7ec6, 0xeaa1_27fa, 0xd4ef_3085, 0x0488_1d05, 0xd9d4_d039, 0xe6db_99e5, 0x1fa2_7cf8, 0xc4ac_5665,
    0xf429_2244, 0x432a_ff97, 0xab94_23a7, 0xfc93_a039, 0x655b_59c3, 0x8f0c_cc92, 0xffef_f47d, 0x8584_5dd1,
    0x6fa8_7e4f, 0xfe2c_e6e0, 0xa301_4314, 0x4e08_11a1, 0xf753_7e82, 0xbd3a_f235, 0x2ad7_d2bb, 0xeb86_d391,
];

/// Apply the standard MD5 compression function for one 64-byte block to `state`.
fn md5_transform(state: &mut [u32; 4], block: &[u8; 64]) {
    let mut m = [0u32; 16];
    for (word, chunk) in m.iter_mut().zip(block.chunks_exact(4)) {
        *word = u32::from_le_bytes(chunk.try_into().expect("4-byte chunk"));
    }
    let [mut a, mut b, mut c, mut d] = *state;
    for i in 0..64 {
        let (f, g) = match i {
            0..=15 => ((b & c) | (!b & d), i),
            16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
            32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
            _ => (c ^ (b | !d), (7 * i) % 16),
        };
        let f = f.wrapping_add(a).wrapping_add(MD5_K[i]).wrapping_add(m[g]);
        a = d;
        d = c;
        c = b;
        b = b.wrapping_add(f.rotate_left(MD5_SHIFTS[i]));
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
}

#[cfg(test)]
mod tests {
    use crate::checksum::dxbc_hash;

    /// Vectors generated from a reference implementation that reproduces the stored checksum of the
    /// game's own shader blobs, covering both final-block branches (`left_over < 56` and `>= 56`).
    #[test]
    fn dxbc_hash_matches_reference_vectors() {
        let cases: &[(&[u8], &str)] = &[
            (&[], "140d60f6b775e2ba4e4abed401b2e9a1"),
            (b"abc", "fbf0ffb01d1f9d12864dff8830d1b4a3"),
            (&bytes(55), "842e55534ba93daa93e94bd5af9b0c03"),
            (&bytes(56), "00f9cc964ff2ec81959d4a3f092ce63f"),
            (&bytes(64), "f42eb06ca921c878e435e3b8d4f92b13"),
            (&skewed(120), "370fd73fe9bf95fa11dcfc5a9acb4826"),
        ];
        for (data, expected) in cases {
            assert_eq!(hex(&dxbc_hash(data)), *expected, "len {}", data.len());
        }
    }

    fn bytes(n: usize) -> Vec<u8> {
        (0..n).map(|i| i as u8).collect()
    }

    fn skewed(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7) as u8).collect()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
