use flate2::read::ZlibDecoder;
use std::io::Read;

/// A parsed tag from the SWF/GFX body.
#[derive(Clone)]
pub struct Tag {
    pub code: u16,
    pub data: Vec<u8>,
}

/// A parsed GFX/SWF file.
pub struct GfxFile {
    pub magic: String,
    pub version: u8,
    pub file_length: u32,
    pub body: Vec<u8>,
    pub tags: Vec<Tag>,
}

impl GfxFile {
    pub fn parse(data: &[u8]) -> anyhow::Result<Self> {
        if data.len() < 8 {
            anyhow::bail!("file too short: {} bytes", data.len());
        }

        let magic = String::from_utf8_lossy(&data[0..3]).to_string();
        let version = data[3];
        let file_length = u32::from_le_bytes(data[4..8].try_into().unwrap());

        // Validate magic: FWS (uncompressed), CWS (zlib), ZWS (LZMA), CFX/GFX (Scaleform)
        let body = match magic.as_bytes() {
            b"FWS" | b"GFX" => data[8..].to_vec(),
            b"CWS" | b"CFX" => {
                let mut decoder = ZlibDecoder::new(&data[8..]);
                let mut out = Vec::with_capacity(file_length as usize);
                decoder.read_to_end(&mut out)?;
                out
            }
            b"ZWS" => {
                // LZMA-compressed. The first 4 bytes after the header are the
                // compressed size, then 5 bytes of LZMA properties, then the data.
                anyhow::bail!("LZMA-compressed SWF (ZWS) not supported yet");
            }
            _ => anyhow::bail!("unknown magic: {magic:?}"),
        };

        // Skip the SWF frame header: RECT (variable length) + frame_rate (u16) + frame_count (u16)
        let frame_header_end = skip_frame_header(&body)?;
        let tags = Self::parse_tags(&body[frame_header_end..])?;

        Ok(Self {
            magic,
            version,
            file_length,
            body,
            tags,
        })
    }

    fn parse_tags(body: &[u8]) -> anyhow::Result<Vec<Tag>> {
        let mut tags = Vec::new();
        let mut pos = 0;

        while pos + 2 <= body.len() {
            let tag_code_and_length = u16::from_le_bytes(body[pos..pos + 2].try_into().unwrap());
            pos += 2;

            let code = tag_code_and_length >> 6;
            let mut length = (tag_code_and_length & 0x3F) as usize;

            if length == 0x3F {
                if pos + 4 > body.len() {
                    break;
                }
                length = u32::from_le_bytes(body[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
            }

            if pos + length > body.len() {
                // truncated tag; take what's left
                length = body.len().saturating_sub(pos);
            }

            let data = body[pos..pos + length].to_vec();
            pos += length;

            tags.push(Tag { code, data });

            // End tag (code 0) marks the end of the tag list
            if code == 0 {
                break;
            }
        }

        Ok(tags)
    }
}

// Helper: read a u16 LE from a slice at the given offset
pub fn read_u16_le(data: &[u8], pos: &mut usize) -> u16 {
    let v = u16::from_le_bytes(data[*pos..*pos + 2].try_into().unwrap());
    *pos += 2;
    v
}

/// Read a u32 LE from a slice at the given offset
pub fn read_u32_le(data: &[u8], pos: &mut usize) -> u32 {
    let v = u32::from_le_bytes(data[*pos..*pos + 4].try_into().unwrap());
    *pos += 4;
    v
}

/// Read an unsigned integer using SWF variable-length encoding (u30/vu).
/// Each byte contributes 7 bits; the high bit signals continuation.
pub fn read_u30(data: &[u8], pos: &mut usize) -> u32 {
    let mut result = 0u32;
    for i in 0..5 {
        if *pos >= data.len() {
            break;
        }
        let byte = data[*pos];
        *pos += 1;
        result |= ((byte & 0x7F) as u32) << (i * 7);
        if byte & 0x80 == 0 {
            break;
        }
    }
    result
}

/// Read a null-terminated UTF-8 string from the slice
pub fn read_string(data: &[u8], pos: &mut usize) -> String {
    let end = data[*pos..]
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(data.len() - *pos);
    let s = String::from_utf8_lossy(&data[*pos..*pos + end]).to_string();
    *pos += end + 1; // skip null terminator
    s
}

/// Skip the SWF frame header: a RECT (variable-length, bit-packed) followed by
/// frame_rate (u16) and frame_count (u16). Returns the byte offset where tags begin.
fn skip_frame_header(body: &[u8]) -> anyhow::Result<usize> {
    if body.is_empty() {
        anyhow::bail!("empty body, cannot read frame header");
    }

    // RECT: first 5 bits = Nbits, then 4 fields of Nbits bits each
    let nbits = (body[0] >> 3) as usize;
    let total_bits = 5 + 4 * nbits;
    let rect_bytes = total_bits.div_ceil(8);

    // After the RECT: frame_rate (u16) + frame_count (u16)
    let header_end = rect_bytes + 4;
    if header_end > body.len() {
        anyhow::bail!("frame header extends past body");
    }

    Ok(header_end)
}
