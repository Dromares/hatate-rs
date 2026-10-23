//! `pyraster`'s Phase 4.e additions (DAN-2127): `Image.resize` with NEAREST, BILINEAR and BICUBIC,
//! and `ImageFilter.GaussianBlur`, pinned to Pillow 12.3.0's output (the digests were computed by
//! Pillow on the same formula-built pictures; the source of the formulas is in each test). The
//! Lanczos path is the one `image_compare_golden.rs` already pins.

use hatate_core::pyraster::{Filter, Image, Mode};
use sha2::{Digest, Sha256};

fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))[..16].to_string()
}

fn rgb(w: usize, h: usize) -> Image {
    let mut data = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            data.extend_from_slice(&[
                ((x * 5 + y * 3) % 256) as u8,
                ((x * x + y * 7) % 256) as u8,
                ((x * y + 13) % 256) as u8,
            ]);
        }
    }
    Image {
        width: w,
        height: h,
        mode: Mode::Rgb,
        data,
    }
}

fn grey(w: usize, h: usize) -> Image {
    let mut data = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            data.push(((x * 37 + y * 91 + (x * y) % 7 * 29) % 256) as u8);
        }
    }
    Image {
        width: w,
        height: h,
        mode: Mode::L,
        data,
    }
}

const RESIZE_SHA: [&str; 96] = [
    "325b548aa5f1eecd",
    "b44c048171aef36a",
    "744474e752acedbb",
    "718b13eaa17bfd65",
    "dd01216e01f908f6",
    "d399738fd1465daa",
    "38da8f8fd4ced795",
    "f214430ecb9ddfa1",
    "80ce384745c89e6d",
    "d6b689fe6f7bd8a0",
    "933b20da397597ec",
    "44193ea049b42190",
    "dc07738f9eddb767",
    "06217d913c3715ac",
    "06d796781ab0e7b3",
    "27bf048bfaa530f0",
    "9de1c6f87a618ec3",
    "34d9448c8f22806c",
    "24232f9e586f88b6",
    "0bd164fe544a7db0",
    "faa306e9dcedab89",
    "ae95f666ae6ca36a",
    "3413ccce396a4cc3",
    "6158019340caa20b",
    "fb66e8751feb8503",
    "a387ef85bda7299c",
    "20f6fe0f955b065c",
    "f78867b4186d441a",
    "d497541c024f086d",
    "0afee0624968d254",
    "976be343e7160204",
    "6b417e57bed6ceca",
    "30b8313cb571da87",
    "a9dc30c03061544c",
    "f865978663839165",
    "750d010cacf01693",
    "4eaf064aa92bee00",
    "f0547f00d283ee7b",
    "e85ab40fa36b19f3",
    "0943a5eeddf28162",
    "21e7803a2df5c9cf",
    "9b196a66f09323fd",
    "e3dea676a91a2d96",
    "a59f38cb4d378ab5",
    "a0e65a6c40725776",
    "608a59347cc29c3a",
    "abc63a2a8ff809e4",
    "d4071915ecf5fb73",
    "35f83441d13d5124",
    "25544eb045c4327e",
    "7c9a538a1a5a0693",
    "b7a28f242b4b248f",
    "266b713d5ba8af9f",
    "6a40717acb0178f0",
    "8f5bdbae2b6b20ee",
    "211a8da3c25c154c",
    "4401c14826fda018",
    "3ea4c02062b5ee52",
    "3ee3f9d5968df589",
    "dd4f008515bc751b",
    "6bd0aa233eda1c29",
    "944592eefe926634",
    "2513d063cb761b10",
    "20e20c67a850e56e",
    "69cc7a7fe60546a1",
    "5477485d10adc5db",
    "ad2e96c34942ebcd",
    "9b3d8a1b83330a5f",
    "97703279fe3544db",
    "89e24a80865b932a",
    "62139eacc6072d80",
    "aec585357fa87519",
    "b6e644624ca096ac",
    "b84e30a81d16478f",
    "b84e30a81d16478f",
    "c6f2595c633e1ec5",
    "0b219e9d227578fb",
    "0b219e9d227578fb",
    "0b219e9d227578fb",
    "0b219e9d227578fb",
    "025095b5d89ef8fc",
    "3650b7d1134912fe",
    "7e21ae840040f281",
    "7e21ae840040f281",
    "e5bbe8ae4b241fee",
    "51ae9b07a696c00e",
    "c34fcabd9f32f56f",
    "9e2b0026a19ed1c1",
    "c83fe64fd2a33b43",
    "af246fa70d5c65e6",
    "2e45c47d3c5134a0",
    "ffbefd5e1e2647f2",
    "8cf7db0cb6c00df3",
    "1d1508b2963c425e",
    "107fbdfc78e6aa19",
    "107fbdfc78e6aa19",
];
const BLUR_SHA: [&str; 10] = [
    "6e340b9cffb37a98",
    "67ff73d66fbd607b",
    "126dda8166d63c48",
    "67f32c2d80eb711c",
    "fc70797f5c3f5292",
    "fea83179db4f071d",
    "a548cff2badcb463",
    "9a59b818f09a5ac0",
    "47f3fead3e2a8684",
    "983f2adc4b650e6b",
];

#[test]
fn resize_matches_pillow_for_every_filter_down_up_and_skewed() {
    let sources = [
        (37usize, 23usize),
        (64, 64),
        (5, 300),
        (300, 5),
        (1, 9),
        (9, 1),
    ];
    let filters = [
        Filter::Nearest,
        Filter::Bilinear,
        Filter::Bicubic,
        Filter::Lanczos,
    ];
    let mut i = 0;
    for (w, h) in sources {
        let im = rgb(w, h);
        let targets = [
            ((w / 2).max(1), (h / 2).max(1)),
            (w * 2, h * 2),
            (w + 3, h + 5),
            ((w - 1).max(1), h),
        ];
        for (tw, th) in targets {
            for filter in filters {
                let out = im.resize_filtered(tw, th, filter);
                assert_eq!((out.width, out.height), (tw, th));
                assert_eq!(
                    digest(&out.data),
                    RESIZE_SHA[i],
                    "{w}x{h} -> {tw}x{th} with {filter:?}"
                );
                i += 1;
            }
        }
    }
    assert_eq!(i, RESIZE_SHA.len());
}

#[test]
fn a_resize_to_the_same_size_is_a_copy() {
    let im = rgb(20, 11);
    for filter in [
        Filter::Nearest,
        Filter::Bilinear,
        Filter::Bicubic,
        Filter::Lanczos,
    ] {
        assert_eq!(im.resize_filtered(20, 11, filter).data, im.data);
    }
}

#[test]
fn gaussian_blur_radius_two_matches_pillow_including_one_pixel_lines() {
    let sizes = [
        (1usize, 1usize),
        (2, 5),
        (3, 3),
        (5, 5),
        (17, 13),
        (64, 40),
        (1, 30),
        (30, 1),
        (4, 4),
        (2, 2),
    ];
    for (i, (w, h)) in sizes.into_iter().enumerate() {
        let out = grey(w, h).gaussian_blur(2.0);
        assert_eq!(digest(&out.data), BLUR_SHA[i], "{w}x{h}");
    }
}

#[test]
fn a_flat_picture_stays_flat_under_the_blur() {
    let flat = Image {
        width: 9,
        height: 7,
        mode: Mode::L,
        data: vec![77; 63],
    };
    assert_eq!(flat.gaussian_blur(2.0).data, vec![77; 63]);
}
