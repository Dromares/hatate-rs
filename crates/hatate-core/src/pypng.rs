//! How Pillow reads the pixel stream of a PNG, so a damaged file decodes (or is refused) the way
//! `Image.open(...).load()` does it, whatever the `png` crate thinks of it.
//!
//! Pillow's `PngImageFile.load_read` hands the zlib decoder the payload of the consecutive `IDAT`
//! chunks and skips each chunk's 4-byte CRC without looking at it, so a bad checksum on a pixel
//! chunk is invisible to Pillow. The decoder (`ZipDecode`) is zlib's `inflate`: a corrupt stream
//! is `OSError("broken data stream when reading image file")`, an adler32 that does not match is
//! the same error, a stream that ends before the last row is "image file is truncated", and
//! whatever follows the last row is ignored. The chunks after the pixel data are walked by
//! `load_end`, which never raises. The `png` crate (through `image`) checks every CRC, ignores
//! the adler32 and stops differently, so [`canonical`] re-states the pixel stream as a plain
//! well-formed PNG (the chunks before the first `IDAT` verbatim, one `IDAT` holding exactly the
//! rows Pillow reads, `IEND`) that the `image` crate decodes to the same pixels.

use std::borrow::Cow;
use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::{Compression, Decompress, FlushDecompress};

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
    fn row_lens(&self) -> impl Iterator<Item = usize> {
        let passes: &[(u64, u64, u64, u64)] = if self.interlaced { &PASSES } else { &[(0, 0, 1, 1)] };
        passes.iter().flat_map(move |&(x0, y0, dx, dy)| {
            let w = self.width.saturating_sub(x0).div_ceil(dx);
            let h = self.height.saturating_sub(y0).div_ceil(dy);
            let len = if w == 0 { 0 } else { (1 + (w * self.bits_per_pixel).div_ceil(8)) as usize };
            std::iter::repeat_n(len, if w == 0 { 0 } else { h as usize })
        })
    }

    /// The total of [`Header::row_lens`], `None` past what a `usize` holds.
    fn raw_len(&self) -> Option<usize> {
        let rows = |w: u64, h: u64| -> Option<u64> {
            if w == 0 || h == 0 {
                return Some(0);
            }
            h.checked_mul(1 + w.checked_mul(self.bits_per_pixel)?.div_ceil(8))
        };
        if !self.interlaced {
            return usize::try_from(rows(self.width, self.height)?).ok();
        }
        let total = PASSES.iter().try_fold(0u64, |sum, &(x0, y0, dx, dy)| {
            let w = self.width.saturating_sub(x0).div_ceil(dx);
            let h = self.height.saturating_sub(y0).div_ceil(dy);
            sum.checked_add(rows(w, h)?)
        })?;
        usize::try_from(total).ok()
    }
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

/// `PngStream.read`'s `is_cid`: four ASCII letters.
fn is_cid(cid: &[u8]) -> bool {
    cid.len() == 4 && cid.iter().all(u8::is_ascii_alphabetic)
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

/// The `IDAT` payloads in the blocks Pillow feeds to the decoder, and whether the file ran out
/// (a chunk cut short, or no complete chunk header after the last one) before a non-pixel chunk.
fn pixel_blocks(data: &[u8], first_idat: usize) -> (Vec<&[u8]>, bool) {
    let mut blocks = Vec::new();
    let mut at = first_idat;
    loop {
        let Some(length) = be32(data, at).and_then(|n| usize::try_from(n).ok()) else {
            return (blocks, true);
        };
        let (mut body, mut length) = (at + 8, length);
        // `fdAT` carries a 4-byte sequence number in front of the data.
        if data.get(at + 4..at + 8) == Some(b"fdAT") {
            body += 4;
            length = length.saturating_sub(4);
        }
        let end = body.saturating_add(length);
        let payload = data.get(body.min(data.len())..end.min(data.len())).unwrap_or(&[]);
        blocks.extend(payload.chunks(BLOCK));
        if end > data.len() {
            return (blocks, true);
        }
        // Skip the CRC without reading it, then the next chunk header must say IDAT/fdAT.
        at = at + 8 + usize::try_from(be32(data, at).unwrap_or(0)).unwrap_or(0) + 4;
        match data.get(at + 4..at + 8) {
            Some(b"IDAT" | b"fdAT") if data.len() >= at + 8 => {}
            Some(cid) if is_cid(cid) && data.len() >= at + 8 => return (blocks, false),
            _ => return (blocks, true),
        }
    }
}

/// The filtered rows `inflate` yields when fed `blocks`, the way `ZipDecode` drives it: one row
/// per call, only while input is left, so a stream whose input ran out before the last row is
/// "truncated" even where zlib could still produce the rest from its state. The call that fills
/// the last row also runs the stream to its end (and so the adler32 check) as far as the input
/// of that block reaches; the rest of the input is ignored.
fn inflate_rows(blocks: &[&[u8]], row_lens: impl Iterator<Item = usize>) -> Result<Vec<u8>, &'static str> {
    const BROKEN: &str = "broken data stream when reading image file";
    let mut z = Decompress::new(true);
    let mut out: Vec<u8> = Vec::new();
    let mut rows = row_lens;
    let Some(mut row_len) = rows.next() else {
        return Ok(out);
    };
    let mut row = vec![0u8; row_len];
    let mut filled = 0usize;
    for block in blocks {
        let mut input = *block;
        while !input.is_empty() {
            let (in0, out0) = (z.total_in(), z.total_out());
            let status = z
                .decompress(input, &mut row[filled..], FlushDecompress::None)
                .map_err(|_| BROKEN)?;
            input = &input[(z.total_in() - in0) as usize..];
            filled += (z.total_out() - out0) as usize;
            if filled == row_len {
                out.extend_from_slice(&row);
                match rows.next() {
                    Some(next) => {
                        row_len = next;
                        row.resize(row_len, 0);
                        filled = 0;
                    }
                    None => {
                        // Run the stream's tail on, without room for more rows.
                        z.decompress(input, &mut [], FlushDecompress::None)
                            .map_err(|_| BROKEN)?;
                        return Ok(out);
                    }
                }
            } else if status == flate2::Status::StreamEnd {
                return Err(TRUNCATED);
            }
        }
    }
    Err(TRUNCATED)
}

const TRUNCATED: &str = "image file is truncated (0 bytes not processed)";

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
    if header.raw_len().is_none() {
        return Ok(Cow::Borrowed(data));
    }
    let (blocks, _ran_out) = pixel_blocks(data, first_idat);
    let rows = inflate_rows(&blocks, header.row_lens()).map_err(str::to_string)?;
    Ok(Cow::Owned(restate(data, first_idat, &rows)))
}
