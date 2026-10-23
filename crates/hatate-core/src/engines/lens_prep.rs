//! What Google Lens is given to upload (`core/google_lens.py`: `media_type`, `LENS_READS`,
//! `prepare_for_lens`, `_is_animated`, `_to_jpeg`, `upload_script`).
//!
//! The picture goes through `image_prep::prepare_upload_bytes` first (the 2 MB budget Lens is
//! carried into the page under), then the format check: a JPEG, PNG, WebP or BMP that is not
//! animated goes as it is; anything else Lens cannot read (GIF, AVIF, TIFF, an animation) is
//! re-encoded to JPEG, and what cannot be re-encoded (a video, JPEG XL, a text file) is refused
//! with a reason instead of being sent and silently rejected at the far end. A refusal is an
//! ordinary failed image: it does not count towards, or set, the stand-down latch (DAN-2063 F2).
//!
//! **What is matched with Python.** The decision, the filename, the media type, the dimensions
//! and the flattened pixels of a re-encoded picture. **What is not:** the bytes of that JPEG,
//! which are the `jpeg-encoder` crate's (`image_prep::encode_rgb_jpeg`; the same grant as
//! `image_prep`'s other re-encodes, DAN-2057), and the formats Pillow decodes that this build
//! cannot (see `docs/google-lens.md`).

use std::io;
use std::path::Path;

use bytes::Bytes;

use super::google_images::SEARCH_BY_IMAGE_URL;
use crate::config::pyjson;
use crate::error::ErrorKind;
use crate::image_prep::{
    DEFAULT_MAX_DIMENSION, basename, encode_rgb_jpeg, os_error_text, prepare_upload_bytes,
};
use crate::pyraster::{self, Mode};

/// `MAX_UPLOAD_BYTES`: the image is carried into the page as a base64 string inside a script, so
/// it is re-encoded smaller than the other engines' 7 MB budget.
pub const MAX_UPLOAD_BYTES: u64 = 2 * 1024 * 1024;

/// `LENS_READS`: what Lens actually reads. GIF is not here, despite being a picture format Google
/// serves everywhere else.
pub const LENS_READS: [&str; 4] = ["image/jpeg", "image/png", "image/webp", "image/bmp"];

/// The quality `_to_jpeg` saves at.
const JPEG_QUALITY: u8 = 90;

/// `media_type`: the media type of these bytes, or `None` if they are not a picture in a format
/// Google takes. Read from the magic bytes, because the filename cannot be trusted to describe
/// the content.
pub fn media_type(image_bytes: &[u8]) -> Option<&'static str> {
    let head = &image_bytes[..image_bytes.len().min(16)];
    const MAGIC: [(&[u8], &str); 4] = [
        (b"\xff\xd8\xff", "image/jpeg"),
        (b"\x89PNG\r\n\x1a\n", "image/png"),
        (b"GIF8", "image/gif"),
        (b"BM", "image/bmp"),
    ];
    for (magic, mime) in MAGIC {
        if head.starts_with(magic) {
            return Some(mime);
        }
    }
    if head.starts_with(b"RIFF") && image_bytes.get(8..12) == Some(b"WEBP") {
        return Some("image/webp");
    }
    None
}

/// What `prepare_for_lens` hands back: `(bytes, filename, media type)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LensUpload {
    pub data: Bytes,
    pub filename: String,
    pub media_type: &'static str,
}

/// Why a picture could not be prepared.
#[derive(Debug)]
pub enum PrepareError {
    /// `OSError` out of `prepare_upload_bytes`: the file cannot be read.
    Unreadable { error: io::Error, path: String },
    /// `GoogleLensError`: not a format Lens accepts and not convertible to one.
    Refused(String),
}

impl PrepareError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            PrepareError::Unreadable { .. } => ErrorKind::ImageUnreadable,
            PrepareError::Refused(_) => ErrorKind::FormatRefused,
        }
    }

    /// What failed and what to do: no path, no OS text.
    pub fn user_message(&self) -> String {
        match self {
            PrepareError::Unreadable { .. } => {
                "The image could not be read. Check that the file still exists and can be opened."
            }
            PrepareError::Refused(_) => {
                "Google Lens cannot take this file, and it could not be converted to a picture it \
                 can. Video and JPEG XL are the usual reasons; the other engines can still \
                 search it."
            }
        }
        .to_owned()
    }

    /// The raw text: Python's `str(OSError)` or the refusal sentence.
    pub fn detail(&self) -> String {
        match self {
            PrepareError::Unreadable { error, path } => os_error_text(error, Path::new(path)),
            PrepareError::Refused(message) => message.clone(),
        }
    }
}

/// `prepare_for_lens`: an upload Google will accept. Blocking (decoding and encoding a large
/// picture takes seconds); async callers use `spawn_blocking`.
pub fn prepare_for_lens(image_path: &Path) -> Result<LensUpload, PrepareError> {
    let (data, filename) =
        prepare_upload_bytes(image_path, MAX_UPLOAD_BYTES, DEFAULT_MAX_DIMENSION).map_err(
            |error| PrepareError::Unreadable {
                error,
                path: image_path.to_string_lossy().into_owned(),
            },
        )?;
    let mime = media_type(&data);
    if let Some(m) = mime.filter(|m| LENS_READS.contains(m))
        && !is_animated(&data)
    {
        return Ok(LensUpload {
            data,
            filename,
            media_type: m,
        });
    }

    let Some(converted) = to_jpeg(&data) else {
        if let Some(m) = mime {
            // It IS a picture Lens could read, just not one it can be given: an animation whose
            // first frame would not decode.
            return Ok(LensUpload {
                data,
                filename,
                media_type: m,
            });
        }
        return Err(PrepareError::Refused(format!(
            "{} is not in a format Google Lens accepts, and it could not be converted to one. \
             Video and JPEG XL are the usual reasons; the other engines can still search it.",
            basename(image_path)
        )));
    };
    tracing::info!(
        "Converted {} to JPEG for Google Lens ({} -> {} bytes)",
        basename(image_path),
        data.len(),
        converted.len()
    );
    Ok(LensUpload {
        data: Bytes::from(converted),
        filename: format!("{}.jpg", splitext_stem(&filename)),
        media_type: "image/jpeg",
    })
}

/// `os.path.splitext(name)[0]`.
fn splitext_stem(name: &str) -> &str {
    if let Some(dot) = name.rfind('.')
        && name[..dot].chars().any(|c| c != '.')
    {
        return &name[..dot];
    }
    name
}

/// `_is_animated`: whether these bytes hold more than one frame. Lens is searching for one
/// picture; an animation is not one, which is why an animated WebP is converted although a still
/// one is not. Anything that raises is "no".
pub fn is_animated(data: &[u8]) -> bool {
    std::panic::catch_unwind(|| crate::pymeta::frame_count(data))
        .ok()
        .flatten()
        .is_some_and(|frames| frames > 1)
}

/// The pixels `_to_jpeg` hands the encoder: `RGB`, flattened onto white when the picture has
/// alpha (JPEG has none, and the default conversion leaves a transparent background black).
/// `None` is any decode failure.
pub fn flattened_rgb(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    std::panic::catch_unwind(|| {
        let opened = pyraster::open(data, None).ok()?;
        let (w, h) = (
            u32::try_from(opened.width).ok()?,
            u32::try_from(opened.height).ok()?,
        );
        let rgb = match opened.mode {
            Mode::Rgba => flatten(
                opened
                    .data
                    .chunks_exact(4)
                    .map(|p| ([p[0], p[1], p[2]], p[3])),
            ),
            Mode::La => flatten(opened.data.chunks_exact(2).map(|p| ([p[0]; 3], p[1]))),
            Mode::L | Mode::Rgb | Mode::Cmyk => opened.to_rgb().data,
        };
        Some((w, h, rgb))
    })
    .ok()
    .flatten()
}

/// `flat.paste(rgba, mask=alpha)` over a white `RGB`: Pillow's `BLEND`, `DIV255(255 * (255 - a) +
/// c * a)` with its rounding.
fn flatten(pixels: impl Iterator<Item = ([u8; 3], u8)>) -> Vec<u8> {
    let mut out = Vec::new();
    for (rgb, alpha) in pixels {
        for c in rgb {
            let tmp = 255 * (255 - u32::from(alpha)) + u32::from(c) * u32::from(alpha) + 128;
            out.push((((tmp >> 8) + tmp) >> 8) as u8);
        }
    }
    out
}

/// `_to_jpeg`: the same picture as a JPEG, or `None` if it is not a picture at all. The first
/// frame of an animation.
pub fn to_jpeg(data: &[u8]) -> Option<Vec<u8>> {
    let (w, h, rgb) = flattened_rgb(data)?;
    std::panic::catch_unwind(|| encode_rgb_jpeg(w, h, rgb, JPEG_QUALITY).ok())
        .ok()
        .flatten()
}

/// The standard base64 alphabet, padded (`base64.b64encode`).
fn b64encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `_UPLOAD_JS`: the script the page runs to post the image from inside the browser, so one
/// session owns both halves of the search. Ported from Python's constant; the one place the
/// product carries JavaScript.
const UPLOAD_JS: &str = "
(function (b64, name, action, mime) {
  var bin = atob(b64), buf = new Uint8Array(bin.length);
  for (var i = 0; i < bin.length; i++) buf[i] = bin.charCodeAt(i);
  var dt = new DataTransfer();
  dt.items.add(new File([buf], name, {type: mime}));

  var form = document.createElement('form');
  form.method = 'POST';
  form.action = action;
  form.enctype = 'multipart/form-data';
  var input = document.createElement('input');
  input.type = 'file';
  input.name = 'encoded_image';
  form.appendChild(input);
  document.body.appendChild(form);
  input.files = dt.files;
  form.submit();
  return true;
})(%s, %s, %s, %s)
";

/// `upload_script`: the JavaScript that uploads one image from inside the page. The arguments go
/// in as JSON strings (`json.dumps`, ASCII only) rather than by string interpolation: a filename
/// comes off the user's disk, and a stray quote in it would otherwise turn into broken, or
/// injected, script.
pub fn upload_script(image_bytes: &[u8], filename: &str) -> String {
    let json = |text: &str| {
        let mut out = String::new();
        pyjson::string(&mut out, text);
        out
    };
    let args = [
        json(&b64encode(image_bytes)),
        json(filename),
        json(SEARCH_BY_IMAGE_URL),
        json(media_type(image_bytes).unwrap_or("image/jpeg")),
    ];
    let mut parts = UPLOAD_JS.split("%s");
    let mut out = String::from(parts.next().unwrap_or_default());
    for (arg, rest) in args.iter().zip(parts) {
        out.push_str(arg);
        out.push_str(rest);
    }
    out
}
