//! The JPEG decoder Pillow uses (libjpeg-turbo 3.1, `JDCT_ISLOW`, fancy upsampling), ported so the
//! pixels equal Pillow's.
//!
//! `core/image_compare.py` hashes pictures, and a perceptual hash is a comparison of neighbouring
//! pixels: a decoder that is one grey level off in a handful of places flips a bit now and then,
//! and one bit decides a page choice (`multipage.choose_matching_page`). The `image` crate's
//! decoder is not libjpeg's (measured: most JPEGs differ from Pillow's in 1% to 30% of samples by
//! 1 to 37 levels), so the decode of a JPEG is ported from libjpeg, and everything else about the
//! picture then goes through [`crate::pyraster`].
//!
//! What is ported, from `jdmarker.c`, `jdhuff.c`, `jdphuff.c`, `jdcoefct.c`, `jdmainct.c`,
//! `jidctint.c`, `jidctred.c`, `jdsample.c`, `jdcolor.c` and `jdmaster.c`:
//!
//! * baseline, extended-sequential and progressive Huffman scans, restart intervals (with
//!   `resync_to_restart`), the "hit a marker early, finish with zero bits and leave the rest grey"
//!   behaviour, and the warnings libjpeg shrugs off (a bad Huffman code reads as 0);
//! * the accurate integer IDCT, and the 4x4, 2x2 and 1x1 reduced IDCTs behind `Image.draft` (the
//!   DCT-domain downscale by 2, 4 and 8), with the per-component DCT size libjpeg picks so a
//!   subsampled chroma plane needs no upsampling when the picture is scaled down;
//! * fancy upsampling (`h2v1`, `h2v2`, `h1v2`, with the replicated edge rows libjpeg's context
//!   buffer supplies), plain replication where libjpeg falls back to it, `YCbCr` to RGB and `YCCK`
//!   to CMYK by libjpeg's fixed-point tables, grey, RGB and CMYK output.
//!
//! What is not: arithmetic coding, lossless JPEG, and progressive files that stop short of full
//! precision in a band (libjpeg's block smoothing would then run). Those read as
//! [`Error::Unsupported`]; `docs/image-compare.md` lists them with what the Python side does.
//! Truncated entropy data (the file ends before a marker) is [`Error::Truncated`], which is what
//! Pillow's "image file is truncated" is.

use std::fmt;

const DCTSIZE: usize = 8;

/// zig-zag position to natural (row-major) position, padded by 16 entries of 63 as libjpeg-turbo
/// does so that a run length past the end of a block cannot index out of range.
const NATURAL_ORDER: [u8; 80] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63,
    63, 63, 63, 63, 63, 63,
];

/// The standard Huffman tables of Annex K (`jstdhuff.c`), which libjpeg-turbo installs for any of
/// the four slots 0 and 1 a file did not define by the time its first scan starts (Motion JPEG
/// frames omit them).
fn standard_table(ac: bool, slot: usize) -> Huff {
    const DC_LUM: ([u8; 16], &[u8]) = (
        [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
    );
    const DC_CHR: ([u8; 16], &[u8]) = (
        [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
    );
    const AC_LUM: ([u8; 16], &[u8]) = (
        [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d],
        &[
            0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51,
            0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1,
            0x15, 0x52, 0xd1, 0xf0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18,
            0x19, 0x1a, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39,
            0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57,
            0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75,
            0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x92,
            0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
            0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3,
            0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8,
            0xd9, 0xda, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2,
            0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
        ],
    );
    const AC_CHR: ([u8; 16], &[u8]) = (
        [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77],
        &[
            0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07,
            0x61, 0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09,
            0x23, 0x33, 0x52, 0xf0, 0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25,
            0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38,
            0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56,
            0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74,
            0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
            0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5,
            0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba,
            0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6,
            0xd7, 0xd8, 0xd9, 0xda, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2,
            0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
        ],
    );
    let (counts, symbols) = match (ac, slot) {
        (false, 0) => DC_LUM,
        (false, _) => DC_CHR,
        (true, 0) => AC_LUM,
        (true, _) => AC_CHR,
    };
    Huff::build(&counts, symbols, !ac).expect("the standard Huffman tables are valid")
}

/// Why a JPEG could not be decoded. Pillow raises `OSError` for all of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The data ended where libjpeg needed more (Pillow: "image file is truncated").
    Truncated,
    /// libjpeg's `error_exit`: the file is not a JPEG it can decode.
    Corrupt(&'static str),
    /// Valid, but outside what is ported (see the module comment).
    Unsupported(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated => f.write_str("image file is truncated"),
            Error::Corrupt(m) => write!(f, "broken JPEG: {m}"),
            Error::Unsupported(m) => write!(f, "unsupported JPEG: {m}"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// Pillow's mode for the decoded picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One component: mode `L`.
    Gray,
    /// Three components: mode `RGB`.
    Rgb,
    /// Four components: mode `CMYK` (Pillow reads them as Adobe's inverted `CMYK;I`).
    Cmyk,
}

impl Kind {
    pub fn channels(self) -> usize {
        match self {
            Kind::Gray => 1,
            Kind::Rgb => 3,
            Kind::Cmyk => 4,
        }
    }
}

/// A decoded picture, interleaved, `width * height * kind.channels()` bytes.
#[derive(Debug, Clone)]
pub struct Decoded {
    pub width: usize,
    pub height: usize,
    pub kind: Kind,
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Space {
    Gray,
    Rgb,
    YCbCr,
    Cmyk,
    Ycck,
}

#[derive(Clone)]
struct Component {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    width_in_blocks: usize,
    height_in_blocks: usize,
    /// Blocks per row / column of the coefficient array, rounded up to whole MCUs.
    padded_w: usize,
    padded_h: usize,
    /// `compptr->_DCT_scaled_size`.
    dct_size: usize,
    downsampled_w: usize,
    downsampled_h: usize,
    coefs: Vec<i16>,
}

#[derive(Clone)]
struct Huff {
    maxcode: [i32; 18],
    valoffset: [i32; 17],
    huffval: Vec<u8>,
}

impl Huff {
    fn build(counts: &[u8; 16], symbols: &[u8], is_dc: bool) -> Result<Huff> {
        let bad = Error::Corrupt("bad Huffman table");
        let mut huffsize = Vec::with_capacity(256);
        for (l, &n) in counts.iter().enumerate() {
            if huffsize.len() + usize::from(n) > 256 {
                return Err(bad);
            }
            huffsize.extend(std::iter::repeat_n(l + 1, usize::from(n)));
        }
        let mut huffcode = vec![0i32; huffsize.len()];
        let mut code = 0i32;
        let mut p = 0;
        if let Some(&first) = huffsize.first() {
            let mut si = first;
            while p < huffsize.len() {
                while p < huffsize.len() && huffsize[p] == si {
                    huffcode[p] = code;
                    code += 1;
                    p += 1;
                }
                if code >= (1 << si) {
                    return Err(bad);
                }
                code <<= 1;
                si += 1;
            }
        }
        let mut maxcode = [-1i32; 18];
        let mut valoffset = [0i32; 17];
        let mut p = 0usize;
        for l in 1..=16 {
            let n = usize::from(counts[l - 1]);
            if n > 0 {
                valoffset[l] = p as i32 - huffcode[p];
                p += n;
                maxcode[l] = huffcode[p - 1];
            }
        }
        maxcode[17] = 0xF_FFFF;
        if is_dc && symbols.iter().take(huffsize.len()).any(|&s| s > 15) {
            return Err(bad);
        }
        Ok(Huff {
            maxcode,
            valoffset,
            huffval: symbols.to_vec(),
        })
    }
}

/// The entropy decoder's bit reader: `jdhuff.c`'s `bitread_perm_state` and `fill_bit_buffer`.
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u64,
    bits: u32,
    /// `cinfo->unread_marker`.
    marker: Option<u8>,
    /// `entropy->pub.insufficient_data`.
    insufficient: bool,
}

/// `MIN_GET_BITS` for a 64-bit bit buffer.
const MIN_GET_BITS: u32 = 64 - 7;

impl<'a> Bits<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Bits {
            data,
            pos,
            buf: 0,
            bits: 0,
            marker: None,
            insufficient: false,
        }
    }

    fn fill(&mut self, need: u32) -> Result<()> {
        if self.marker.is_none() {
            while self.bits < MIN_GET_BITS {
                let Some(&c) = self.data.get(self.pos) else {
                    return Err(Error::Truncated);
                };
                self.pos += 1;
                let mut c = c;
                if c == 0xFF {
                    loop {
                        let Some(&c2) = self.data.get(self.pos) else {
                            return Err(Error::Truncated);
                        };
                        self.pos += 1;
                        if c2 == 0xFF {
                            continue;
                        }
                        if c2 == 0 {
                            c = 0xFF;
                        } else {
                            self.marker = Some(c2);
                        }
                        break;
                    }
                    if self.marker.is_some() {
                        break;
                    }
                }
                self.buf = (self.buf << 8) | u64::from(c);
                self.bits += 8;
            }
        }
        if need > self.bits {
            // `JWRN_HIT_MARKER`: pad with zero bits and let the rest of the segment read as grey.
            self.insufficient = true;
            self.buf <<= MIN_GET_BITS - self.bits;
            self.bits = MIN_GET_BITS;
        }
        Ok(())
    }

    fn get_bits(&mut self, n: u32) -> Result<i32> {
        if n == 0 {
            return Ok(0);
        }
        if self.bits < n {
            self.fill(n)?;
        }
        self.bits -= n;
        Ok(((self.buf >> self.bits) & ((1u64 << n) - 1)) as i32)
    }

    fn decode(&mut self, tbl: &Huff) -> Result<u8> {
        if self.bits < 8 {
            self.fill(0)?;
        }
        let mut l = 1usize;
        let mut code = self.get_bits(1)?;
        while code > tbl.maxcode[l] {
            code = (code << 1) | self.get_bits(1)?;
            l += 1;
        }
        if l > 16 {
            // `JWRN_HUFF_BAD_CODE`
            return Ok(0);
        }
        Ok(tbl
            .huffval
            .get((code + tbl.valoffset[l]) as usize)
            .copied()
            .unwrap_or(0))
    }

    /// `next_marker`: skips to the next marker, `None` if the data ends first.
    fn next_marker(&mut self) -> Result<u8> {
        loop {
            let mut c = *self.data.get(self.pos).ok_or(Error::Truncated)?;
            self.pos += 1;
            while c != 0xFF {
                c = *self.data.get(self.pos).ok_or(Error::Truncated)?;
                self.pos += 1;
            }
            loop {
                c = *self.data.get(self.pos).ok_or(Error::Truncated)?;
                self.pos += 1;
                if c != 0xFF {
                    break;
                }
            }
            if c != 0 {
                return Ok(c);
            }
        }
    }
}

#[inline]
fn extend(x: i32, s: u32) -> i32 {
    if x < (1 << (s - 1)) {
        x + (-1 << s) + 1
    } else {
        x
    }
}

struct Frame {
    width: usize,
    height: usize,
    progressive: bool,
    comps: Vec<Component>,
    max_h: usize,
    max_v: usize,
}

struct ScanComp {
    ci: usize,
    dc: usize,
    ac: usize,
}

/// Decodes `data` as Pillow does for a JPEG, scaled down by `scale_denom` (1, 2, 4 or 8) in the
/// DCT domain as `Image.draft` asks.
pub fn decode(data: &[u8], scale_denom: usize) -> Result<Decoded> {
    assert!(matches!(scale_denom, 1 | 2 | 4 | 8));
    if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8 {
        return Err(Error::Corrupt("not a JPEG file"));
    }
    let mut pos = 2usize;
    let mut frame: Option<Frame> = None;
    let mut qt: [Option<[u16; 64]>; 4] = [None; 4];
    let mut dc_tabs: [Option<Huff>; 4] = [None, None, None, None];
    let mut ac_tabs: [Option<Huff>; 4] = [None, None, None, None];
    let mut restart_interval = 0usize;
    let mut saw_jfif = false;
    let mut adobe: Option<u8> = None;
    let mut saw_scan = false;
    // `has_multiple_scans`: with one scan libjpeg writes rows as the MCUs are decoded and never
    // needs the marker after it (Pillow ignores `jpeg_finish_decompress` suspending for it).
    let mut multiple_scans = false;
    // A marker a scan's entropy decoder stopped at, to be handled before reading another.
    let mut pending: Option<u8> = None;

    loop {
        let marker = match pending.take() {
            Some(m) => m,
            None => {
                let mut b = Bits::new(data, pos);
                match b.next_marker() {
                    Ok(m) => {
                        pos = b.pos;
                        m
                    }
                    Err(Error::Truncated) if saw_scan && !multiple_scans => break,
                    Err(e) => return Err(e),
                }
            }
        };
        match marker {
            0xC0..=0xC2 => {
                if frame.is_some() {
                    return Err(Error::Corrupt("duplicate SOF"));
                }
                let body = segment(data, &mut pos)?;
                frame = Some(parse_sof(body, marker == 0xC2, scale_denom)?);
            }
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                return Err(Error::Unsupported(
                    "arithmetic, lossless or hierarchical JPEG",
                ));
            }
            0xC4 => {
                let body = segment(data, &mut pos)?;
                let mut i = 0usize;
                while i < body.len() {
                    if body.len() - i < 17 {
                        return Err(Error::Corrupt("bad DHT length"));
                    }
                    let index = body[i];
                    let mut counts = [0u8; 16];
                    counts.copy_from_slice(&body[i + 1..i + 17]);
                    let total: usize = counts.iter().map(|&c| usize::from(c)).sum();
                    if total > 256 || body.len() - i - 17 < total {
                        return Err(Error::Corrupt("bad DHT length"));
                    }
                    let symbols = &body[i + 17..i + 17 + total];
                    i += 17 + total;
                    let is_ac = index & 0x10 != 0;
                    let slot = usize::from(index & 0x0F);
                    if slot >= 4 || index & 0xE0 != 0 {
                        return Err(Error::Corrupt("bad DHT index"));
                    }
                    let table = Huff::build(&counts, symbols, !is_ac)?;
                    if is_ac {
                        ac_tabs[slot] = Some(table);
                    } else {
                        dc_tabs[slot] = Some(table);
                    }
                }
            }
            0xDB => {
                let body = segment(data, &mut pos)?;
                let mut i = 0usize;
                while i < body.len() {
                    let n = body[i];
                    i += 1;
                    let slot = usize::from(n & 15);
                    let wide = n >> 4 != 0;
                    if slot >= 4 {
                        return Err(Error::Corrupt("bad DQT index"));
                    }
                    let need = if wide { 128 } else { 64 };
                    if body.len() - i < need {
                        return Err(Error::Corrupt("bad DQT length"));
                    }
                    let mut table = [0u16; 64];
                    for k in 0..64 {
                        let v = if wide {
                            u16::from_be_bytes([body[i + 2 * k], body[i + 2 * k + 1]])
                        } else {
                            u16::from(body[i + k])
                        };
                        table[usize::from(NATURAL_ORDER[k])] = v;
                    }
                    i += need;
                    qt[slot] = Some(table);
                }
            }
            0xDD => {
                let body = segment(data, &mut pos)?;
                if body.len() != 2 {
                    return Err(Error::Corrupt("bad DRI length"));
                }
                restart_interval = usize::from(u16::from_be_bytes([body[0], body[1]]));
            }
            0xE0 => {
                let body = segment(data, &mut pos)?;
                if body.len() >= 14 && &body[..5] == b"JFIF\0" {
                    saw_jfif = true;
                }
            }
            0xEE => {
                let body = segment(data, &mut pos)?;
                if body.len() >= 12 && &body[..5] == b"Adobe" {
                    adobe = Some(body[11]);
                }
            }
            0xDA => {
                let body = segment(data, &mut pos)?;
                let Some(fr) = frame.as_mut() else {
                    return Err(Error::Corrupt("SOS before SOF"));
                };
                if !saw_scan {
                    for slot in 0..2 {
                        dc_tabs[slot].get_or_insert_with(|| standard_table(false, slot));
                        ac_tabs[slot].get_or_insert_with(|| standard_table(true, slot));
                    }
                    multiple_scans = fr.progressive
                        || body
                            .first()
                            .is_none_or(|&n| usize::from(n) < fr.comps.len());
                }
                let (scan, ss, se, ah, al) = parse_sos(body, fr, &dc_tabs, &ac_tabs)?;
                saw_scan = true;
                let mut bits = Bits::new(data, pos);
                decode_scan(
                    fr,
                    &scan,
                    (ss, se, ah, al),
                    restart_interval,
                    &dc_tabs,
                    &ac_tabs,
                    &mut bits,
                )?;
                pos = bits.pos;
                pending = bits.marker;
            }
            0xD9 => break,
            // RSTn and TEM have no parameter segment.
            0x01 | 0xD0..=0xD7 => {}
            // The other APPn, COM and DNL are read past.
            0xE1..=0xED | 0xEF | 0xFE | 0xDC => {
                segment(data, &mut pos)?;
            }
            _ => return Err(Error::Corrupt("unknown marker")),
        }
    }
    let Some(frame) = frame else {
        return Err(Error::Corrupt("no image"));
    };
    if !saw_scan {
        return Err(Error::Corrupt("no scan"));
    }
    finish(frame, &qt, scale_denom, saw_jfif, adobe)
}

/// A marker's parameter segment (the length includes its own two bytes).
fn segment<'a>(data: &'a [u8], pos: &mut usize) -> Result<&'a [u8]> {
    let len = data.get(*pos..*pos + 2).ok_or(Error::Truncated)?;
    let len = usize::from(u16::from_be_bytes([len[0], len[1]]));
    if len < 2 {
        return Err(Error::Corrupt("bad marker length"));
    }
    let body = data.get(*pos + 2..*pos + len).ok_or(Error::Truncated)?;
    *pos += len;
    Ok(body)
}

fn parse_sof(body: &[u8], progressive: bool, scale_denom: usize) -> Result<Frame> {
    if body.len() < 6 {
        return Err(Error::Corrupt("bad SOF length"));
    }
    if body[0] != 8 {
        return Err(Error::Unsupported("sample precision other than 8"));
    }
    let height = usize::from(u16::from_be_bytes([body[1], body[2]]));
    let width = usize::from(u16::from_be_bytes([body[3], body[4]]));
    let n = usize::from(body[5]);
    if height == 0 || width == 0 || n == 0 {
        return Err(Error::Corrupt("empty image"));
    }
    if body.len() != 6 + 3 * n {
        return Err(Error::Corrupt("bad SOF length"));
    }
    if n > 4 {
        return Err(Error::Corrupt("too many components"));
    }
    let mut comps = Vec::new();
    for i in 0..n {
        let c = &body[6 + 3 * i..9 + 3 * i];
        let (h, v) = (usize::from(c[1] >> 4), usize::from(c[1] & 15));
        if !(1..=4).contains(&h) || !(1..=4).contains(&v) {
            return Err(Error::Corrupt("bad sampling factors"));
        }
        comps.push(Component {
            id: c[0],
            h,
            v,
            tq: usize::from(c[2]),
            width_in_blocks: 0,
            height_in_blocks: 0,
            padded_w: 0,
            padded_h: 0,
            dct_size: 0,
            downsampled_w: 0,
            downsampled_h: 0,
            coefs: Vec::new(),
        });
    }
    let max_h = comps.iter().map(|c| c.h).max().unwrap_or(1);
    let max_v = comps.iter().map(|c| c.v).max().unwrap_or(1);
    let min_dct = DCTSIZE / scale_denom;
    let mcu_cols = width.div_ceil(max_h * DCTSIZE);
    let mcu_rows = height.div_ceil(max_v * DCTSIZE);
    for c in &mut comps {
        c.width_in_blocks = (width * c.h).div_ceil(max_h * DCTSIZE);
        c.height_in_blocks = (height * c.v).div_ceil(max_v * DCTSIZE);
        c.padded_w = mcu_cols * c.h;
        c.padded_h = mcu_rows * c.v;
        // `jpeg_calc_output_dimensions`: a component with fewer samples than the largest is
        // decoded at a larger DCT size when that makes upsampling unnecessary.
        let mut ssize = min_dct;
        while ssize < DCTSIZE
            && (max_h * min_dct) % (c.h * ssize * 2) == 0
            && (max_v * min_dct) % (c.v * ssize * 2) == 0
        {
            ssize *= 2;
        }
        c.dct_size = ssize;
        c.downsampled_w = (width * c.h * ssize).div_ceil(max_h * DCTSIZE);
        c.downsampled_h = (height * c.v * ssize).div_ceil(max_v * DCTSIZE);
        c.coefs = vec![0i16; c.padded_w * c.padded_h * 64];
    }
    Ok(Frame {
        width,
        height,
        progressive,
        comps,
        max_h,
        max_v,
    })
}

type ScanParams = (Vec<ScanComp>, usize, usize, usize, usize);

fn parse_sos(
    body: &[u8],
    fr: &Frame,
    dc_tabs: &[Option<Huff>; 4],
    ac_tabs: &[Option<Huff>; 4],
) -> Result<ScanParams> {
    let bad = Error::Corrupt("bad SOS");
    let n = usize::from(*body.first().ok_or(Error::Corrupt("bad SOS length"))?);
    if body.len() != 4 + 2 * n || !(1..=4).contains(&n) {
        return Err(Error::Corrupt("bad SOS length"));
    }
    let mut scan = Vec::new();
    for i in 0..n {
        let id = body[1 + 2 * i];
        let t = body[2 + 2 * i];
        let Some(ci) = fr.comps.iter().position(|c| c.id == id) else {
            return Err(Error::Corrupt("bad component id in SOS"));
        };
        if scan.iter().any(|s: &ScanComp| s.ci == ci) {
            return Err(Error::Corrupt("duplicate component in SOS"));
        }
        let (dc, ac) = (usize::from(t >> 4), usize::from(t & 15));
        if dc >= 4 || ac >= 4 {
            return Err(bad);
        }
        scan.push(ScanComp { ci, dc, ac });
    }
    let (ss, se) = (usize::from(body[1 + 2 * n]), usize::from(body[2 + 2 * n]));
    let (ah, al) = (
        usize::from(body[3 + 2 * n] >> 4),
        usize::from(body[3 + 2 * n] & 15),
    );
    // Tables a scan names must exist (`jpeg_make_d_derived_tbl`: "no Huffman table").
    let need_dc = !fr.progressive || (ss == 0 && ah == 0);
    let need_ac = !fr.progressive || ss != 0;
    let dc_refine = fr.progressive && ss == 0 && ah != 0;
    for s in &scan {
        if (need_dc && !dc_refine && dc_tabs[s.dc].is_none())
            || (need_ac && ac_tabs[s.ac].is_none())
        {
            return Err(Error::Corrupt("missing Huffman table"));
        }
    }
    if fr.progressive {
        let ok = if ss == 0 {
            se == 0
        } else {
            ss <= se && se < 64 && scan.len() == 1
        };
        if !ok || (ah != 0 && al != ah - 1) || al > 13 {
            return Err(Error::Corrupt("bad progression parameters"));
        }
    }
    Ok((scan, ss, se, ah, al))
}

#[allow(clippy::too_many_arguments)]
fn decode_scan(
    fr: &mut Frame,
    scan: &[ScanComp],
    (ss, se, ah, al): (usize, usize, usize, usize),
    restart_interval: usize,
    dc_tabs: &[Option<Huff>; 4],
    ac_tabs: &[Option<Huff>; 4],
    bits: &mut Bits,
) -> Result<()> {
    // The blocks of each MCU, as (scan component index, dx, dy).
    let (mcu_cols, mcu_rows, layout): (usize, usize, Vec<(usize, usize, usize)>) =
        if scan.len() == 1 {
            let c = &fr.comps[scan[0].ci];
            (c.width_in_blocks, c.height_in_blocks, vec![(0, 0, 0)])
        } else {
            let mut layout = Vec::new();
            for (si, s) in scan.iter().enumerate() {
                let c = &fr.comps[s.ci];
                for y in 0..c.v {
                    for x in 0..c.h {
                        layout.push((si, x, y));
                    }
                }
            }
            if layout.len() > 10 {
                return Err(Error::Corrupt("MCU too large"));
            }
            (
                fr.width.div_ceil(fr.max_h * DCTSIZE),
                fr.height.div_ceil(fr.max_v * DCTSIZE),
                layout,
            )
        };
    let interleaved = scan.len() > 1;
    let mut last_dc = [0i32; 4];
    let mut eobrun = 0u32;
    let mut restarts_to_go = restart_interval;
    let mut next_restart = 0u8;
    let progressive = fr.progressive;

    for mcu_row in 0..mcu_rows {
        for mcu_col in 0..mcu_cols {
            if restart_interval != 0 && restarts_to_go == 0 {
                // `process_restart`
                bits.bits = 0;
                bits.buf = 0;
                if bits.marker.is_none() {
                    let m = bits.next_marker()?;
                    bits.marker = Some(m);
                }
                if bits.marker == Some(0xD0 + next_restart) {
                    bits.marker = None;
                } else {
                    resync_to_restart(bits, next_restart)?;
                }
                next_restart = (next_restart + 1) & 7;
                last_dc = [0; 4];
                eobrun = 0;
                restarts_to_go = restart_interval;
                if bits.marker.is_none() {
                    bits.insufficient = false;
                }
            }
            if !bits.insufficient {
                for &(si, dx, dy) in &layout {
                    let s = &scan[si];
                    let c = &mut fr.comps[s.ci];
                    let (bx, by) = if interleaved {
                        (mcu_col * c.h + dx, mcu_row * c.v + dy)
                    } else {
                        (mcu_col, mcu_row)
                    };
                    let off = (by * c.padded_w + bx) * 64;
                    let block: &mut [i16] = &mut c.coefs[off..off + 64];
                    if !progressive {
                        let dct = dc_tabs[s.dc].as_ref().ok_or(Error::Corrupt("table"))?;
                        let act = ac_tabs[s.ac].as_ref().ok_or(Error::Corrupt("table"))?;
                        decode_block_baseline(bits, dct, act, &mut last_dc[si], block)?;
                    } else if ss == 0 {
                        if ah == 0 {
                            let dct = dc_tabs[s.dc].as_ref().ok_or(Error::Corrupt("table"))?;
                            let t = usize::from(bits.decode(dct)?);
                            let mut d = 0;
                            if t != 0 {
                                let r = bits.get_bits(t as u32)?;
                                d = extend(r, t as u32);
                            }
                            d = d.wrapping_add(last_dc[si]);
                            last_dc[si] = d;
                            block[0] = (d << al) as i16;
                        } else if bits.get_bits(1)? != 0 {
                            block[0] |= (1i32 << al) as i16;
                        }
                    } else {
                        let act = ac_tabs[s.ac].as_ref().ok_or(Error::Corrupt("table"))?;
                        if ah == 0 {
                            decode_ac_first(bits, act, block, ss, se, al, &mut eobrun)?;
                        } else {
                            decode_ac_refine(bits, act, block, ss, se, al, &mut eobrun)?;
                        }
                    }
                }
            }
            if restart_interval != 0 {
                restarts_to_go = restarts_to_go.wrapping_sub(1);
            }
        }
    }
    // The bytes between the last MCU and the next marker (padding bits, and any junk).
    Ok(())
}

fn resync_to_restart(bits: &mut Bits, desired: u8) -> Result<()> {
    let mut marker = bits.marker.unwrap_or(0);
    loop {
        let action = if marker < 0xC0 {
            2
        } else if !(0xD0..=0xD7).contains(&marker) {
            3
        } else {
            let m = marker - 0xD0;
            if m == (desired + 1) & 7 || m == (desired + 2) & 7 {
                3
            } else if m == desired.wrapping_sub(1) & 7 || m == desired.wrapping_sub(2) & 7 {
                2
            } else {
                1
            }
        };
        match action {
            1 => {
                bits.marker = None;
                return Ok(());
            }
            2 => {
                marker = bits.next_marker()?;
                bits.marker = Some(marker);
            }
            _ => return Ok(()),
        }
    }
}

fn decode_block_baseline(
    bits: &mut Bits,
    dct: &Huff,
    act: &Huff,
    last_dc: &mut i32,
    block: &mut [i16],
) -> Result<()> {
    let t = u32::from(bits.decode(dct)?);
    let mut s = 0;
    if t != 0 {
        let r = bits.get_bits(t)?;
        s = extend(r, t);
    }
    s = s.wrapping_add(*last_dc);
    *last_dc = s;
    block[0] = s as i16;
    let mut k = 1usize;
    while k < 64 {
        let sym = bits.decode(act)?;
        let (r, s) = (usize::from(sym >> 4), u32::from(sym & 15));
        if s != 0 {
            k += r;
            let v = bits.get_bits(s)?;
            block[usize::from(NATURAL_ORDER[k])] = extend(v, s) as i16;
        } else {
            if r != 15 {
                break;
            }
            k += 15;
        }
        k += 1;
    }
    Ok(())
}

fn decode_ac_first(
    bits: &mut Bits,
    act: &Huff,
    block: &mut [i16],
    ss: usize,
    se: usize,
    al: usize,
    eobrun: &mut u32,
) -> Result<()> {
    if *eobrun > 0 {
        *eobrun -= 1;
        return Ok(());
    }
    let mut k = ss;
    while k <= se {
        let sym = bits.decode(act)?;
        let (r, s) = (usize::from(sym >> 4), u32::from(sym & 15));
        if s != 0 {
            k += r;
            let v = bits.get_bits(s)?;
            block[usize::from(NATURAL_ORDER[k])] = (extend(v, s) << al) as i16;
        } else if r == 15 {
            k += 15;
        } else {
            *eobrun = 1 << r;
            if r != 0 {
                *eobrun += bits.get_bits(r as u32)? as u32;
            }
            *eobrun -= 1;
            break;
        }
        k += 1;
    }
    Ok(())
}

fn decode_ac_refine(
    bits: &mut Bits,
    act: &Huff,
    block: &mut [i16],
    ss: usize,
    se: usize,
    al: usize,
    eobrun: &mut u32,
) -> Result<()> {
    let p1 = (1i32 << al) as i16;
    let m1 = (-1i32 << al) as i16;
    let mut k = ss;
    let correct = |bits: &mut Bits, coef: &mut i16| -> Result<()> {
        if bits.get_bits(1)? != 0 && *coef & p1 == 0 {
            *coef = coef.wrapping_add(if *coef >= 0 { p1 } else { m1 });
        }
        Ok(())
    };
    if *eobrun == 0 {
        while k <= se {
            let sym = bits.decode(act)?;
            let (mut r, s) = (i32::from(sym >> 4), u32::from(sym & 15));
            let mut newval = 0i16;
            if s != 0 {
                newval = if bits.get_bits(1)? != 0 { p1 } else { m1 };
            } else if r != 15 {
                *eobrun = 1 << r;
                if r != 0 {
                    *eobrun += bits.get_bits(r as u32)? as u32;
                }
                break;
            }
            loop {
                let pos = usize::from(NATURAL_ORDER[k]);
                if block[pos] != 0 {
                    correct(bits, &mut block[pos])?;
                } else {
                    r -= 1;
                    if r < 0 {
                        break;
                    }
                }
                k += 1;
                if k > se {
                    break;
                }
            }
            if newval != 0 {
                block[usize::from(NATURAL_ORDER[k])] = newval;
            }
            k += 1;
        }
    }
    if *eobrun > 0 {
        while k <= se {
            let pos = usize::from(NATURAL_ORDER[k]);
            if block[pos] != 0 {
                correct(bits, &mut block[pos])?;
            }
            k += 1;
        }
        *eobrun -= 1;
    }
    Ok(())
}

const CONST_BITS: i32 = 13;
const PASS1_BITS: i32 = 2;

/// `idct_range_limit[x & RANGE_MASK]`: clamps `x + 128` to 0..255, wrapping as libjpeg's table does
/// for values far out of range.
#[inline]
fn range_limit(x: i32) -> u8 {
    let i = x & 1023;
    if i < 128 {
        (i + 128) as u8
    } else if i < 512 {
        255
    } else if i < 896 {
        0
    } else {
        (i - 896) as u8
    }
}

/// What the SIMD IDCTs (`jidctint-avx2.asm`, `jidctred-sse2.asm`: the 8x8, 4x4 and 2x2 ones, which
/// is what Pillow's wheels run) store: the sample saturated to 0..255. libjpeg's C code indexes a
/// wrapping table instead, and the two only differ for a sample more than 384 outside the range,
/// which damaged data produces and an encoder never does. The 1x1 IDCT has no SIMD version.
#[inline]
fn saturate(x: i32) -> u8 {
    (x + 128).clamp(0, 255) as u8
}

/// A coefficient times its quantiser as the SIMD IDCTs compute it, `pmullw`: the low 16 bits.
#[inline]
fn dequant(coef: i16, q: u16) -> i32 {
    i32::from(coef.wrapping_mul(q as i16))
}

#[inline]
fn descale(x: i32, n: i32) -> i32 {
    (x + (1 << (n - 1))) >> n
}

/// `jpeg_idct_islow`: the 8x8 accurate integer IDCT.
fn idct_islow(coef: &[i16], q: &[u16; 64], out: &mut [u8], stride: usize) {
    let mut ws = [0i32; 64];
    for col in 0..8 {
        let g = |row: usize| dequant(coef[row * 8 + col], q[row * 8 + col]);
        let (z2, z3) = (g(2), g(6));
        let z1 = (z2 + z3) * 4433;
        let tmp2 = z1 + z3 * -15137;
        let tmp3 = z1 + z2 * 6270;
        let (z2, z3) = (g(0), g(4));
        let tmp0 = (z2 + z3) << CONST_BITS;
        let tmp1 = (z2 - z3) << CONST_BITS;
        let (tmp10, tmp13, tmp11, tmp12) = (tmp0 + tmp3, tmp0 - tmp3, tmp1 + tmp2, tmp1 - tmp2);
        let (mut t0, mut t1, mut t2, mut t3) = (g(7), g(5), g(3), g(1));
        let (z1, z2, z3, z4) = (t0 + t3, t1 + t2, t0 + t2, t1 + t3);
        let z5 = (z3 + z4) * 9633;
        t0 *= 2446;
        t1 *= 16819;
        t2 *= 25172;
        t3 *= 12299;
        let (z1, z2) = (z1 * -7373, z2 * -20995);
        let (z3, z4) = (z3 * -16069 + z5, z4 * -3196 + z5);
        t0 += z1 + z3;
        t1 += z2 + z4;
        t2 += z2 + z3;
        t3 += z1 + z4;
        let sh = CONST_BITS - PASS1_BITS;
        ws[col] = descale(tmp10 + t3, sh);
        ws[56 + col] = descale(tmp10 - t3, sh);
        ws[8 + col] = descale(tmp11 + t2, sh);
        ws[48 + col] = descale(tmp11 - t2, sh);
        ws[16 + col] = descale(tmp12 + t1, sh);
        ws[40 + col] = descale(tmp12 - t1, sh);
        ws[24 + col] = descale(tmp13 + t0, sh);
        ws[32 + col] = descale(tmp13 - t0, sh);
    }
    for row in 0..8 {
        let w = &ws[row * 8..row * 8 + 8];
        let (z2, z3) = (w[2], w[6]);
        let z1 = (z2 + z3) * 4433;
        let tmp2 = z1 + z3 * -15137;
        let tmp3 = z1 + z2 * 6270;
        let tmp0 = (w[0] + w[4]) << CONST_BITS;
        let tmp1 = (w[0] - w[4]) << CONST_BITS;
        let (tmp10, tmp13, tmp11, tmp12) = (tmp0 + tmp3, tmp0 - tmp3, tmp1 + tmp2, tmp1 - tmp2);
        let (mut t0, mut t1, mut t2, mut t3) = (w[7], w[5], w[3], w[1]);
        let (z1, z2, z3, z4) = (t0 + t3, t1 + t2, t0 + t2, t1 + t3);
        let z5 = (z3 + z4) * 9633;
        t0 *= 2446;
        t1 *= 16819;
        t2 *= 25172;
        t3 *= 12299;
        let (z1, z2) = (z1 * -7373, z2 * -20995);
        let (z3, z4) = (z3 * -16069 + z5, z4 * -3196 + z5);
        t0 += z1 + z3;
        t1 += z2 + z4;
        t2 += z2 + z3;
        t3 += z1 + z4;
        let sh = CONST_BITS + PASS1_BITS + 3;
        let o = &mut out[row * stride..row * stride + 8];
        o[0] = saturate(descale(tmp10 + t3, sh));
        o[7] = saturate(descale(tmp10 - t3, sh));
        o[1] = saturate(descale(tmp11 + t2, sh));
        o[6] = saturate(descale(tmp11 - t2, sh));
        o[2] = saturate(descale(tmp12 + t1, sh));
        o[5] = saturate(descale(tmp12 - t1, sh));
        o[3] = saturate(descale(tmp13 + t0, sh));
        o[4] = saturate(descale(tmp13 - t0, sh));
    }
}

/// `jpeg_idct_4x4`.
fn idct_4x4(coef: &[i16], q: &[u16; 64], out: &mut [u8], stride: usize) {
    let mut ws = [0i32; 64];
    for col in 0..8 {
        if col == 4 {
            continue;
        }
        let g = |row: usize| dequant(coef[row * 8 + col], q[row * 8 + col]);
        let tmp0 = g(0) << (CONST_BITS + 1);
        let tmp2 = g(2) * 15137 + g(6) * -6270;
        let (tmp10, tmp12) = (tmp0 + tmp2, tmp0 - tmp2);
        let (z1, z2, z3, z4) = (g(7), g(5), g(3), g(1));
        let tmp0 = z1 * -1730 + z2 * 11893 + z3 * -17799 + z4 * 8697;
        let tmp2 = z1 * -4176 + z2 * -4926 + z3 * 7373 + z4 * 20995;
        let sh = CONST_BITS - PASS1_BITS + 1;
        ws[col] = descale(tmp10 + tmp2, sh);
        ws[24 + col] = descale(tmp10 - tmp2, sh);
        ws[8 + col] = descale(tmp12 + tmp0, sh);
        ws[16 + col] = descale(tmp12 - tmp0, sh);
    }
    for row in 0..4 {
        let w = &ws[row * 8..row * 8 + 8];
        let tmp0 = w[0] << (CONST_BITS + 1);
        let tmp2 = w[2] * 15137 + w[6] * -6270;
        let (tmp10, tmp12) = (tmp0 + tmp2, tmp0 - tmp2);
        let (z1, z2, z3, z4) = (w[7], w[5], w[3], w[1]);
        let tmp0 = z1 * -1730 + z2 * 11893 + z3 * -17799 + z4 * 8697;
        let tmp2 = z1 * -4176 + z2 * -4926 + z3 * 7373 + z4 * 20995;
        let sh = CONST_BITS + PASS1_BITS + 3 + 1;
        let o = &mut out[row * stride..row * stride + 4];
        o[0] = saturate(descale(tmp10 + tmp2, sh));
        o[3] = saturate(descale(tmp10 - tmp2, sh));
        o[1] = saturate(descale(tmp12 + tmp0, sh));
        o[2] = saturate(descale(tmp12 - tmp0, sh));
    }
}

/// `jpeg_idct_2x2`.
fn idct_2x2(coef: &[i16], q: &[u16; 64], out: &mut [u8], stride: usize) {
    let mut ws = [0i32; 16];
    for col in [0usize, 1, 3, 5, 7] {
        let g = |row: usize| dequant(coef[row * 8 + col], q[row * 8 + col]);
        let tmp0 = g(7) * -5906 + g(5) * 6967 + g(3) * -10426 + g(1) * 29692;
        let tmp10 = g(0) << (CONST_BITS + 2);
        let sh = CONST_BITS - PASS1_BITS + 2;
        ws[col] = descale(tmp10 + tmp0, sh);
        ws[8 + col] = descale(tmp10 - tmp0, sh);
    }
    for row in 0..2 {
        let w = &ws[row * 8..row * 8 + 8];
        let tmp0 = w[7] * -5906 + w[5] * 6967 + w[3] * -10426 + w[1] * 29692;
        let tmp10 = w[0] << (CONST_BITS + 2);
        let sh = CONST_BITS + PASS1_BITS + 3 + 2;
        out[row * stride] = saturate(descale(tmp10 + tmp0, sh));
        out[row * stride + 1] = saturate(descale(tmp10 - tmp0, sh));
    }
}

/// `jpeg_idct_1x1`.
fn idct_1x1(coef: &[i16], q: &[u16; 64], out: &mut [u8], _stride: usize) {
    let dc = i32::from(coef[0]) * i32::from(q[0]);
    out[0] = range_limit(descale(dc, 3));
}

/// One component after the IDCT: `rows` of `stride` samples.
struct Plane {
    data: Vec<u8>,
    stride: usize,
    /// `downsampled_width` / `downsampled_height`.
    width: usize,
    height: usize,
}

impl Plane {
    #[inline]
    fn row(&self, y: usize) -> &[u8] {
        let y = y.min(self.height - 1);
        &self.data[y * self.stride..y * self.stride + self.width]
    }
}

fn finish(
    frame: Frame,
    qt: &[Option<[u16; 64]>; 4],
    scale_denom: usize,
    saw_jfif: bool,
    adobe: Option<u8>,
) -> Result<Decoded> {
    let n = frame.comps.len();
    let space = match n {
        1 => Space::Gray,
        3 => {
            if saw_jfif {
                Space::YCbCr
            } else if let Some(t) = adobe {
                match t {
                    0 => Space::Rgb,
                    _ => Space::YCbCr,
                }
            } else {
                let ids: Vec<u8> = frame.comps.iter().map(|c| c.id).collect();
                if ids == [b'R', b'G', b'B'] {
                    Space::Rgb
                } else {
                    Space::YCbCr
                }
            }
        }
        4 => match adobe {
            Some(2) => Space::Ycck,
            _ => Space::Cmyk,
        },
        _ => return Err(Error::Corrupt("unsupported colour conversion")),
    };
    let out_w = frame.width.div_ceil(scale_denom);
    let out_h = frame.height.div_ceil(scale_denom);
    let min_dct = DCTSIZE / scale_denom;

    // IDCT every real block.
    let mut planes = Vec::with_capacity(n);
    for c in &frame.comps {
        let table = qt[c.tq]
            .as_ref()
            .ok_or(Error::Corrupt("missing quantisation table"))?;
        let s = c.dct_size;
        let stride = c.width_in_blocks * s;
        let rows = c.height_in_blocks * s;
        let mut data = vec![0u8; stride * rows];
        for by in 0..c.height_in_blocks {
            for bx in 0..c.width_in_blocks {
                let off = (by * c.padded_w + bx) * 64;
                let coef = &c.coefs[off..off + 64];
                let dst = &mut data[(by * s) * stride + bx * s..];
                match s {
                    8 => idct_islow(coef, table, dst, stride),
                    4 => idct_4x4(coef, table, dst, stride),
                    2 => idct_2x2(coef, table, dst, stride),
                    _ => idct_1x1(coef, table, dst, stride),
                }
            }
        }
        planes.push(Plane {
            data,
            stride,
            width: c.downsampled_w,
            height: c.downsampled_h,
        });
    }

    // Upsampling method per component (`jinit_upsampler`).
    #[derive(Clone, Copy)]
    enum Up {
        Full,
        H2v1Fancy,
        H2v2Fancy,
        H1v2Fancy,
        Int(usize, usize),
    }
    let do_fancy = min_dct > 1;
    let mut ups = Vec::new();
    for (c, p) in frame.comps.iter().zip(&planes) {
        let h_in = c.h * c.dct_size / min_dct;
        let v_in = c.v * c.dct_size / min_dct;
        let (h_out, v_out) = (frame.max_h, frame.max_v);
        let up = if h_in == h_out && v_in == v_out {
            Up::Full
        } else if h_in * 2 == h_out && v_in == v_out {
            if do_fancy && p.width > 2 {
                Up::H2v1Fancy
            } else {
                Up::Int(2, 1)
            }
        } else if h_in * 2 == h_out && v_in * 2 == v_out {
            if do_fancy && p.width > 2 {
                Up::H2v2Fancy
            } else {
                Up::Int(2, 2)
            }
        } else if h_in == h_out && v_in * 2 == v_out && do_fancy {
            Up::H1v2Fancy
        } else if h_out % h_in == 0 && v_out % v_in == 0 {
            Up::Int(h_out / h_in, v_out / v_in)
        } else {
            return Err(Error::Unsupported("fractional sampling ratio"));
        };
        ups.push(up);
    }

    let ch = n;
    let mut out = vec![0u8; out_w * out_h * ch];
    let mut rows: Vec<Vec<u8>> = (0..n).map(|_| vec![0u8; out_w.max(1) + 2]).collect();
    let tables = ColorTables::new();
    for y in 0..out_h {
        for ci in 0..n {
            let p = &planes[ci];
            let dst = &mut rows[ci];
            if dst.len() < p.width * 2 {
                dst.resize(p.width * 2, 0);
            }
            match ups[ci] {
                Up::Full => {
                    let src = p.row(y);
                    let m = src.len().min(out_w);
                    dst[..m].copy_from_slice(&src[..m]);
                }
                Up::H2v1Fancy => h2v1_fancy(p.row(y), dst),
                Up::H2v2Fancy => {
                    let inrow = y / 2;
                    let near = p.row(inrow);
                    let far = if y % 2 == 0 {
                        p.row(inrow.saturating_sub(1))
                    } else {
                        p.row(inrow + 1)
                    };
                    h2v2_fancy(near, far, dst);
                }
                Up::H1v2Fancy => {
                    let inrow = y / 2;
                    let near = p.row(inrow);
                    let (far, bias) = if y % 2 == 0 {
                        (p.row(inrow.saturating_sub(1)), 1u32)
                    } else {
                        (p.row(inrow + 1), 2u32)
                    };
                    for (x, d) in dst.iter_mut().enumerate().take(near.len()) {
                        *d = ((u32::from(near[x]) * 3 + u32::from(far[x]) + bias) >> 2) as u8;
                    }
                }
                Up::Int(hx, vy) => {
                    let src = p.row(y / vy);
                    for x in 0..out_w {
                        dst[x] = src[(x / hx).min(src.len() - 1)];
                    }
                }
            }
        }
        let o = &mut out[y * out_w * ch..(y + 1) * out_w * ch];
        for x in 0..out_w {
            match space {
                Space::Gray => o[x] = rows[0][x],
                Space::Rgb => {
                    o[x * 3] = rows[0][x];
                    o[x * 3 + 1] = rows[1][x];
                    o[x * 3 + 2] = rows[2][x];
                }
                Space::YCbCr => {
                    let (r, g, b) = tables.rgb(rows[0][x], rows[1][x], rows[2][x]);
                    o[x * 3] = r;
                    o[x * 3 + 1] = g;
                    o[x * 3 + 2] = b;
                }
                Space::Cmyk => {
                    // Pillow's `CMYK;I` unpacker inverts what libjpeg returns.
                    for k in 0..4 {
                        o[x * 4 + k] = 255 - rows[k][x];
                    }
                }
                Space::Ycck => {
                    let (r, g, b) = tables.rgb(rows[0][x], rows[1][x], rows[2][x]);
                    o[x * 4] = r;
                    o[x * 4 + 1] = g;
                    o[x * 4 + 2] = b;
                    o[x * 4 + 3] = 255 - rows[3][x];
                }
            }
        }
    }
    let kind = match space {
        Space::Gray => Kind::Gray,
        Space::Rgb | Space::YCbCr => Kind::Rgb,
        Space::Cmyk | Space::Ycck => Kind::Cmyk,
    };
    Ok(Decoded {
        width: out_w,
        height: out_h,
        kind,
        data: out,
    })
}

/// `h2v1_fancy_upsample`: triangle filter, 3/4 nearer sample and 1/4 the next.
fn h2v1_fancy(inp: &[u8], out: &mut [u8]) {
    let n = inp.len();
    let v = |i: usize| u32::from(inp[i]);
    out[0] = inp[0];
    out[1] = ((v(0) * 3 + v(1) + 2) >> 2) as u8;
    for i in 1..n - 1 {
        let t = v(i) * 3;
        out[2 * i] = ((t + v(i - 1) + 1) >> 2) as u8;
        out[2 * i + 1] = ((t + v(i + 1) + 2) >> 2) as u8;
    }
    out[2 * (n - 1)] = ((v(n - 1) * 3 + v(n - 2) + 1) >> 2) as u8;
    out[2 * n - 1] = inp[n - 1];
}

/// `h2v2_fancy_upsample` for one output row: `near` is the input row this output row sits in,
/// `far` the one above or below it.
fn h2v2_fancy(near: &[u8], far: &[u8], out: &mut [u8]) {
    let n = near.len();
    let sum = |i: usize| u32::from(near[i]) * 3 + u32::from(far[i]);
    let mut this = sum(0);
    let mut next = sum(1);
    out[0] = ((this * 4 + 8) >> 4) as u8;
    out[1] = ((this * 3 + next + 7) >> 4) as u8;
    let mut last = this;
    this = next;
    for i in 2..n {
        next = sum(i);
        out[2 * (i - 1)] = ((this * 3 + last + 8) >> 4) as u8;
        out[2 * (i - 1) + 1] = ((this * 3 + next + 7) >> 4) as u8;
        last = this;
        this = next;
    }
    out[2 * (n - 1)] = ((this * 3 + last + 8) >> 4) as u8;
    out[2 * n - 1] = ((this * 4 + 7) >> 4) as u8;
}

/// `jdcolor.c`'s fixed-point `YCbCr` to `RGB` tables (`build_ycc_rgb_table`).
struct ColorTables {
    cr_r: [i32; 256],
    cb_b: [i32; 256],
    cr_g: [i32; 256],
    cb_g: [i32; 256],
}

impl ColorTables {
    fn new() -> Self {
        const SCALEBITS: i32 = 16;
        const ONE_HALF: i32 = 1 << (SCALEBITS - 1);
        let fix = |x: f64| (x * f64::from(1 << SCALEBITS) + 0.5) as i32;
        let mut t = ColorTables {
            cr_r: [0; 256],
            cb_b: [0; 256],
            cr_g: [0; 256],
            cb_g: [0; 256],
        };
        for i in 0..256 {
            let x = i as i32 - 128;
            t.cr_r[i] = (fix(1.402) * x + ONE_HALF) >> SCALEBITS;
            t.cb_b[i] = (fix(1.772) * x + ONE_HALF) >> SCALEBITS;
            t.cr_g[i] = -fix(0.71414) * x;
            t.cb_g[i] = -fix(0.34414) * x + ONE_HALF;
        }
        t
    }

    #[inline]
    fn rgb(&self, y: u8, cb: u8, cr: u8) -> (u8, u8, u8) {
        let y = i32::from(y);
        let (cb, cr) = (usize::from(cb), usize::from(cr));
        let c = |v: i32| v.clamp(0, 255) as u8;
        (
            c(y + self.cr_r[cr]),
            c(y + ((self.cb_g[cb] + self.cr_g[cr]) >> 16)),
            c(y + self.cb_b[cb]),
        )
    }
}
