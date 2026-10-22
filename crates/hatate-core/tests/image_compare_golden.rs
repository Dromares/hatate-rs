//! Hashes pinned to Pillow's (DAN-2062, Phase 3.g).
//!
//! `tests/data/image_compare/` holds synthetic pictures in every format the decoders handle, and
//! `expected.json` is what the Python code (`core/image_compare.py` at `76c85c4`, Pillow 12.3.0,
//! libjpeg-turbo 3.1.4) answered for them: the dhash of each at 64 and 256 bits, its dimensions,
//! `local_prints`, and `compare_to_local` / `aligned_similarity` for a set of pairs. The Rust
//! port must answer the same, bit for bit. The pictures are made by a script that is not in this
//! tree (it is Python); the bytes are what is committed.
//!
//! `jpeg/` does the same for the decoder alone: the SHA-256 of the pixels Pillow decodes each file
//! to, at every `Image.draft` scale, and which files Pillow refuses.

use std::path::{Path, PathBuf};

use hatate_core::image_compare::{self as ic, Source};
use hatate_core::pyjpeg::{self, Error};
use serde_json::Value;
use sha2::{Digest, Sha256};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/image_compare")
}

fn expected(rel: &str) -> Value {
    serde_json::from_slice(&std::fs::read(dir().join(rel)).unwrap()).unwrap()
}

fn hex(h: Option<ic::Hash>) -> Value {
    h.map_or(Value::Null, |h| Value::String(h.to_hex()))
}

fn hexes(v: &[ic::Hash]) -> Value {
    Value::Array(v.iter().map(|h| Value::String(h.to_hex())).collect())
}

#[test]
fn dhash_matches_pillow_on_every_format() {
    let want = expected("images/expected.json");
    let mut checked = 0;
    for (name, v) in want["dhash8"].as_object().unwrap() {
        let path = dir().join("images").join(name);
        assert_eq!(
            &hex(ic::dhash(Source::Path(&path), 8).unwrap()),
            v,
            "dhash8 {name}"
        );
        checked += 1;
    }
    for (name, v) in want["dhash16"].as_object().unwrap() {
        let data = std::fs::read(dir().join("images").join(name)).unwrap();
        assert_eq!(
            &hex(ic::dhash(Source::Bytes(&data), 16).unwrap()),
            v,
            "dhash16 {name}"
        );
        checked += 1;
    }
    assert_eq!(checked, 76, "every picture is checked at both sizes");
}

#[test]
fn dimensions_match_pillow() {
    let want = expected("images/expected.json");
    for (name, v) in want["dims"].as_object().unwrap() {
        let data = std::fs::read(dir().join("images").join(name)).unwrap();
        let got = ic::image_dimensions(Source::Bytes(&data))
            .unwrap()
            .map_or(Value::Null, |(w, h)| serde_json::json!([w, h]));
        assert_eq!(&got, v, "dims {name}");
    }
}

#[test]
fn local_prints_match_pillow() {
    let want = expected("images/expected.json");
    for (name, v) in want["prints"].as_object().unwrap() {
        let path = dir().join("images").join(name);
        let got = ic::local_prints(Source::Path(&path)).map_or(Value::Null, |p| {
            serde_json::json!({"straight": hexes(&p.straight), "mirrored": hex(p.mirrored),
                "fine_straight": hexes(&p.fine_straight), "fine_mirrored": hex(p.fine_mirrored)})
        });
        // Python's dict has these four keys; compare them one at a time for a readable failure.
        for key in ["straight", "mirrored", "fine_straight", "fine_mirrored"] {
            assert_eq!(got.get(key), v.get(key), "prints {name} {key}");
        }
        assert_eq!(got.is_null(), v.is_null(), "prints {name}");
    }
}

#[test]
fn comparisons_match_pillow() {
    let want = expected("images/expected.json");
    let pairs = want["compare"].as_array().unwrap();
    assert!(pairs.len() >= 20);
    for pair in pairs {
        let (local, thumb) = (
            pair["local"].as_str().unwrap(),
            pair["thumb"].as_str().unwrap(),
        );
        let prints = ic::local_prints(Source::Path(&dir().join("images").join(local)));
        let data = std::fs::read(dir().join("images").join(thumb)).unwrap();
        let c = ic::compare_to_local(prints.as_ref(), Source::Bytes(&data));
        let a = ic::aligned_similarity(prints.as_ref(), Source::Bytes(&data));
        let label = format!("{local} vs {thumb}");
        assert_eq!(
            serde_json::json!(c.distance),
            pair["distance"],
            "distance {label}"
        );
        assert_eq!(
            serde_json::json!(c.mirrored),
            pair["mirrored"],
            "mirrored {label}"
        );
        assert_eq!(
            serde_json::json!(c.fine_distance),
            pair["fine"],
            "fine {label}"
        );
        assert_eq!(
            serde_json::json!([a.0, a.1]),
            pair["aligned"],
            "aligned {label}"
        );
    }
}

fn decode_sha(data: &[u8], scale: usize) -> Result<(usize, usize, String), Error> {
    let d = pyjpeg::decode(data, scale)?;
    Ok((d.width, d.height, format!("{:x}", Sha256::digest(&d.data))))
}

#[test]
fn the_jpeg_decoder_matches_libjpeg_turbo_at_every_scale() {
    let want = expected("jpeg/expected.json");
    let mut decodes = 0;
    for (name, scales) in want.as_object().unwrap() {
        let data = std::fs::read(dir().join("jpeg").join(name)).unwrap();
        for (scale, v) in scales.as_object().unwrap() {
            let scale: usize = scale.parse().unwrap();
            let got = decode_sha(&data, scale);
            if name == "cj_arith.jpg" {
                // Arithmetic coding: Pillow decodes it, this does not (docs/image-compare.md, class C1).
                assert!(matches!(got, Err(Error::Unsupported(_))), "{name}");
                continue;
            }
            if v.get("error").is_some() {
                // Pillow raised (OSError): so must the port, and as a truncation.
                assert!(
                    got.is_err(),
                    "{name} scale {scale}: Pillow raised, the port decoded"
                );
                continue;
            }
            let (w, h, sha) = got.unwrap_or_else(|e| panic!("{name} scale {scale}: {e}"));
            assert_eq!(
                serde_json::json!([w, h]),
                v["size"],
                "{name} scale {scale} size"
            );
            assert_eq!(
                sha,
                v["sha256"].as_str().unwrap(),
                "{name} scale {scale} pixels"
            );
            decodes += 1;
        }
    }
    assert_eq!(decodes, 76, "every decode that Pillow makes is compared");
}
