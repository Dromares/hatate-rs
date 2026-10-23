//! `pyraster::open` on damaged PNGs (DAN-2160): a bad CRC on a pixel chunk, damaged zlib data, a
//! stream that ends early, chunks cut short after the pixel data. Each case is a synthetic copy of
//! one 8x8 grey picture, and its expected outcome is what Pillow 12.3.0 did with the same bytes
//! (`Image.open(...).load()`), checked case by case: the pixels it decodes, or the message of the
//! `OSError` it raises. Where Pillow raises, `pyraster` refuses with the same text; where it
//! decodes, the pixels are the same.

use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use hatate_core::pyraster::{self, Mode, OpenError};

const W: usize = 8;
const H: usize = 8;

const BROKEN: &str = "broken data stream when reading image file";
const NOT_PROCESSED: &str = "image file is truncated (0 bytes not processed)";
const TRUNCATED: &str = "image file is truncated";
const UNKNOWN_FILTER: &str = "unrecognized data stream contents when reading image file";
const READ_CUT: &str = "Truncated File Read";

fn pixel(x: usize, y: usize) -> u8 {
    (y * 20 + x * 3) as u8
}

fn picture() -> Vec<u8> {
    (0..H)
        .flat_map(|y| (0..W).map(move |x| pixel(x, y)))
        .collect()
}

/// The filtered rows of the picture (every row filter type 0).
fn rows() -> Vec<u8> {
    let mut raw = Vec::new();
    for y in 0..H {
        raw.push(0);
        raw.extend((0..W).map(|x| pixel(x, y)));
    }
    raw
}

/// The same picture as Adam7 passes (every row filter type 0).
fn adam7_rows() -> Vec<u8> {
    const PASSES: [(usize, usize, usize, usize); 7] = [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ];
    let mut raw = Vec::new();
    for (x0, y0, dx, dy) in PASSES {
        for y in (y0..H).step_by(dy) {
            if x0 < W {
                raw.push(0);
                raw.extend((x0..W).step_by(dx).map(|x| pixel(x, y)));
            }
        }
    }
    raw
}

fn zlib(raw: &[u8], level: Compression) -> Vec<u8> {
    let mut z = ZlibEncoder::new(Vec::new(), level);
    z.write_all(raw).unwrap();
    z.finish().unwrap()
}

fn crc(cid: &[u8], body: &[u8]) -> [u8; 4] {
    let mut h = crc32fast::Hasher::new();
    h.update(cid);
    h.update(body);
    h.finalize().to_be_bytes()
}

/// A chunk; `crc` overrides the checksum.
fn chunk_with(cid: &[u8; 4], body: &[u8], crc_override: Option<[u8; 4]>) -> Vec<u8> {
    let mut out = (body.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(cid);
    out.extend_from_slice(body);
    out.extend_from_slice(&crc_override.unwrap_or_else(|| crc(cid, body)));
    out
}

fn chunk(cid: &[u8; 4], body: &[u8]) -> Vec<u8> {
    chunk_with(cid, body, None)
}

fn ihdr(interlace: u8) -> Vec<u8> {
    let mut body = (W as u32).to_be_bytes().to_vec();
    body.extend((H as u32).to_be_bytes());
    body.extend([8, 0, 0, 0, interlace]);
    chunk(b"IHDR", &body)
}

const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// A file: signature, `IHDR`, the given chunks, `IEND`.
fn file(interlace: u8, middle: &[Vec<u8>], with_iend: bool) -> Vec<u8> {
    let mut out = SIGNATURE.to_vec();
    out.extend(ihdr(interlace));
    for c in middle {
        out.extend_from_slice(c);
    }
    if with_iend {
        out.extend(chunk(b"IEND", b""));
    }
    out
}

fn idat(stream: &[u8]) -> Vec<u8> {
    chunk(b"IDAT", stream)
}

/// The stream cut into `n` chunks of about equal size, each with a good checksum.
fn idats(stream: &[u8], n: usize) -> Vec<Vec<u8>> {
    let step = stream.len().div_ceil(n);
    stream.chunks(step).map(idat).collect()
}

fn with_bad_crc(chunk: &[u8]) -> Vec<u8> {
    let mut c = chunk.to_vec();
    let n = c.len();
    c[n - 1] ^= 0xff;
    c
}

pub enum Expect {
    Pixels(Vec<u8>),
    Refused(&'static str),
}

use Expect::{Pixels, Refused};

/// Every case: a name, the file, and what Pillow did with it.
pub fn cases() -> Vec<(&'static str, Vec<u8>, Expect)> {
    let pic = picture();
    let stream = zlib(&rows(), Compression::default());
    let mut v: Vec<(&'static str, Vec<u8>, Expect)> = Vec::new();
    let mut add = |name, bytes, expect| v.push((name, bytes, expect));

    add(
        "clean",
        file(0, &[idat(&stream)], true),
        Pixels(pic.clone()),
    );
    add(
        "bad_crc_on_the_idat_chunk",
        file(0, &[with_bad_crc(&idat(&stream))], true),
        Pixels(pic.clone()),
    );
    let three: Vec<Vec<u8>> = idats(&stream, 3).iter().map(|c| with_bad_crc(c)).collect();
    add(
        "bad_crc_on_every_idat_of_three",
        file(0, &three, true),
        Pixels(pic.clone()),
    );
    add(
        "stored_blocks",
        file(0, &[idat(&zlib(&rows(), Compression::none()))], true),
        Pixels(pic.clone()),
    );
    add(
        "adam7",
        file(
            1,
            &[idat(&zlib(&adam7_rows(), Compression::default()))],
            true,
        ),
        Pixels(pic.clone()),
    );
    add(
        "missing_iend",
        file(0, &[idat(&stream)], false),
        Pixels(pic.clone()),
    );
    let mut bad_iend = file(0, &[idat(&stream)], false);
    bad_iend.extend(chunk_with(b"IEND", b"", Some([0, 0, 0, 0])));
    add("bad_iend_crc", bad_iend, Pixels(pic.clone()));
    let mut after_iend = file(0, &[idat(&stream)], true);
    after_iend.extend_from_slice(b"trailing bytes after the end");
    add("bytes_after_iend", after_iend, Pixels(pic.clone()));

    // The adler32 is the last four bytes of the stream. Pillow checks it only when it is in
    // the block that completes the last row.
    let n = stream.len();
    let mut bad_adler = stream.clone();
    bad_adler[n - 1] ^= 1;
    add(
        "bad_adler_in_the_last_chunk",
        file(0, &[idat(&bad_adler)], true),
        Refused(BROKEN),
    );
    add(
        "bad_adler_in_a_chunk_of_its_own",
        file(
            0,
            &[idat(&stream[..n - 4]), idat(&bad_adler[n - 4..])],
            true,
        ),
        Pixels(pic.clone()),
    );
    add(
        "no_adler_at_all",
        file(0, &[idat(&stream[..n - 4])], true),
        Pixels(pic.clone()),
    );

    let mut bad_header = stream.clone();
    bad_header[0] ^= 1;
    add(
        "zlib_header_damaged",
        file(0, &[idat(&bad_header)], true),
        Refused(BROKEN),
    );
    // A final stored block with its type bits set to the reserved 3.
    let mut bad_block = zlib(&rows(), Compression::none());
    bad_block[2] |= 0x07;
    add(
        "reserved_block_type",
        file(0, &[idat(&bad_block)], true),
        Refused(BROKEN),
    );
    // The header of a stream that wants a preset dictionary (FDICT), then the same deflate data.
    let mut dict = vec![0x78, 0x20, 0, 0, 0, 1];
    dict.extend_from_slice(&stream[2..]);
    add(
        "preset_dictionary",
        file(0, &[idat(&dict)], true),
        Refused(NOT_PROCESSED),
    );

    // A stream that ends early.
    let stored = zlib(&rows(), Compression::none());
    let rows_of = |k: usize| zlib(&rows()[..k * (W + 1)], Compression::default());
    let mut three_rows = picture()[..3 * W].to_vec();
    three_rows.resize(H * W, 0);
    add(
        "stream_ends_on_a_row_boundary",
        file(0, &[idat(&rows_of(3))], true),
        Pixels(three_rows),
    );
    add(
        "stream_ends_inside_a_row",
        file(
            0,
            &[idat(&zlib(
                &rows()[..3 * (W + 1) + 4],
                Compression::default(),
            ))],
            true,
        ),
        Refused(NOT_PROCESSED),
    );
    add(
        "stream_cut_in_the_middle",
        file(0, &[idat(&stream[..stream.len() / 2])], true),
        Refused(NOT_PROCESSED),
    );
    add(
        "stored_stream_cut",
        file(0, &[idat(&stored[..stored.len() - 10])], true),
        Refused(NOT_PROCESSED),
    );
    add(
        "empty_idat",
        file(0, &[idat(b"")], true),
        Refused(NOT_PROCESSED),
    );
    let cut_after_crc = file(0, &[idat(&stream[..stream.len() / 2])], false);
    add(
        "file_ends_after_the_idat_chunk",
        cut_after_crc,
        Refused(TRUNCATED),
    );

    let mut bad_filter = rows();
    bad_filter[2 * (W + 1)] = 9;
    add(
        "filter_type_past_four",
        file(0, &[idat(&zlib(&bad_filter, Compression::default()))], true),
        Refused(UNKNOWN_FILTER),
    );

    // Chunks after the pixel data are read to the end of their bodies.
    let mut cut_text = file(0, &[idat(&stream)], false);
    cut_text.extend_from_slice(&chunk(b"tEXt", b"Comment\0a long enough comment")[..14]);
    add(
        "text_chunk_after_the_data_cut_short",
        cut_text,
        Refused(READ_CUT),
    );
    let mut cut_idat = file(0, &[idat(&stream)], false);
    cut_idat.extend_from_slice(&idat(&stream)[..11]);
    add(
        "second_idat_after_the_data_cut_short",
        cut_idat,
        Refused(READ_CUT),
    );
    let mut text_crc = file(0, &[idat(&stream)], false);
    text_crc.extend(chunk_with(b"tEXt", b"Comment\0fine", Some([1, 2, 3, 4])));
    text_crc.extend(chunk(b"IEND", b""));
    add(
        "text_chunk_after_the_data_bad_crc",
        text_crc,
        Pixels(pic.clone()),
    );
    // The decoder fails, then the chunk walk raises first.
    let mut broken_then_cut = file(0, &[idat(&bad_header)], false);
    broken_then_cut.extend_from_slice(&chunk(b"tEXt", b"Comment\0a long enough comment")[..14]);
    add(
        "broken_stream_then_a_cut_chunk",
        broken_then_cut,
        Refused(READ_CUT),
    );
    v
}

const PREFIX: &str = "Downloaded data does not decode as a valid image: ";

/// `_validate_image_decodes` opens, runs `verify()` (which checks every chunk's checksum, the
/// pixel chunks too) and only then loads: the message Pillow's `verify()` raises, or `None`.
pub fn validator_cases() -> Vec<(&'static str, Vec<u8>, Option<String>)> {
    let stream = zlib(&rows(), Compression::default());
    let msg = |m: &str| Some(format!("{PREFIX}{m}"));
    let mut v = Vec::new();
    v.push(("clean", file(0, &[idat(&stream)], true), None));
    v.push((
        "bad_idat_crc",
        file(0, &[with_bad_crc(&idat(&stream))], true),
        msg("broken PNG file (bad header checksum in b'IDAT')"),
    ));
    let mut second = file(0, &[idat(&stream[..10])], false);
    second.extend(with_bad_crc(&idat(&stream[10..])));
    second.extend(chunk(b"IEND", b""));
    v.push((
        "bad_crc_on_a_later_idat",
        second,
        msg("broken PNG file (bad header checksum in b'IDAT')"),
    ));
    let mut text = file(0, &[idat(&stream)], false);
    text.extend(chunk_with(b"tEXt", b"Comment\0fine", Some([1, 2, 3, 4])));
    text.extend(chunk(b"IEND", b""));
    v.push((
        "bad_crc_on_a_text_chunk_after_the_data",
        text,
        msg("broken PNG file (bad header checksum in b'tEXt')"),
    ));
    v.push((
        "missing_iend",
        file(0, &[idat(&stream)], false),
        msg("truncated PNG file"),
    ));
    let mut iend_crc = file(0, &[idat(&stream)], false);
    iend_crc.extend(chunk_with(b"IEND", b"", Some([0, 0, 0, 0])));
    v.push(("bad_iend_crc_is_not_looked_at", iend_crc, None));
    let mut cut_crc = file(0, &[idat(&stream)], false);
    cut_crc.truncate(cut_crc.len() - 2);
    v.push((
        "file_ends_inside_the_idat_checksum",
        cut_crc,
        msg("broken PNG file (incomplete checksum in b'IDAT')"),
    ));
    let mut cut_body = file(0, &[idat(&stream)], false);
    cut_body.extend_from_slice(&chunk(b"tEXt", b"Comment\0a long enough comment")[..14]);
    v.push(("text_chunk_cut_short", cut_body, msg(READ_CUT)));
    let mut junk = file(0, &[idat(&stream)], false);
    junk.extend_from_slice(b"\0\0\0\x01\x01\x02\x03\x04");
    v.push((
        "junk_chunk_id",
        junk,
        msg("broken PNG file (chunk b'\\x01\\x02\\x03\\x04')"),
    ));
    // The checksum comes before the load: a stream that would not decode reads as a bad checksum.
    let mut bad_header = stream.clone();
    bad_header[0] ^= 1;
    v.push((
        "bad_checksum_and_a_broken_stream",
        file(0, &[with_bad_crc(&idat(&bad_header))], true),
        msg("broken PNG file (bad header checksum in b'IDAT')"),
    ));
    v.push((
        "broken_stream_good_checksums",
        file(0, &[idat(&bad_header)], true),
        msg(BROKEN),
    ));
    v
}

#[test]
fn the_validator_checks_every_chunk_checksum_before_it_loads() {
    for (name, bytes, want) in validator_cases() {
        assert_eq!(
            hatate_core::hydrus_import::validate_image_decodes(&bytes, Some("PNG")),
            want,
            "{name}"
        );
    }
}

#[test]
fn damaged_pngs_decode_or_are_refused_as_pillow_does() {
    for (name, bytes, expect) in cases() {
        let got = pyraster::open(&bytes, None);
        match (expect, got) {
            (Pixels(want), Ok(img)) => {
                assert_eq!((img.width, img.height, img.mode), (W, H, Mode::L), "{name}");
                assert_eq!(img.data, want, "{name}");
            }
            (Refused(want), Err(OpenError::Os(text))) => assert_eq!(text, want, "{name}"),
            (Pixels(_), Err(e)) => panic!("{name}: refused: {e}"),
            (Refused(want), Ok(_)) => panic!("{name}: decoded, Pillow raises {want:?}"),
            (Refused(_), Err(e)) => panic!("{name}: wrong error kind: {e:?}"),
        }
    }
}

/// The `image` crate refuses a bad checksum on a pixel chunk; Pillow never looks at it.
#[test]
fn bad_idat_crc_is_not_an_error_for_any_pixel_chunk_count() {
    let stream = zlib(&rows(), Compression::none());
    for parts in 1..=6 {
        let damaged: Vec<Vec<u8>> = idats(&stream, parts)
            .iter()
            .map(|c| with_bad_crc(c))
            .collect();
        let img = pyraster::open(&file(0, &damaged, true), None).unwrap();
        assert_eq!(img.data, picture(), "{parts} chunks");
    }
}
