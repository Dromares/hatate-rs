//! The pixel size `Image.open(io.BytesIO(data)).size` reports, for the one caller that asks only
//! whether a picture is a speck (`google_lens._is_placeholder_thumb`).
//!
//! Pillow (12.3, the version that recorded the corpus) reads a header and stops; so does this.
//! **GIF** follows `GifImagePlugin._open` and the first `_seek(0)`: the logical screen, grown to
//! the first frame's extent when that sticks out. **PNG** (signature, then chunks up to the first
//! `IDAT`/`IEND`, `IHDR` giving the size), **JPEG** (`SOF` after the `APPn`/`DQT`/... segments,
//! 8-bit precision), **WebP** (`VP8 `, `VP8L`, `VP8X`) and **BMP** (the DIB header) read the
//! dimension fields and the checks that stand between the signature and them; Pillow's later checks
//! (PNG chunk CRCs, BMP bit depths and compression, WebP chunk bounds, a JPEG's component
//! count) are not repeated, so a damaged file of one of those four whose header is intact reads
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
    let (w, h) = header_size(data)?;
    // `ImageFile.__init__`: "not identified by this driver" for a side of 0; and
    // `Image._decompression_bomb_check`.
    (w > 0 && h > 0 && u64::from(w) * u64::from(h) <= 2 * 89_478_485).then_some((w, h))
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

/// A GIF sub-block sequence: each block is a length byte and that many bytes, ended by a zero
/// length. Returns the position after the terminator; a sequence cut short ends at the data's end
/// (`fp.read` returns less, never fails).
fn skip_sub_blocks(d: &[u8], mut at: usize) -> usize {
    loop {
        match d.get(at) {
            None => return d.len(),
            Some(0) => return at + 1,
            Some(&n) => at = (at + 1 + usize::from(n)).min(d.len()),
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
                d.get(at + 1)?; // the label byte
                at = skip_sub_blocks(d, at + 2);
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

fn png(d: &[u8]) -> Option<(u32, u32)> {
    let (mut at, mut size) = (8usize, (0, 0));
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
                // `s[11]`, the filter method, must be 0.
                if *d.get(body + 11)? != 0 {
                    return None;
                }
            }
            b"IDAT" | b"IEND" => return Some(size),
            _ => {}
        }
        // The chunk, then its 4-byte CRC.
        at = body.checked_add(length)?.checked_add(4)?;
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
                if s[0] != 8 || !matches!(*s.get(5)?, 1 | 3 | 4) {
                    return None;
                }
                at += 2 + n;
            }
            // Start of scan: the header is over.
            0xFFDA => {
                u16be(d, at)?; // the Skip handler reads the segment's length
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
    let chunk = d.get(12..16)?;
    match chunk {
        b"VP8 " => {
            // frame tag (3 bytes), start code 9d 01 2a, then 14-bit width and height.
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

fn bmp(d: &[u8]) -> Option<(u32, u32)> {
    let header_size = u32le(d, 14)?;
    match header_size {
        12 => Some((u16le(d, 18)?, u16le(d, 20)?)),
        40 | 52 | 56 | 64 | 108 | 124 => {
            let width = u32le(d, 18)?;
            let raw = u32le(d, 22)?;
            // A top-down bitmap stores a negative height (`header_data[7] == 0xFF`).
            let height = if *d.get(25)? == 0xff {
                0u32.wrapping_sub(raw)
            } else {
                raw
            };
            Some((width, height))
        }
        _ => None,
    }
}
