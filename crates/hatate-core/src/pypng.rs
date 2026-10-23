//! How Pillow reads the pixel stream of a PNG, so a damaged file decodes (or is refused) the way
//! `Image.open(...).load()` does it, whatever the `png` crate thinks of it.
//!
//! Pillow's `PngImageFile.load_read` hands the zlib decoder the payload of the consecutive `IDAT`
//! chunks, at most `ImageFile.MAXBLOCK` bytes at a time, and skips each chunk's 4-byte CRC without
//! looking at it: a bad checksum on a pixel chunk is invisible to Pillow. The decoder
//! (`ZipDecode`) is zlib's `inflate` called once per row (see [`crate::pyinflate`]): a corrupt
//! stream, or an adler32 that does not match inside the block that completes the last row, is
//! "broken data stream", a stream that runs out of input before the last row is "image file is
//! truncated", a stream that ends exactly on a row boundary leaves the rows after it zero, and
//! whatever follows the last row is never read. The chunks after the pixel data are walked by
//! `load_end`, which does not raise. The `png` crate (through `image`) checks every CRC, ignores
//! the adler32 and stops differently, so [`canonical`] re-states the pixel stream as a plain
//! well-formed PNG (the chunks before the first `IDAT` verbatim, one `IDAT` holding exactly the
//! rows Pillow got, `IEND`) that the `image` crate decodes to the same pixels.

use std::borrow::Cow;
use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::pyinflate::{Broken, Inflate, Stop};

/// `ImageFile._safe_read` on a body the file cuts short.
const READ_CUT: &str = "Truncated File Read";

/// `ImageFile.MAXBLOCK`: Pillow feeds the decoder at most this many bytes at a time.
const BLOCK: usize = 65536;

/// (x offset, y offset, x step, y step) of the seven Adam7 passes.
const PASSES: [(u64, u64, u64, u64); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

/// The `IHDR` fields that fix how many bytes of filtered rows a picture has.
struct Header {
    width: u64,
    height: u64,
    bits_per_pixel: u64,
    interlaced: bool,
}

impl Header {
    /// The length of each row `ZipDecode` expects from the inflated stream (the row plus its
    /// filter byte), pass by pass for Adam7 (a pass with no pixels has no rows).
    fn row_lens(&self) -> impl Iterator<Item = usize> + '_ {
        let passes: &[(u64, u64, u64, u64)] = if self.interlaced {
            &PASSES
        } else {
            &[(0, 0, 1, 1)]
        };
        passes.iter().flat_map(move |&(x0, y0, dx, dy)| {
            let w = self.width.saturating_sub(x0).div_ceil(dx);
            let h = self.height.saturating_sub(y0).div_ceil(dy);
            let (len, rows) = if w == 0 || h == 0 {
                (0, 0)
            } else {
                (1 + (w * self.bits_per_pixel).div_ceil(8), h)
            };
            std::iter::repeat_n(len as usize, rows as usize)
        })
    }

    /// Whether the rows add up to something a `usize` holds.
    fn fits(&self) -> bool {
        let one = |w: u64, h: u64| -> Option<u64> {
            if w == 0 || h == 0 {
                return Some(0);
            }
            h.checked_mul(1 + w.checked_mul(self.bits_per_pixel)?.div_ceil(8))
        };
        let total = if self.interlaced {
            PASSES.iter().try_fold(0u64, |sum, &(x0, y0, dx, dy)| {
                let w = self.width.saturating_sub(x0).div_ceil(dx);
                let h = self.height.saturating_sub(y0).div_ceil(dy);
                sum.checked_add(one(w, h)?)
            })
        } else {
            one(self.width, self.height)
        };
        total.is_some_and(|n| usize::try_from(n).is_ok())
    }
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

/// `PngImagePlugin.is_cid`: `\w\w\w\w` on bytes.
fn is_cid(cid: &[u8]) -> bool {
    cid.len() == 4 && cid.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

/// The `IHDR` fields and where the first `IDAT` chunk starts, or `None` for a file this does not
/// restate (no `IDAT`, an `IHDR` it does not understand): the caller decodes those as they are.
fn scan(data: &[u8]) -> Option<(Header, usize)> {
    let mut at = 8usize;
    let mut header = None;
    loop {
        let length = usize::try_from(be32(data, at)?).ok()?;
        let cid = data.get(at + 4..at + 8)?;
        let body = at + 8;
        match cid {
            b"IHDR" if header.is_none() => {
                let ihdr = data.get(body..body + 13)?;
                let channels = match ihdr[9] {
                    0 | 3 => 1,
                    2 => 3,
                    4 => 2,
                    6 => 4,
                    _ => return None,
                };
                if ihdr[12] > 1 {
                    return None;
                }
                header = Some(Header {
                    width: u64::from(be32(ihdr, 0)?),
                    height: u64::from(be32(ihdr, 4)?),
                    bits_per_pixel: u64::from(ihdr[8]) * channels,
                    interlaced: ihdr[12] == 1,
                });
            }
            b"IDAT" => return Some((header?, at)),
            b"IEND" => return None,
            _ => {}
        }
        at = body.checked_add(length)?.checked_add(4)?;
    }
}

/// What Pillow's `load_read` hands the decoder, and what it says when asked for more after that.
struct PixelData {
    /// The payloads of the consecutive pixel chunks, back to back.
    buf: Vec<u8>,
    /// Where each block (a payload, cut into [`BLOCK`] pieces) ends in `buf`.
    ends: Vec<usize>,
    /// Where, in the file, the chunk each block came from ends its payload.
    chunk_ends: Vec<usize>,
    /// The `OSError` or `SyntaxError` text for a decoder that needs more than there is.
    lacking: String,
}

fn bytes_repr(b: &[u8]) -> String {
    let mut s = String::from("b'");
    for &c in b {
        match c {
            b'\\' => s.push_str("\\\\"),
            b'\'' => s.push_str("\\'"),
            b'\t' => s.push_str("\\t"),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            0x20..=0x7e => s.push(c as char),
            _ => s.push_str(&format!("\\x{c:02x}")),
        }
    }
    s.push('\'');
    s
}

fn pixel_data(data: &[u8], first_idat: usize) -> PixelData {
    const TRUNCATED: &str = "image file is truncated";
    const NOT_PROCESSED: &str = "image file is truncated (0 bytes not processed)";
    let mut px = PixelData {
        buf: Vec::new(),
        ends: Vec::new(),
        chunk_ends: Vec::new(),
        lacking: NOT_PROCESSED.to_string(),
    };
    let mut at = first_idat;
    loop {
        let length = be32(data, at).map_or(0, |n| n as usize);
        // `fdAT` carries a 4-byte sequence number in front of the data.
        let skip = if data.get(at + 4..at + 8) == Some(b"fdAT") {
            4
        } else {
            0
        };
        let body = at + 8 + skip;
        let length = length.saturating_sub(skip);
        let end = body.saturating_add(length);
        // `load_read` asks for `min(MAXBLOCK, bytes left in the chunk)` and counts what it asked
        // for, not what it got.
        let mut done = 0;
        while done < length {
            let asked = BLOCK.min(length - done);
            let from = (body + done).min(data.len());
            let got = &data[from..(body + done + asked).min(data.len())];
            if !got.is_empty() {
                px.buf.extend_from_slice(got);
                px.ends.push(px.buf.len());
                px.chunk_ends.push(end);
            }
            done += asked;
            if got.len() < asked {
                // The file ended inside the payload. An empty read is the end of the data; a
                // short one is taken, and the next read finds the chunk spent (and no header
                // after it) or still owed bytes it cannot get.
                if !got.is_empty() && done == length {
                    px.lacking = TRUNCATED.to_string();
                }
                return px;
            }
        }
        // The CRC is skipped unread; then the next chunk header decides.
        let next = end + 4;
        let rest = data.len().saturating_sub(next);
        if rest < 4 {
            px.lacking = TRUNCATED.to_string();
            return px;
        }
        let cid = &data[next + 4..(next + 8).min(data.len())];
        if !is_cid(cid) {
            px.lacking = format!("broken PNG file (chunk {})", bytes_repr(cid));
            return px;
        }
        if !matches!(cid, b"IDAT" | b"fdAT" | b"DDAT") {
            return px;
        }
        at = next;
    }
}

/// Why the pixel stream did not load.
enum Fail {
    /// Raised while loading: nothing else is looked at.
    Now(String),
    /// The decoder failed and stopped; Pillow still walks the chunks after the data (`load_end`)
    /// and only then raises, so what that walk raises wins. Holds the file offset it starts from.
    Later(String, usize),
}

/// The filtered rows Pillow ends up with, `Err` with its message where it raises.
fn inflate_rows(px: &PixelData, header: &Header) -> Result<(Vec<u8>, usize), Fail> {
    const BROKEN: &str = "broken data stream when reading image file";
    const UNKNOWN: &str = "unrecognized data stream contents when reading image file";
    let mut inf = Inflate::new(&px.buf);
    let mut blocks = px.ends.iter();
    let mut fetched = 0usize;
    let mut rows = header.row_lens();
    let Some(first) = rows.next() else {
        return Ok((Vec::new(), 0));
    };
    let (mut row_start, mut row_end) = (0, first);
    // `avail_in == 0`: the next block (or the lack of one) is wanted.
    let mut starved = true;
    loop {
        if starved {
            match blocks.next() {
                Some(&end) => {
                    inf.set_limit(end);
                    fetched += 1;
                }
                None => return Err(Fail::Now(px.lacking.clone())),
            }
            starved = !inf.has_input();
            continue;
        }
        let stop = inf
            .run(row_end)
            .map_err(|Broken| Fail::Later(BROKEN.to_string(), px.chunk_ends[fetched - 1]))?;
        if inf.out.len() == row_end {
            // `ZipDecode` unfilters each row as it completes; a filter type past 4 stops it.
            let chunk_end = px.chunk_ends[fetched - 1];
            if inf.out[row_start] > 4 {
                return Err(Fail::Later(UNKNOWN.to_string(), chunk_end));
            }
            let Some(next) = rows.next() else {
                return Ok((std::mem::take(&mut inf.out), chunk_end));
            };
            if stop == Stop::End {
                // A stream that ends on a row boundary: the rows after it stay zero.
                let mut out = std::mem::take(&mut inf.out);
                out.resize(out.len() + next, 0);
                for len in rows {
                    out.resize(out.len() + len, 0);
                }
                return Ok((out, chunk_end));
            }
            row_start = row_end;
            row_end += next;
            starved = stop == Stop::Need || !inf.has_input();
        } else {
            match stop {
                Stop::Need => starved = true,
                Stop::End | Stop::NeedDict | Stop::Full => {
                    return Err(Fail::Now(px.lacking.clone()));
                }
            }
        }
    }
}

/// `PngImageFile.load_end` after the pixel data: every chunk that follows is read through to the
/// end of its body, and a body the file cuts short is `OSError("Truncated File Read")`. Checksums
/// are not looked at, and a header that does not parse ends the walk quietly.
fn trailing_chunks_complete(data: &[u8], payload_end: usize) -> Result<(), String> {
    let mut at = payload_end;
    loop {
        at = at.saturating_add(4);
        let Some(header) = data.get(at..at.saturating_add(8)) else {
            return Ok(());
        };
        if !is_cid(&header[4..]) || &header[4..] == b"IEND" {
            return Ok(());
        }
        let length = be32(header, 0).map_or(0, |n| n as usize);
        at += 8;
        let end = at.saturating_add(length);
        if end > data.len() {
            return Err(READ_CUT.to_string());
        }
        at = end;
    }
}

/// `PngImageFile.verify()`, which `Image.verify()` runs: every chunk from the first `IDAT` to
/// `IEND` read to the end of its body and its checksum compared, the pixel chunks included. `Err`
/// is the exception's text. Chunks before the first `IDAT` were checked when the file was opened.
pub(crate) fn verify(data: &[u8]) -> Result<(), String> {
    let Some((_, mut at)) = scan(data) else {
        return Ok(());
    };
    loop {
        let header = data.get(at..(at + 8).min(data.len())).unwrap_or(&[]);
        if header.len() < 4 {
            return Err("truncated PNG file".to_string());
        }
        let cid = &header[4..];
        if !is_cid(cid) {
            return Err(format!("broken PNG file (chunk {})", bytes_repr(cid)));
        }
        if cid == b"IEND" {
            return Ok(());
        }
        let body = at + 8;
        let end = body.saturating_add(be32(header, 0).map_or(0, |n| n as usize));
        let Some(content) = data.get(body..end) else {
            return Err(READ_CUT.to_string());
        };
        let Some(stored) = data.get(end..end + 4) else {
            return Err(format!(
                "broken PNG file (incomplete checksum in {})",
                bytes_repr(cid)
            ));
        };
        let mut crc = crc32fast::Hasher::new();
        crc.update(cid);
        crc.update(content);
        if crc.finalize().to_be_bytes() != stored {
            return Err(format!(
                "broken PNG file (bad header checksum in {})",
                bytes_repr(cid)
            ));
        }
        at = end + 4;
    }
}

/// A PNG whose pixel stream is `rows` in a single `IDAT`, behind the chunks `data` has before its
/// first `IDAT`.
fn restate(data: &[u8], first_idat: usize, rows: &[u8]) -> Vec<u8> {
    let mut z = ZlibEncoder::new(Vec::new(), Compression::none());
    let compressed = z.write_all(rows).and_then(|()| z.finish());
    let compressed = compressed.unwrap_or_default();
    let mut out = Vec::with_capacity(first_idat + compressed.len() + 24);
    out.extend_from_slice(&data[..first_idat]);
    for (cid, body) in [(&b"IDAT"[..], &compressed[..]), (&b"IEND"[..], &[][..])] {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(cid);
        out.extend_from_slice(body);
        let mut crc = crc32fast::Hasher::new();
        crc.update(cid);
        crc.update(body);
        out.extend_from_slice(&crc.finalize().to_be_bytes());
    }
    out
}

/// `data` as the PNG the `image` crate must decode to get Pillow's pixels, `Err` with Pillow's
/// message where Pillow raises while loading, and `data` itself where it is not a PNG this
/// restates.
pub(crate) fn canonical(data: &[u8]) -> Result<Cow<'_, [u8]>, String> {
    if !data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(Cow::Borrowed(data));
    }
    let Some((header, first_idat)) = scan(data) else {
        return Ok(Cow::Borrowed(data));
    };
    if !header.fits() {
        return Ok(Cow::Borrowed(data));
    }
    let (rows, payload_end) = match inflate_rows(&pixel_data(data, first_idat), &header) {
        Ok(done) => done,
        Err(Fail::Now(msg)) => return Err(msg),
        Err(Fail::Later(msg, payload_end)) => {
            trailing_chunks_complete(data, payload_end)?;
            return Err(msg);
        }
    };
    trailing_chunks_complete(data, payload_end)?;
    Ok(Cow::Owned(restate(data, first_idat, &rows)))
}
