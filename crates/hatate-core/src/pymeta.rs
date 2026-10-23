//! What `check_upscaler_metadata` reads out of an image through Pillow: every `str` and `bytes`
//! value of `Image.info`, then EXIF tags 270, 305 and 315 (`ImageDescription`, `Software`,
//! `Artist`) from `Image.getexif()`.
//!
//! This is a port of the parts of Pillow 12.3's `PngImagePlugin`, `JpegImagePlugin`,
//! `GifImagePlugin`, `WebPImagePlugin` and `TiffImagePlugin` that fill those: which chunk or
//! marker lands in `info` under which key, as which type, and when Pillow gives up on a file
//! (`Image.open` raising), in which case the scan finds nothing. Formats whose `info` holds no
//! text (BMP, ICO, PNM, TGA) give an empty scan.

use flate2::{Decompress, FlushDecompress, Status};

/// An `Image.info` value as far as the scan cares.
#[derive(Debug, Clone)]
enum Val {
    Str(String),
    Bytes(Vec<u8>),
    /// Anything else (ints, floats, tuples, dicts, `None`): skipped by the scan.
    Other,
}

/// `Image.info`: insertion-ordered, a repeated key keeps its first position.
#[derive(Debug, Clone, Default)]
struct Info(Vec<(String, Val)>);

impl Info {
    fn set(&mut self, key: &str, val: Val) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = val,
            None => self.0.push((key.to_string(), val)),
        }
    }

    fn get(&self, key: &str) -> Option<&Val> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

/// The texts `check_upscaler_metadata` joins and searches: the `str`/`bytes` values of `info`
/// (bytes decoded as UTF-8 with errors ignored), then the EXIF tags. `None` is `Image.open`
/// raising (or the file being unidentified): the Python function then returns `None`.
pub(crate) fn metadata_texts(data: &[u8]) -> Option<Vec<String>> {
    let (info, exif) = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(data)?
    } else if data.starts_with(b"\xff\xd8\xff") {
        jpeg(data)?
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        (gif(data)?, None)
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        webp(data)?
    } else if data.starts_with(b"II\x2a\x00") || data.starts_with(b"MM\x00\x2a") {
        tiff(data)?
    } else {
        (Info::default(), None)
    };
    let mut texts = Vec::new();
    for (_, v) in &info.0 {
        match v {
            Val::Str(s) => texts.push(s.clone()),
            Val::Bytes(b) => texts.push(decode_utf8_ignore(b)),
            Val::Other => {}
        }
    }
    if let Some(exif) = exif {
        texts.extend(exif_texts(&exif));
    }
    Some(texts)
}

/// `bytes.decode("utf-8", errors="ignore")`.
fn decode_utf8_ignore(mut b: &[u8]) -> String {
    let mut out = String::new();
    loop {
        match std::str::from_utf8(b) {
            Ok(s) => {
                out.push_str(s);
                return out;
            }
            Err(e) => {
                let (good, rest) = b.split_at(e.valid_up_to());
                out.push_str(std::str::from_utf8(good).expect("valid prefix"));
                match e.error_len() {
                    Some(n) => b = &rest[n..],
                    None => return out,
                }
            }
        }
    }
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

// ---------------------------------------------------------------------------------------------
// PNG

/// `MAX_TEXT_CHUNK` and `ImageFile.SAFEBLOCK`: one decompressed text chunk may not exceed 1 MiB.
const MAX_TEXT_CHUNK: usize = 1024 * 1024;
/// `MAX_TEXT_MEMORY`: all text chunks together may not exceed 64 MiB (in characters).
const MAX_TEXT_MEMORY: usize = 64 * 1024 * 1024;

enum Inflate {
    Done(Vec<u8>),
    /// `ValueError`: more than `MAX_TEXT_CHUNK` of output.
    TooLarge,
    /// `zlib.error`.
    Corrupt,
}

/// `_safe_zlib_decompress`: `decompressobj().decompress(s, MAX_TEXT_CHUNK)`. The partial output
/// of a truncated stream counts, as in Python; input left over once the limit is reached is the
/// `ValueError`.
fn safe_zlib_decompress(input: &[u8]) -> Inflate {
    let mut d = Decompress::new(true);
    let mut out = vec![0u8; MAX_TEXT_CHUNK];
    match d.decompress(input, &mut out, FlushDecompress::None) {
        Err(_) => Inflate::Corrupt,
        Ok(status) => {
            out.truncate(d.total_out() as usize);
            let tail = (d.total_in() as usize) < input.len();
            if status != Status::StreamEnd && tail && out.len() == MAX_TEXT_CHUNK {
                Inflate::TooLarge
            } else {
                Inflate::Done(out)
            }
        }
    }
}

fn crc_ok(cid: &[u8], body: &[u8], crc: &[u8]) -> bool {
    let mut h = crc32fast::Hasher::new();
    h.update(cid);
    h.update(body);
    crc.len() == 4 && h.finalize().to_be_bytes() == *crc
}

fn is_cid(cid: &[u8]) -> bool {
    cid.len() == 4 && cid.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

/// The PNG `mode` for `(bit depth, colour type)`, as `_MODES`.
fn png_mode(depth: u8, color: u8) -> Option<&'static str> {
    Some(match (depth, color) {
        (1, 0) => "1",
        (2 | 4 | 8, 0) => "L",
        (16, 0) => "I;16",
        (8 | 16, 2) => "RGB",
        (1 | 2 | 4 | 8, 3) => "P",
        (8 | 16, 4) => "LA",
        (8 | 16, 6) => "RGBA",
        _ => return None,
    })
}

struct PngState {
    info: Info,
    mode: &'static str,
    text_memory: usize,
    seq_num: Option<u32>,
    size: (u32, u32),
    n_frames: Option<u32>,
    /// `"bbox" in im_info`: an `fcTL` came before the pixel data.
    bbox: bool,
}

enum Flow {
    Next,
    /// `EOFError`: IDAT, fdAT or IEND.
    Stop,
}

impl PngState {
    fn check_text_memory(&mut self, chars: usize) -> Option<()> {
        self.text_memory += chars;
        (self.text_memory <= MAX_TEXT_MEMORY).then_some(())
    }

    /// `PngStream.chunk_<cid>`. `None` is an exception that makes `Image.open` fail.
    fn call(&mut self, cid: &[u8], s: &[u8]) -> Option<Flow> {
        match cid {
            b"IDAT" | b"fdAT" | b"IEND" => return Some(Flow::Stop),
            b"IHDR" => {
                if s.len() < 13 {
                    return None;
                }
                self.size = (be32(s, 0)?, be32(s, 4)?);
                if let Some(m) = png_mode(s[8], s[9]) {
                    self.mode = m;
                }
                if s[12] != 0 {
                    self.info.set("interlace", Val::Other);
                }
                if s[11] != 0 {
                    return None;
                }
            }
            b"iCCP" => {
                let i = s.iter().position(|&c| c == 0).map_or(-1, |p| p as i64);
                let comp = *s.get((i + 1) as usize)?;
                if comp != 0 {
                    return None;
                }
                let val = match safe_zlib_decompress(s.get((i + 2) as usize..)?) {
                    Inflate::Done(b) => Val::Bytes(b),
                    Inflate::TooLarge => return None,
                    Inflate::Corrupt => Val::Other,
                };
                self.info.set("icc_profile", val);
            }
            b"tRNS" => match self.mode {
                "1" | "L" | "I;16" => {
                    be16(s, 0)?;
                }
                "RGB" => {
                    be16(s, 0)?;
                    be16(s, 2)?;
                    be16(s, 4)?;
                }
                "P" => self.info.set("transparency", Val::Other),
                _ => {}
            },
            b"gAMA" => {
                be32(s, 0)?;
            }
            b"cHRM" => {
                if s.len() % 4 != 0 {
                    return None;
                }
            }
            b"sRGB" => {
                if s.is_empty() {
                    return None;
                }
            }
            b"pHYs" => {
                if s.len() < 9 {
                    return None;
                }
            }
            b"tEXt" => {
                let (k, v) = match s.iter().position(|&c| c == 0) {
                    Some(p) => (&s[..p], &s[p + 1..]),
                    None => (s, &s[..0]),
                };
                if !k.is_empty() {
                    let v_str = latin1(v);
                    let key = latin1(k);
                    let val = if k == b"exif" {
                        Val::Bytes(v.to_vec())
                    } else {
                        Val::Str(v_str.clone())
                    };
                    self.info.set(&key, val);
                    self.check_text_memory(v_str.chars().count())?;
                }
            }
            b"zTXt" => {
                let (k, v) = match s.iter().position(|&c| c == 0) {
                    Some(p) => (&s[..p], &s[p + 1..]),
                    None => (s, &s[..0]),
                };
                let comp = v.first().copied().unwrap_or(0);
                if comp != 0 {
                    return None;
                }
                let body = match safe_zlib_decompress(v.get(1..).unwrap_or(&[])) {
                    Inflate::Done(b) => b,
                    Inflate::TooLarge => return None,
                    Inflate::Corrupt => Vec::new(),
                };
                if !k.is_empty() {
                    let v_str = latin1(&body);
                    self.info.set(&latin1(k), Val::Str(v_str.clone()));
                    self.check_text_memory(v_str.chars().count())?;
                }
            }
            b"iTXt" => {
                let Some(p) = s.iter().position(|&c| c == 0) else {
                    return Some(Flow::Next);
                };
                let (k, r) = (&s[..p], &s[p + 1..]);
                if r.len() < 2 {
                    return Some(Flow::Next);
                }
                let (cf, cm, r) = (r[0], r[1], &r[2..]);
                let mut parts = r.splitn(3, |&c| c == 0);
                let (Some(lang), Some(tk), Some(v)) = (parts.next(), parts.next(), parts.next())
                else {
                    return Some(Flow::Next);
                };
                let mut v = v.to_vec();
                if cf != 0 {
                    if cm != 0 {
                        return Some(Flow::Next);
                    }
                    match safe_zlib_decompress(&v) {
                        Inflate::Done(b) => v = b,
                        Inflate::TooLarge => return None,
                        Inflate::Corrupt => return Some(Flow::Next),
                    }
                }
                if k == b"XML:com.adobe.xmp" {
                    self.info.set("xmp", Val::Bytes(v.clone()));
                }
                let (Ok(lang), Ok(tk), Ok(v_str)) = (
                    std::str::from_utf8(lang),
                    std::str::from_utf8(tk),
                    std::str::from_utf8(&v),
                ) else {
                    return Some(Flow::Next);
                };
                let _ = (lang, tk);
                self.info.set(&latin1(k), Val::Str(v_str.to_string()));
                self.check_text_memory(v_str.chars().count())?;
            }
            b"eXIf" => {
                let mut v = b"Exif\0\0".to_vec();
                v.extend_from_slice(s);
                self.info.set("exif", Val::Bytes(v));
            }
            b"acTL" => {
                if s.len() < 8 {
                    return None;
                }
                if self.n_frames.is_none() {
                    let n = be32(s, 0)?;
                    if n != 0 && n <= 0x8000_0000 {
                        self.n_frames = Some(n);
                        self.info.set("loop", Val::Other);
                    }
                } else {
                    self.n_frames = None;
                }
            }
            b"fcTL" => {
                if s.len() < 26 {
                    return None;
                }
                let seq = be32(s, 0)?;
                let bad_seq = match self.seq_num {
                    None => seq != 0,
                    Some(prev) => prev != seq.wrapping_sub(1),
                };
                if bad_seq {
                    return None;
                }
                self.seq_num = Some(seq);
                let (w, h, px, py) = (be32(s, 4)?, be32(s, 8)?, be32(s, 12)?, be32(s, 16)?);
                if u64::from(px) + u64::from(w) > u64::from(self.size.0)
                    || u64::from(py) + u64::from(h) > u64::from(self.size.1)
                {
                    return None;
                }
                self.bbox = true;
            }
            _ => {}
        }
        Some(Flow::Next)
    }
}

/// `(info, exif bytes)` of a PNG, or `None` if Pillow cannot open it.
fn png(data: &[u8]) -> Option<(Info, Option<Vec<u8>>)> {
    let mut st = PngState {
        info: Info::default(),
        mode: "",
        text_memory: 0,
        seq_num: None,
        size: (0, 0),
        n_frames: None,
        bbox: false,
    };
    let mut pos = 8;
    // `PngImageFile._open`: chunks up to the first IDAT, each with its checksum verified.
    let after_first_idat = loop {
        let header = data.get(pos..pos + 8)?;
        let cid = &header[4..];
        if !is_cid(cid) {
            return None;
        }
        let length = be32(header, 0)? as usize;
        let body_at = pos + 8;
        if matches!(cid, b"IDAT" | b"fdAT" | b"IEND") {
            break body_at;
        }
        let body = data.get(body_at..body_at.checked_add(length)?)?;
        if st.call(cid, body)?.is_stop() {
            break body_at;
        }
        let crc = data.get(body_at + length..body_at + length + 4)?;
        if !crc_ok(cid, body, crc) {
            return None;
        }
        pos = body_at + length + 4;
    };
    if st.mode.is_empty() || st.size.0 == 0 || st.size.1 == 0 {
        return None;
    }

    let texts_info = st.info.clone();
    // `PngImageFile.getexif`: no "exif" in `info` makes Pillow `load()` the picture, which reads
    // the chunks after the pixel data into the same `info`. A picture that does not decode
    // never gets there; the exception is swallowed by `check_upscaler_metadata`.
    if st.info.get("exif").is_none() && crate::pyraster::open(data, None).is_ok() {
        trailing_chunks(data, after_first_idat - 8, &mut st);
    }
    let exif = match st.info.get("exif") {
        Some(Val::Bytes(b)) => Some(b.clone()),
        Some(_) => None,
        None => match st.info.get("Raw profile type exif") {
            Some(Val::Str(raw)) => {
                let joined: String = raw.split('\n').skip(3).collect();
                match from_hex(&joined) {
                    Some(b) => Some(b),
                    None => return Some((texts_info, None)),
                }
            }
            _ => None,
        },
    };
    Some((texts_info, exif))
}

/// `PngImageFile.load_end`: chunks after the pixel data, no checksum check, until IEND or the
/// first thing that does not parse.
fn trailing_chunks(data: &[u8], first_idat: usize, st: &mut PngState) {
    let mut pos = first_idat;
    // Skip the IDAT run: `load_read` consumed it.
    loop {
        let (Some(header), cid) = (data.get(pos..pos + 8), data.get(pos + 4..pos + 8)) else {
            return;
        };
        let Some(length) = be32(header, 0) else {
            return;
        };
        if cid != Some(b"IDAT") {
            break;
        }
        pos += 8 + length as usize + 4;
    }
    loop {
        let Some(header) = data.get(pos..pos + 8) else {
            return;
        };
        let cid = &header[4..];
        if !is_cid(cid) {
            return;
        }
        let Some(length) = be32(header, 0).map(|l| l as usize) else {
            return;
        };
        if cid == b"IEND" {
            return;
        }
        let body_at = pos + 8;
        let Some(body) = data.get(body_at..body_at.saturating_add(length)) else {
            return;
        };
        match st.call(cid, body) {
            Some(_) => {}
            None => return,
        }
        pos = body_at + length + 4;
    }
}

impl Flow {
    fn is_stop(&self) -> bool {
        matches!(self, Flow::Stop)
    }
}

/// `bytes.fromhex`: ASCII whitespace between pairs is skipped; anything else is an error.
fn from_hex(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut it = s.bytes().peekable();
    while let Some(&c) = it.peek() {
        if matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            it.next();
            continue;
        }
        let hi = (it.next()? as char).to_digit(16)?;
        let lo = (it.next()? as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------------
// JPEG

fn jpeg(data: &[u8]) -> Option<(Info, Option<Vec<u8>>)> {
    let mut info = Info::default();
    let mut icclist: Vec<Vec<u8>> = Vec::new();
    let mut exif: Option<Vec<u8>> = None;
    let mut size_seen = false;
    let mut pos = 3;
    let mut s: Vec<u8> = vec![0xff];
    loop {
        let first = *s.first()?;
        let marker: u16 = if first == 0xff {
            let next = data.get(pos).copied();
            if let Some(n) = next {
                pos += 1;
                s.push(n);
            }
            if s.len() < 2 {
                return None;
            }
            u16::from_be_bytes([s[0], s[1]])
        } else {
            s = data.get(pos..pos + 1).map_or_else(Vec::new, <[u8]>::to_vec);
            pos += usize::from(!s.is_empty());
            continue;
        };
        let handler = match marker {
            0xffc0..=0xffc3 | 0xffc5..=0xffc7 | 0xffc9..=0xffcb | 0xffcd..=0xffcf | 0xffde => {
                Some("SOF")
            }
            0xffc4 | 0xffcc | 0xffda | 0xffdc | 0xffdd | 0xffdf => Some("SKIP"),
            0xffdb => Some("DQT"),
            0xffe0..=0xffef => Some("APP"),
            0xfffe => Some("COM"),
            0xffc8 | 0xffd0..=0xffd9 | 0xfff0..=0xfffd => Some(""),
            _ => None,
        };
        match handler {
            Some(h) => {
                if !h.is_empty() {
                    let n = i64::from(be16(data, pos)?) - 2;
                    pos += 2;
                    let seg = if n <= 0 {
                        &data[..0]
                    } else {
                        let seg = data.get(pos..pos + n as usize)?;
                        pos += n as usize;
                        seg
                    };
                    match h {
                        "APP" => jpeg_app(marker, seg, &mut info, &mut icclist, &mut exif),
                        "COM" => info.set("comment", Val::Bytes(seg.to_vec())),
                        "SOF" => {
                            if seg.len() < 6 || seg[0] != 8 || ![1, 3, 4].contains(&seg[5]) {
                                return None;
                            }
                            size_seen = true;
                            if !icclist.is_empty() {
                                icclist.sort();
                                let list = std::mem::take(&mut icclist);
                                let val = match list[0].get(13) {
                                    Some(&n) if usize::from(n) == list.len() => Val::Bytes(
                                        list.iter()
                                            .flat_map(|p| {
                                                p.get(14..).unwrap_or(&[]).iter().copied()
                                            })
                                            .collect(),
                                    ),
                                    _ => Val::Other,
                                };
                                info.set("icc_profile", val);
                            }
                        }
                        "DQT" => {
                            let mut rest = seg;
                            while let Some(&v) = rest.first() {
                                let len = 1 + if v / 16 == 0 { 64 } else { 128 };
                                if rest.len() < len {
                                    return None;
                                }
                                rest = &rest[len..];
                            }
                        }
                        _ => {}
                    }
                }
                if marker == 0xffda {
                    break;
                }
                s = data.get(pos..pos + 1).map_or_else(Vec::new, <[u8]>::to_vec);
                pos += usize::from(!s.is_empty());
            }
            None if marker == 0 || marker == 0xffff => s = vec![0xff],
            None if marker == 0xff00 => {
                s = data.get(pos..pos + 1).map_or_else(Vec::new, <[u8]>::to_vec);
                pos += usize::from(!s.is_empty());
            }
            None => return None,
        }
    }
    if !size_seen {
        return None;
    }
    if let Some(e) = &exif {
        info.set("exif", Val::Bytes(e.clone()));
    }
    Some((info, exif))
}

/// What `jpeg_factory` looks for in an `APP1` to tell an Ultra HDR image from an MPO.
const HDRGM: &[u8] = b" hdrgm:Version=\"";

fn jpeg_app(
    marker: u16,
    s: &[u8],
    info: &mut Info,
    icclist: &mut Vec<Vec<u8>>,
    exif: &mut Option<Vec<u8>>,
) {
    if marker == 0xffe1 && s.windows(HDRGM.len()).any(|w| w == HDRGM) {
        // Not an `info` key of Pillow's: `jpeg_factory` looks for this in `applist`.
        info.set("hdrgm", Val::Other);
    }
    if marker == 0xffe0 && s.starts_with(b"JFIF") {
        info.set("jfif", Val::Other);
    } else if marker == 0xffe1 && s.starts_with(b"Exif\0\0") {
        match exif {
            Some(e) => e.extend_from_slice(&s[6..]),
            None => *exif = Some(s.to_vec()),
        }
        if let Some(e) = exif {
            info.set("exif", Val::Bytes(e.clone()));
        }
    } else if marker == 0xffe1 && s.starts_with(b"http://ns.adobe.com/xap/1.0/\0") {
        let body = s
            .iter()
            .position(|&c| c == 0)
            .map_or(&s[..0], |p| &s[p + 1..]);
        info.set("xmp", Val::Bytes(body.to_vec()));
    } else if marker == 0xffe2 && s.starts_with(b"FPXR\0") {
        info.set("flashpix", Val::Bytes(s.to_vec()));
    } else if marker == 0xffe2 && s.starts_with(b"ICC_PROFILE\0") {
        icclist.push(s.to_vec());
    } else if marker == 0xffe2 && s.starts_with(b"MPF\0") {
        info.set("mp", Val::Bytes(s[4..].to_vec()));
    } else if marker == 0xffed && s.starts_with(b"Photoshop 3.0\0") {
        info.set("photoshop", Val::Other);
    } else if marker == 0xffee && s.starts_with(b"Adobe") {
        info.set("adobe", Val::Other);
    }
}

// ---------------------------------------------------------------------------------------------
// GIF

fn gif(data: &[u8]) -> Option<Info> {
    let mut info = Info::default();
    info.set("version", Val::Bytes(data.get(..6)?.to_vec()));
    let flags = *data.get(10)?;
    let mut pos = 13;
    if flags & 128 != 0 {
        pos += 3 << ((flags & 7) + 1);
    }
    // `data()`: one length-prefixed block; `None` at the terminator or at the end of the file.
    let block = |pos: &mut usize| -> Option<Vec<u8>> {
        let len = *data.get(*pos)?;
        *pos += 1;
        if len == 0 {
            return None;
        }
        let end = (*pos + usize::from(len)).min(data.len());
        let b = data[*pos..end].to_vec();
        *pos = end;
        Some(b)
    };
    let mut frame_comment: Option<Vec<u8>> = None;
    let mut s: Option<u8> = data.get(pos).copied();
    pos += 1;
    let mut found_image = false;
    while let Some(sv) = s {
        if sv == b';' {
            break;
        }
        if sv == b'!' {
            let label = *data.get(pos)?;
            pos += 1;
            let mut blk = block(&mut pos);
            if label == 254 {
                let mut comment = Vec::new();
                while let Some(b) = blk {
                    comment.extend_from_slice(&b);
                    blk = block(&mut pos);
                }
                match &mut frame_comment {
                    Some(c) => {
                        c.push(b'\n');
                        c.extend_from_slice(&comment);
                    }
                    None => frame_comment = Some(comment),
                }
                s = data.get(pos).copied();
                pos += 1;
                continue;
            }
            if label == 255 {
                if let Some(b) = &blk {
                    if b.starts_with(b"NETSCAPE2.0") {
                        let _ = block(&mut pos);
                    }
                }
            }
            while block(&mut pos).is_some() {}
        } else if sv == b',' {
            found_image = true;
            break;
        }
        s = data.get(pos).copied();
        pos += 1;
    }
    if !found_image {
        return None;
    }
    if let Some(c) = frame_comment.filter(|c| !c.is_empty()) {
        info.set("comment", Val::Bytes(c));
    }
    Some(info)
}

// ---------------------------------------------------------------------------------------------
// WebP

fn webp(data: &[u8]) -> Option<(Info, Option<Vec<u8>>)> {
    let mut info = Info::default();
    let mut flags: Option<u8> = None;
    let mut found = [None::<Vec<u8>>, None, None];
    let mut pos = 12;
    while pos + 8 <= data.len() {
        let fourcc = &data[pos..pos + 4];
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = data.get(pos + 8..(pos + 8).saturating_add(size).min(data.len()))?;
        match fourcc {
            b"VP8X" if flags.is_none() => flags = body.first().copied(),
            b"ICCP" if flags.is_some_and(|f| f & 0x20 != 0) && found[0].is_none() => {
                found[0] = Some(body.to_vec())
            }
            b"EXIF" if flags.is_some_and(|f| f & 0x08 != 0) && found[1].is_none() => {
                found[1] = Some(body.to_vec())
            }
            b"XMP " if flags.is_some_and(|f| f & 0x04 != 0) && found[2].is_none() => {
                found[2] = Some(body.to_vec())
            }
            _ => {}
        }
        pos += 8 + size + (size & 1);
    }
    for (key, chunk) in ["icc_profile", "exif", "xmp"].into_iter().zip(&found) {
        if let Some(v) = chunk.as_ref().filter(|v| !v.is_empty()) {
            info.set(key, Val::Bytes(v.clone()));
        }
    }
    let exif = found[1].clone().filter(|v| !v.is_empty());
    Some((info, exif))
}

// ---------------------------------------------------------------------------------------------
// EXIF

/// Tags of IFD0 of a TIFF block (an EXIF payload or a whole TIFF file), as
/// `ImageFileDirectory_v2.load` reads them: `(tag, type, raw value)`, the last of a repeated tag
/// winning. A malformed block yields what Pillow had read when it stopped (nothing for a bad
/// header and for BigTIFF, which Pillow's EXIF reader cannot take from an 8-byte head).
fn ifd0(exif: &[u8]) -> Vec<(u16, u16, Vec<u8>)> {
    let mut data = exif;
    while data.starts_with(b"Exif\0\0") {
        data = &data[6..];
    }
    let head = &data[..data.len().min(8)];
    let accepted = head.len() >= 4
        && matches!(
            &head[..4],
            b"MM\x00\x2a"
                | b"II\x2a\x00"
                | b"MM\x2a\x00"
                | b"II\x00\x2a"
                | b"MM\x00\x2b"
                | b"II\x2b\x00"
        );
    if !accepted || head.len() < 8 || head[2] == 43 {
        return Vec::new();
    }
    let little = &head[..2] == b"II";
    let u16_at = |b: &[u8]| -> u16 {
        let a: [u8; 2] = b[..2].try_into().expect("two bytes");
        if little {
            u16::from_le_bytes(a)
        } else {
            u16::from_be_bytes(a)
        }
    };
    let u32_at = |b: &[u8]| -> u32 {
        let a: [u8; 4] = b[..4].try_into().expect("four bytes");
        if little {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        }
    };
    let mut tags: Vec<(u16, u16, Vec<u8>)> = Vec::new();
    let mut pos = u32_at(&head[4..]) as usize;
    let Some(count_bytes) = data.get(pos..pos + 2) else {
        return tags;
    };
    let count = u16_at(count_bytes);
    pos += 2;
    for _ in 0..count {
        let Some(entry) = data.get(pos..pos + 12) else {
            break;
        };
        pos += 12;
        let (tag, typ, n, inline) = (
            u16_at(entry),
            u16_at(&entry[2..]),
            u32_at(&entry[4..]),
            &entry[8..12],
        );
        let unit: u64 = match typ {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 | 13 => 4,
            5 | 10 | 12 | 16 | 17 | 18 => 8,
            _ => continue,
        };
        let size = u64::from(n) * unit;
        let value: Vec<u8> = if size > 4 {
            let offset = u32_at(inline) as usize;
            match data.get(offset..offset.saturating_add(size as usize)) {
                Some(v) => v.to_vec(),
                None => break,
            }
        } else {
            inline[..size as usize].to_vec()
        };
        if value.is_empty() {
            continue;
        }
        tags.retain(|(t, _, _)| *t != tag);
        tags.push((tag, typ, value));
    }
    tags
}

/// `str(value)` for the tags in `check_upscaler_metadata`'s list, from `Image.getexif()`: the
/// ASCII, BYTE and UNDEFINED values of IFD0 tags 270, 305 and 315 that are not empty. Numbers
/// cannot hold a tool name, so they are not rendered.
fn exif_texts(exif: &[u8]) -> Vec<String> {
    let tags = ifd0(exif);
    let mut out = Vec::new();
    for want in [270u16, 305, 315] {
        let Some((_, typ, v)) = tags.iter().find(|(t, _, _)| *t == want) else {
            continue;
        };
        match typ {
            2 => {
                let v = v.strip_suffix(&[0]).unwrap_or(v);
                let s = latin1(v);
                if !s.is_empty() {
                    out.push(s);
                }
            }
            1 | 7 => out.push(bytes_repr(v)),
            _ => {}
        }
    }
    out
}

/// A TIFF file: `info` gets the ICC profile (tag 34675) and the XMP packet (tag 700); the EXIF
/// reader is pointed at IFD0 of the file itself.
fn tiff(data: &[u8]) -> Option<(Info, Option<Vec<u8>>)> {
    let mut info = Info::default();
    let tags = ifd0(data);
    for (key, want) in [("icc_profile", 34675u16), ("xmp", 700)] {
        if let Some((_, typ, v)) = tags.iter().find(|(t, _, _)| *t == want) {
            if matches!(typ, 1 | 7) {
                info.set(key, Val::Bytes(v.clone()));
            }
        }
    }
    Some((info, Some(data.to_vec())))
}

/// Python's `repr(bytes)`.
fn bytes_repr(b: &[u8]) -> String {
    let quote = if b.contains(&b'\'') && !b.contains(&b'"') {
        '"'
    } else {
        '\''
    };
    let mut s = String::from("b");
    s.push(quote);
    for &c in b {
        match c {
            b'\\' => s.push_str("\\\\"),
            b'\t' => s.push_str("\\t"),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            c if char::from(c) == quote => {
                s.push('\\');
                s.push(quote);
            }
            0x20..=0x7e => s.push(char::from(c)),
            c => s.push_str(&format!("\\x{c:02x}")),
        }
    }
    s.push(quote);
    s
}

// ---------------------------------------------------------------------------------------------
// Frame count

/// `getattr(Image.open(io.BytesIO(data)), "n_frames", 1)` for the formats whose signature
/// `google_lens.media_type` hands on to `_is_animated` (JPEG, PNG, WebP; a BMP has one frame).
/// `None` is `Image.open` or `n_frames` raising, which `_is_animated` reads as "not animated".
pub(crate) fn frame_count(data: &[u8]) -> Option<u64> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        png_frame_count(data)
    } else if data.starts_with(b"\xff\xd8\xff") {
        jpeg_frame_count(data)
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        webp_frame_count(data)
    } else {
        Some(1)
    }
}

/// `PngImageFile._open`: `n_frames` from `acTL`, one more when the `IDAT` holds a default image
/// that is not the first animation frame.
fn png_frame_count(data: &[u8]) -> Option<u64> {
    let mut st = PngState {
        info: Info::default(),
        mode: "",
        text_memory: 0,
        seq_num: None,
        size: (0, 0),
        n_frames: None,
        bbox: false,
    };
    let mut pos = 8;
    loop {
        let header = data.get(pos..pos + 8)?;
        let cid = &header[4..];
        if !is_cid(cid) {
            return None;
        }
        let length = be32(header, 0)? as usize;
        let body_at = pos + 8;
        if matches!(cid, b"IDAT" | b"fdAT" | b"IEND") {
            break;
        }
        let body = data.get(body_at..body_at.checked_add(length)?)?;
        st.call(cid, body)?;
        let crc = data.get(body_at + length..body_at + length + 4)?;
        if !crc_ok(cid, body, crc) {
            return None;
        }
        pos = body_at + length + 4;
    }
    if st.mode.is_empty() || st.size.0 == 0 || st.size.1 == 0 {
        return None;
    }
    // `chunk_IDAT` (also called by `chunk_fdAT`) marks the default image when an `acTL` was seen
    // and no `fcTL` put a box on the frame.
    Some(match st.n_frames {
        Some(n) if !st.bbox => u64::from(n) + 1,
        Some(n) => u64::from(n),
        None => 1,
    })
}

/// `jpeg_factory`: an `APP2` MPF index naming more than one image makes the file an MPO, whose
/// `n_frames` is that number. Anything the index gets wrong makes it a plain JPEG (one frame), or
/// raises; both read as not animated.
fn jpeg_frame_count(data: &[u8]) -> Option<u64> {
    let (info, _) = jpeg(data)?;
    let Some(Val::Bytes(mp)) = info.get("mp") else {
        return Some(1);
    };
    let big = mp.starts_with(b"MM\x00\x2a");
    if !big && !mp.starts_with(b"II\x2a\x00") {
        return Some(1);
    }
    let u16_at = |at: usize| -> Option<u16> {
        let b: [u8; 2] = mp.get(at..at + 2)?.try_into().ok()?;
        Some(if big {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let b: [u8; 4] = mp.get(at..at + 4)?.try_into().ok()?;
        Some(if big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    };
    let ifd = u32_at(4)? as usize;
    let entries = usize::from(u16_at(ifd)?);
    let (mut quant, mut raw) = (None, None);
    for i in 0..entries {
        let at = ifd + 2 + 12 * i;
        let (tag, kind, count) = (u16_at(at)?, u16_at(at + 2)?, u32_at(at + 4)? as usize);
        match (tag, kind) {
            (0xB001, 3 | 4) if count == 1 => {
                quant = Some(if kind == 3 {
                    u64::from(u16_at(at + 8)?)
                } else {
                    u64::from(u32_at(at + 8)?)
                });
            }
            (0xB002, 7) => {
                let start = if count > 4 {
                    u32_at(at + 8)? as usize
                } else {
                    at + 8
                };
                raw = Some(mp.get(start..start.checked_add(count)?)?);
            }
            _ => {}
        }
    }
    let (quant, raw) = (quant?, raw?);
    if quant <= 1 {
        return Some(1);
    }
    for n in 0..usize::try_from(quant).ok()? {
        let attribute = {
            let b: [u8; 4] = raw.get(n * 16..n * 16 + 4)?.try_into().ok()?;
            if big {
                u32::from_be_bytes(b)
            } else {
                u32::from_le_bytes(b)
            }
        };
        raw.get(n * 16..n * 16 + 16)?;
        if (attribute >> 24) & 7 != 0 {
            return Some(1);
        }
    }
    if info.get("hdrgm").is_some() {
        return Some(1);
    }
    Some(quant)
}

/// libwebp's demuxer, as `WebPAnimDecoder` uses it: the frames of an animation, `1` for a still
/// picture. A file it refuses is `None`.
fn webp_frame_count(data: &[u8]) -> Option<u64> {
    let le32 = |at: usize| -> Option<usize> {
        Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?) as usize)
    };
    let riff_end = le32(4)?.checked_add(8)?;
    if riff_end < 12 + 8 || data.len() < riff_end {
        return None;
    }
    let data = &data[..riff_end];
    let first = data.get(12..16)?;
    if first != b"VP8X" {
        // A still picture of the simple kind: one frame, or the decoder refuses it, and either
        // way `_is_animated` answers no.
        return Some(1);
    }
    if le32(16)? != 10 {
        return None;
    }
    let flags = *data.get(20)?;
    let animated = flags & 0x02 != 0;
    let canvas = (
        1 + usize::from_le_bytes([data[24], data[25], data[26], 0, 0, 0, 0, 0]),
        1 + usize::from_le_bytes([data[27], data[28], data[29], 0, 0, 0, 0, 0]),
    );
    if canvas.0.checked_mul(canvas.1)? >= 1 << 32 {
        return None;
    }
    if !animated {
        return Some(1);
    }
    let (mut pos, mut frames, mut anim) = (30usize, 0u64, false);
    while pos + 8 <= data.len() {
        let size = le32(pos + 4)?;
        let body = data.get(pos + 8..(pos + 8).checked_add(size)?)?;
        match &data[pos..pos + 4] {
            b"ANIM" => {
                if size < 6 {
                    return None;
                }
                anim = true;
            }
            b"ANMF" => {
                if !anim || size < 16 {
                    return None;
                }
                let u24 = |at: usize| {
                    usize::from_le_bytes([body[at], body[at + 1], body[at + 2], 0, 0, 0, 0, 0])
                };
                let (x, y) = (2 * u24(0), 2 * u24(3));
                let (w, h) = (1 + u24(6), 1 + u24(9));
                if x + w > canvas.0 || y + h > canvas.1 {
                    return None;
                }
                // The frame's own picture: a `VP8 `/`VP8L` chunk (with an optional `ALPH`).
                let mut at = 16;
                let mut seen = false;
                while at + 8 <= body.len() {
                    let sub = u32::from_le_bytes(body[at + 4..at + 8].try_into().ok()?) as usize;
                    let payload = body.get(at + 8..at + 8 + sub)?;
                    if matches!(&body[at..at + 4], b"VP8 " | b"VP8L") {
                        let mut still = b"RIFF\0\0\0\0WEBP".to_vec();
                        still.extend_from_slice(&body[at..at + 8]);
                        still.extend_from_slice(payload);
                        // The RIFF size, which the still-picture check reads (`WebPGetInfo`).
                        let riff = u32::try_from(still.len() - 8).ok()?;
                        still[4..8].copy_from_slice(&riff.to_le_bytes());
                        let (iw, ih) = crate::pyimage::image_size(&still)?;
                        if (iw as usize, ih as usize) != (w, h) {
                            return None;
                        }
                        seen = true;
                        break;
                    }
                    at += 8 + sub + (sub & 1);
                }
                if !seen {
                    return None;
                }
                frames += 1;
            }
            _ => {}
        }
        pos += 8 + size + (size & 1);
    }
    if frames == 0 {
        return None;
    }
    Some(frames)
}
