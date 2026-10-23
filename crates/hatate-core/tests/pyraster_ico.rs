//! `pyraster::open` on icons (DAN-2163): which entry of the directory is shown, and a PNG entry
//! that is RGB rather than RGBA. Expected outcomes are what Pillow 12.3.0 did with the same
//! shapes (`Image.open(...).load()`): `IcoFile` shows the largest entry, the lowest colour depth
//! among equals, and decodes a PNG entry as any other PNG.

use hatate_core::pyraster::{self, Mode};
use image::{ColorType, ImageEncoder, codecs::png::PngEncoder};

fn png_rgb(w: u32, h: u32, v: u8) -> Vec<u8> {
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(&vec![v; (w * h * 3) as usize], w, h, ColorType::Rgb8.into())
        .unwrap();
    out
}

/// An icon whose entries are `(width, height, declared bpp, PNG bytes)`.
fn ico(entries: &[(u8, u8, u16, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![0, 0, 1, 0, entries.len() as u8, 0];
    let mut offset = 6 + 16 * entries.len() as u32;
    for (w, h, bpp, png) in entries {
        out.extend_from_slice(&[*w, *h, 0, 0, 1, 0]);
        out.extend_from_slice(&bpp.to_le_bytes());
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (.., png) in entries {
        out.extend_from_slice(png);
    }
    out
}

#[test]
fn a_png_entry_that_is_rgb_is_decoded_as_any_other_png() {
    let img = pyraster::open(&ico(&[(4, 4, 32, png_rgb(4, 4, 9))]), None).unwrap();
    assert_eq!((img.width, img.height, img.mode), (4, 4, Mode::Rgb));
    assert!(img.data.iter().all(|&b| b == 9));
}

#[test]
fn the_largest_entry_is_shown_even_when_a_smaller_one_has_more_bits() {
    let data = ico(&[(4, 4, 32, png_rgb(4, 4, 1)), (8, 8, 8, png_rgb(8, 8, 2))]);
    let img = pyraster::open(&data, None).unwrap();
    assert_eq!((img.width, img.height), (8, 8));
    assert_eq!(img.data[0], 2);
}

#[test]
fn among_equal_sizes_the_lowest_colour_depth_is_shown() {
    let data = ico(&[(4, 4, 32, png_rgb(4, 4, 1)), (4, 4, 8, png_rgb(4, 4, 2))]);
    let img = pyraster::open(&data, None).unwrap();
    assert_eq!(img.data[0], 2);
}
