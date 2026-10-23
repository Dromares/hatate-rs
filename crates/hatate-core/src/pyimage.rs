//! The pixel size `Image.open(io.BytesIO(data)).size` reports, for the one caller that asks only
//! whether a picture is a speck (`google_lens._is_placeholder_thumb`).
//!
//! Pillow (12.3, the version that recorded the corpus) reads a header and stops; so does this.
//! **GIF** follows `GifImagePlugin._open` and the first `_seek(0)`: the logical screen, grown to
//! the first frame's extent when that sticks out. **PNG** (signature, then chunks up to the first
//! `IDAT`/`IEND`, `IHDR` giving the size), **JPEG** (`SOF` after the `APPn`/`DQT`/... segments,
//! 8-bit precision), **WebP** (`VP8 `, `VP8L`, `VP8X`) and **BMP** (the DIB header) read the
//! dimension fields and the checks that stand between the signature and them; Pillow's later checks
//! (BMP bit depths and compression, a WebP's later chunks and bitstream) are not repeated, so a damaged file of one of those four whose header is intact reads
//! here as the size it states. Every other format Pillow can open (TIFF, ICO, PPM, ...) reads as
//! "not an image", which is what a tile of Google's page never carries.
//!
//! Everything `Image.open` cannot read (a bad signature, a header cut short, a GIF with no frame,
//! a side of 0) is `UnidentifiedImageError`, which the caller catches: `None` here. The one thing
//! Pillow raises that the caller does not catch is `DecompressionBombError`, past 178 million
//! pixels; that reads here as `None` too, where Python's search would fail with the exception.

fn u16le(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_le_bytes([
        *d.get(at)?,
        *d.get(at + 1)?,
    ])))
}

fn u16be(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes([
        *d.get(at)?,
        *d.get(at + 1)?,
    ])))
}

fn u32le(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn u32be(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

/// `(width, height)` of the picture in `data`, or `None` when Pillow would not open it.
pub fn image_size(data: &[u8]) -> Option<(u32, u32)> {
    image_size_checked(data).ok().flatten()
}

/// What `Image.open` raises past 178 million pixels (`DecompressionBombError`): the one failure a
/// caller of `Image.open` does not see as "not an image".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecompressionBomb {
    pub pixels: u64,
}

impl DecompressionBomb {
    /// Pillow's `MAX_IMAGE_PIXELS` is 89 478 485; twice that raises.
    pub const LIMIT: u64 = 2 * 89_478_485;
}

impl std::fmt::Display for DecompressionBomb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Image size ({} pixels) exceeds limit of {} pixels, could be decompression bomb DOS attack.",
            self.pixels,
            Self::LIMIT
        )
    }
}

/// [`image_size`], with the decompression-bomb check as an error instead of "not an image".
pub fn image_size_checked(data: &[u8]) -> Result<Option<(u32, u32)>, DecompressionBomb> {
    let Some((w, h)) = header_size(data) else {
        return Ok(None);
    };
    // `ImageFile.__init__`: "not identified by this driver" for a side of 0; and
    // `Image._decompression_bomb_check`.
    if w == 0 || h == 0 {
        return Ok(None);
    }
    let pixels = u64::from(w) * u64::from(h);
    if pixels > DecompressionBomb::LIMIT {
        return Err(DecompressionBomb { pixels });
    }
    Ok(Some((w, h)))
}

fn header_size(data: &[u8]) -> Option<(u32, u32)> {
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        gif(data)
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(data)
    } else if data.starts_with(b"\xff\xd8\xff") {
        jpeg(data)
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        webp(data)
    } else if data.starts_with(b"BM") {
        bmp(data)
    } else {
        None
    }
}

/// `GifImageFile.data()`: one sub-block, a length byte and that many bytes. A zero length (or the
/// end of the data) is `None` and consumes only the length byte; a block cut short is what is left.
fn data_block(d: &[u8], at: usize) -> (usize, Option<&[u8]>) {
    match d.get(at) {
        None => (at, None),
        Some(0) => (at + 1, None),
        Some(&n) => {
            let end = (at + 1 + usize::from(n)).min(d.len());
            (end, Some(&d[at + 1..end]))
        }
    }
}

fn gif(d: &[u8]) -> Option<(u32, u32)> {
    // `s[10]` is read unchecked: a header shorter than 11 bytes is not a GIF to Pillow.
    let flags = *d.get(10)?;
    let (mut w, mut h) = (u16le(d, 6)?, u16le(d, 8)?);
    let mut at = 13usize.min(d.len());
    if flags & 0x80 != 0 {
        at = (at + (3usize << ((flags & 7) + 1))).min(d.len());
    }
    // `_seek(0)`: blocks until the first image descriptor.
    loop {
        match *d.get(at)? {
            b';' => return None, // "no more images in GIF file" (EOFError)
            b'!' => {
                let label = *d.get(at + 1)?;
                let (mut next, first) = data_block(d, at + 2);
                if label == 0xfe {
                    // A comment: blocks up to the first empty one (which may be the first).
                    let mut block = first;
                    while block.is_some() {
                        (next, block) = data_block(d, next);
                    }
                } else {
                    // The graphic control extension reads `block[0]`, `block[3]` when the
                    // transparency flag is set, and `i16(block, 1)`: a shorter block is an
                    // `IndexError` or `struct.error`.
                    if let Some(block) = first.filter(|_| label == 0xf9)
                        && (block.len() < 3 || (block[0] & 1 != 0 && block.len() < 4))
                    {
                        return None;
                    }
                    // Application extension: NETSCAPE2.0 reads one more block.
                    if label == 0xff && first.is_some_and(|b| b.starts_with(b"NETSCAPE2.0")) {
                        (next, _) = data_block(d, next);
                    }
                    // Then `while self.data(): pass`, which also swallows the blocks that follow
                    // an empty first one.
                    loop {
                        let (after, block) = data_block(d, next);
                        next = after;
                        if block.is_none() {
                            break;
                        }
                    }
                }
                at = next;
            }
            b',' => {
                let (x0, y0) = (u16le(d, at + 1)?, u16le(d, at + 3)?);
                let (x1, y1) = (x0 + u16le(d, at + 5)?, y0 + u16le(d, at + 7)?);
                d.get(at + 9)?; // the descriptor's flags byte, `s[8]`
                let local_palette = d[at + 9] & 0x80 != 0;
                let mut after = at + 10;
                if local_palette {
                    after = (after + (3usize << ((d[at + 9] & 7) + 1))).min(d.len());
                }
                d.get(after)?; // `self.fp.read(1)[0]`: the LZW code size
                if x1 > w || y1 > h {
                    (w, h) = (w.max(x1), h.max(y1));
                }
                return Some((w, h));
            }
            _ => at += 1,
        }
    }
}

/// `zlib.crc32` (the PNG checksum), bitwise: chunks before the first `IDAT` are small.
fn crc32(parts: &[&[u8]]) -> u32 {
    let mut crc = !0u32;
    for byte in parts.iter().flat_map(|p| p.iter()) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn png(d: &[u8]) -> Option<(u32, u32)> {
    let (mut at, mut size, mut mode_ok) = (8usize, (0, 0), false);
    loop {
        let length = usize::try_from(u32be(d, at)?).ok()?;
        let cid = d.get(at + 4..at + 8)?;
        // `is_cid`: four ASCII letters.
        if !cid.iter().all(u8::is_ascii_alphabetic) {
            return None;
        }
        let body = at + 8;
        match cid {
            b"IHDR" => {
                if length < 13 {
                    return None;
                }
                size = (u32be(d, body)?, u32be(d, body + 4)?);
                // `_MODES[(bit depth, colour type)]`: a pair Pillow has no mode for is a
                // `KeyError`, and no mode is "not identified by this driver".
                mode_ok = matches!(
                    (*d.get(body + 8)?, *d.get(body + 9)?),
                    (1 | 2 | 4 | 8 | 16, 0) | (8 | 16, 2) | (1 | 2 | 4 | 8, 3) | (8 | 16, 4 | 6)
                );
                // `s[11]`, the filter method, must be 0.
                if *d.get(body + 11)? != 0 {
                    return None;
                }
            }
            b"IDAT" | b"IEND" => return mode_ok.then_some(size),
            _ => {}
        }
        // The chunk, then its 4-byte CRC over the type and the data.
        let end = body.checked_add(length)?;
        let crc = u32be(d, end)?;
        if crc != crc32(&[cid, d.get(body..end)?]) {
            return None;
        }
        at = end + 4;
    }
}

fn jpeg(d: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2usize; // the first 0xFF of the signature is the marker's first byte below
    let mut size = None;
    loop {
        // `s` holds the byte before the marker: 0xFF starts one; anything else is junk to skip.
        if *d.get(at)? != 0xFF {
            at += 1;
            continue;
        }
        let marker = 0xFF00 | u32::from(*d.get(at + 1)?);
        at += 2;
        match marker {
            // SOF0-3, 5-7, 9-11, 13-15 and DHP: the frame header with the size.
            0xFFC0..=0xFFC3 | 0xFFC5..=0xFFC7 | 0xFFC9..=0xFFCB | 0xFFCD..=0xFFCF | 0xFFDE => {
                let n = usize::try_from(u16be(d, at)?).ok()?.saturating_sub(2);
                let s = d.get(at + 2..at + 2 + n)?;
                size = Some((u16be(s, 3)?, u16be(s, 1)?));
                // "cannot handle N-bit layers" / "cannot handle N-layer images"
                // ... and the layer table is read three bytes at a time: a short last entry is an
                // IndexError.
                if s[0] != 8 || !matches!(*s.get(5)?, 1 | 3 | 4) || (s.len() - 6) % 3 != 0 {
                    return None;
                }
                at += 2 + n;
            }
            // Start of scan: the header is over.
            0xFFDA => {
                // The Skip handler reads the whole segment.
                let n = usize::try_from(u16be(d, at)?).ok()?.saturating_sub(2);
                d.get(at + 2..at + 2 + n)?;
                return size;
            }
            // DHT, DAC, DQT, DNL, DRI, EXP and APPn carry a length.
            0xFFC4 | 0xFFCC | 0xFFDB..=0xFFDD | 0xFFDF | 0xFFE0..=0xFFEF => {
                let n = usize::try_from(u16be(d, at)?).ok()?.saturating_sub(2);
                d.get(at + 2..at + 2 + n)?;
                at += 2 + n;
            }
            // COM carries a length too.
            0xFFFE => {
                let n = usize::try_from(u16be(d, at)?).ok()?.saturating_sub(2);
                d.get(at + 2..at + 2 + n)?;
                at += 2 + n;
            }
            // No handler: JPG, RSTn, SOI, EOI and JPGn have no payload as far as Pillow reads.
            0xFFC8 | 0xFFD0..=0xFFD9 | 0xFFF0..=0xFFFD => {}
            0xFF00 => {}
            0xFFFF => at -= 1, // padding: the second 0xFF may start the next marker
            _ => return None,  // "no marker found"
        }
    }
}

fn webp(d: &[u8]) -> Option<(u32, u32)> {
    // libwebp's demuxer (Pillow's only way in) wants the whole RIFF container and the first chunk
    // inside it.
    let riff_end = usize::try_from(u32le(d, 4)?).ok()?.checked_add(8)?;
    if riff_end > d.len() || usize::try_from(u32le(d, 16)?).ok()?.checked_add(20)? > riff_end {
        return None;
    }
    let chunk = d.get(12..16)?;
    match chunk {
        b"VP8 " => {
            // `VP8GetInfo`: the frame tag (3 bytes) says key frame, profile 0 to 3, shown, with a
            // first partition shorter than the chunk; then the start code 9d 01 2a, then the
            // 14-bit width and height.
            let tag =
                u32::from(*d.get(20)?) | u32::from(*d.get(21)?) << 8 | u32::from(*d.get(22)?) << 16;
            let chunk_size = u32le(d, 16)?;
            (tag & 1 == 0 && (tag >> 1) & 7 <= 3 && (tag >> 4) & 1 == 1 && (tag >> 5) < chunk_size)
                .then_some(())?;
            (d.get(23..26)? == [0x9d, 0x01, 0x2a]).then_some(())?;
            Some((u16le(d, 26)? & 0x3fff, u16le(d, 28)? & 0x3fff))
        }
        b"VP8L" => {
            (*d.get(20)? == 0x2f).then_some(())?;
            let bits = u32le(d, 21)?;
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8X" => {
            let w =
                u32::from(*d.get(24)?) | u32::from(*d.get(25)?) << 8 | u32::from(*d.get(26)?) << 16;
            let h =
                u32::from(*d.get(27)?) | u32::from(*d.get(28)?) << 8 | u32::from(*d.get(29)?) << 16;
            Some((w + 1, h + 1))
        }
        _ => None,
    }
}

/// The `(r, g, b, a)` masks of a 32-bit BITFIELDS bitmap Pillow has a raw mode for, and the
/// `(r, g, b)` masks for 24 and 16 bits (`SUPPORTED` in `BmpImagePlugin._bitmap`).
const BMP_MASKS_32: [[u32; 4]; 8] = [
    [0xff0000, 0xff00, 0xff, 0x0],
    [0xff00_0000, 0xff0000, 0xff00, 0x0],
    [0xff00_0000, 0xff00, 0xff, 0x0],
    [0xff00_0000, 0xff0000, 0xff00, 0xff],
    [0xff, 0xff00, 0xff0000, 0xff00_0000],
    [0xff0000, 0xff00, 0xff, 0xff00_0000],
    [0xff00_0000, 0xff00, 0xff, 0xff0000],
    [0x0, 0x0, 0x0, 0x0],
];

fn bmp(d: &[u8]) -> Option<(u32, u32)> {
    let header_size = usize::try_from(u32le(d, 14)?).ok()?;
    // The whole DIB header is read (`_safe_read` raises on a short file).
    let header = d.get(18..14usize.checked_add(header_size)?)?;
    let (width, height, bits, compression, colors) = match header_size {
        12 => (
            u16le(header, 0)?,
            u16le(header, 2)?,
            u16le(header, 6)?,
            0,
            0,
        ),
        40 | 52 | 56 | 64 | 108 | 124 => {
            let raw = u32le(header, 4)?;
            // A top-down bitmap stores a negative height (`header_data[7] == 0xFF`).
            let height = if *header.get(7)? == 0xff {
                0u32.wrapping_sub(raw)
            } else {
                raw
            };
            (
                u32le(header, 0)?,
                height,
                u16le(header, 10)?,
                u32le(header, 12)?,
                u32le(header, 28)?,
            )
        }
        _ => return None, // "Unsupported BMP header type"
    };
    // `BIT2MODE`: "Unsupported BMP pixel depth".
    if !matches!(bits, 1 | 4 | 8 | 16 | 24 | 32) {
        return None;
    }
    match compression {
        0..=2 => {}
        3 => {
            // BITFIELDS: three or four masks, in the header when it is long enough, else the
            // twelve bytes after it.
            let mask = |at: usize| u32le(header, at);
            let masks = if header.len() >= 48 {
                [
                    mask(36)?,
                    mask(40)?,
                    mask(44)?,
                    if header.len() >= 52 { mask(48)? } else { 0 },
                ]
            } else {
                let after = 14 + header_size;
                [
                    u32le(d, after)?,
                    u32le(d, after + 4)?,
                    u32le(d, after + 8)?,
                    0,
                ]
            };
            let supported = match bits {
                32 => BMP_MASKS_32.contains(&masks),
                24 => masks[..3] == [0xff0000, 0xff00, 0xff],
                16 => masks[..3] == [0xf800, 0x7e0, 0x1f] || masks[..3] == [0x7c00, 0x3e0, 0x1f],
                _ => false,
            };
            if !supported {
                return None; // "Unsupported BMP bitfields layout"
            }
        }
        _ => return None, // "Unsupported BMP compression"
    }
    // A paletted bitmap: "Unsupported BMP Palette size".
    let colors = if colors == 0 {
        1u64 << bits
    } else {
        u64::from(colors)
    };
    if bits <= 8 && !(1..=65536).contains(&colors) {
        return None;
    }
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_past_the_pixel_limit_is_a_bomb_not_a_non_image() {
        // A GIF whose logical screen is 65535 x 65535 (4.29e9 pixels).
        let mut gif = gif();
        gif[6..10].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        let bomb = image_size_checked(&gif).unwrap_err();
        assert_eq!(bomb.pixels, 65535 * 65535);
        assert_eq!(
            bomb.to_string(),
            "Image size (4294836225 pixels) exceeds limit of 178956970 pixels, could be decompression bomb DOS attack."
        );
        assert_eq!(
            image_size(&gif),
            None,
            "the older caller still reads it as not an image"
        );
        assert_eq!(image_size_checked(b"<html>"), Ok(None));
    }

    fn unhex(hex: &[&str]) -> Vec<u8> {
        let hex: String = hex.concat();
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
            .collect()
    }

    // 3x2 red pictures, written by Pillow 12.3 (`Image.new("RGB", (3, 2), "red").save(...)`).
    fn gif() -> Vec<u8> {
        unhex(&[
            "47494638376103000200810000ff00000000000000000000002c00000000030002000008060001081c1810003b",
        ])
    }
    fn png() -> Vec<u8> {
        unhex(&[
            "89504e470d0a1a0a0000000d49484452000000030000000208020000001216f14d0000001049444154789c63fccf0005",
            "4c3006030013290103f0d807840000000049454e44ae426082",
        ])
    }
    fn jpeg() -> Vec<u8> {
        unhex(&[
            "ffd8ffe000104a46494600010100000100010000ffdb004300080606070605080707070909080a0c140d0c0b0b0c1912",
            "130f141d1a1f1e1d1a1c1c20242e2720222c231c1c2837292c30313434341f27393d38323c2e333432ffdb0043010909",
            "090c0b0c180d0d1832211c21323232323232323232323232323232323232323232323232323232323232323232323232",
            "3232323232323232323232323232ffc00011080002000303012200021101031101ffc4001f0000010501010101010100",
            "000000000000000102030405060708090a0bffc400b5100002010303020403050504040000017d010203000411051221",
            "31410613516107227114328191a1082342b1c11552d1f02433627282090a161718191a25262728292a3435363738393a",
            "434445464748494a535455565758595a636465666768696a737475767778797a838485868788898a9293949596979899",
            "9aa2a3a4a5a6a7a8a9aab2b3b4b5b6b7b8b9bac2c3c4c5c6c7c8c9cad2d3d4d5d6d7d8d9dae1e2e3e4e5e6e7e8e9eaf1",
            "f2f3f4f5f6f7f8f9faffc4001f0100030101010101010101010000000000000102030405060708090a0bffc400b51100",
            "020102040403040705040400010277000102031104052131061241510761711322328108144291a1b1c109233352f015",
            "6272d10a162434e125f11718191a262728292a35363738393a434445464748494a535455565758595a63646566676869",
            "6a737475767778797a82838485868788898a92939495969798999aa2a3a4a5a6a7a8a9aab2b3b4b5b6b7b8b9bac2c3c4",
            "c5c6c7c8c9cad2d3d4d5d6d7d8d9dae2e3e4e5e6e7e8e9eaf2f3f4f5f6f7f8f9faffda000c03010002110311003f00e2",
            "e8a28af993f713ffd9",
        ])
    }
    fn webp() -> Vec<u8> {
        unhex(&["524946461c000000574542505650384c0f0000002f024000000710fd8ffe0722a2ff0100"])
    }
    fn webp_lossy() -> Vec<u8> {
        unhex(&[
            "524946463c000000574542505650382030000000d001009d012a0300020001402625a00274ba01f80003b000fef2eb7f",
            "fcd815cd73eff7ffd2e0fd2e0fd2e0ffd2900000",
        ])
    }
    fn bmp() -> Vec<u8> {
        unhex(&[
            "424d4e0000000000000036000000280000000300000002000000010018000000000018000000c40e0000c40e00000000",
            "0000000000000000ff0000ff0000ff0000000000ff0000ff0000ff000000",
        ])
    }

    #[test]
    fn each_format_reports_its_header_size() {
        for (name, data) in [
            ("gif", gif()),
            ("png", png()),
            ("jpeg", jpeg()),
            ("webp lossless", webp()),
            ("webp lossy", webp_lossy()),
            ("bmp", bmp()),
        ] {
            assert_eq!(image_size(&data), Some((3, 2)), "{name}");
        }
    }

    #[test]
    fn what_pillow_cannot_open_has_no_size() {
        assert_eq!(image_size(b""), None);
        assert_eq!(image_size(b"AAAA"), None);
        assert_eq!(image_size(b"GIF89a"), None);
        // Cut short at every length: nothing reads as a picture of another size.
        for data in [gif(), png(), jpeg(), webp(), webp_lossy(), bmp()] {
            for n in 0..data.len() {
                assert!(
                    matches!(image_size(&data[..n]), None | Some((3, 2))),
                    "{} bytes",
                    n
                );
            }
        }
        // The pieces a truncation takes that Pillow notices.
        assert_eq!(image_size(&gif()[..13]), None, "a GIF with no frame");
        assert_eq!(
            image_size(&png()[..30]),
            None,
            "a PNG with no chunk after its header"
        );
        assert_eq!(
            image_size(&jpeg()[..400]),
            None,
            "a JPEG cut inside a table"
        );
        assert_eq!(image_size(&webp()[..30]), None);
        assert_eq!(image_size(&bmp()[..40]), None);
    }

    #[test]
    fn a_zero_side_is_not_a_picture() {
        let mut g = gif();
        g[6..10].copy_from_slice(&[0, 0, 0, 0]);
        // The frame descriptor then stretches the screen back to the frame's own extent.
        assert_eq!(image_size(&g), Some((3, 2)));
        let mut p = png();
        p[16..20].copy_from_slice(&[0, 0, 0, 0]);
        // A changed byte breaks the IHDR checksum first.
        assert_eq!(image_size(&p), None);
    }

    #[test]
    fn a_gif_frame_that_sticks_out_grows_the_screen() {
        let mut g = gif();
        // Screen 1x1: the frame descriptor (0x2c ...) says 3x2 at (0, 0).
        g[6..10].copy_from_slice(&[1, 0, 1, 0]);
        assert_eq!(image_size(&g), Some((3, 2)));
    }

    #[test]
    fn a_corrupt_png_checksum_is_not_a_picture() {
        let mut p = png();
        let last_header_byte = 8 + 8 + 12; // the CRC of IHDR ends at 8 + 8 + 13 + 4
        p[last_header_byte + 4] ^= 1;
        assert_eq!(image_size(&p), None);
    }

    #[test]
    fn an_enormous_picture_is_refused_as_pillow_refuses_it() {
        let mut g = gif();
        g[6..10].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        assert_eq!(image_size(&g), None);
    }

    #[test]
    fn the_checksum_is_zlibs() {
        assert_eq!(crc32(&[b"123456789"]), 0xcbf4_3926);
        assert_eq!(crc32(&[b"1234", b"56789"]), 0xcbf4_3926);
    }
}
