//! Keeps `corpus/` honest: every `corpus/MANIFEST.json` entry exists on disk and its
//! SHA-256 hashes match the raw bytes, and no item directory is missing from the manifest.
//!
//! std only, plus `sha2` (dev-dependency) for the hashes. The manifest is JSON, so this file
//! carries a small JSON reader instead of pulling in a second crate.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const PYTHON_COMMIT: &str = "76c85c44fea050e06ae91c77ecc5b40e2f23f986";
const STATUSES: [&str; 4] = ["live-match", "drifted", "dead", "not-probed"];
const FAMILIES: [&str; 2] = ["boorus", "engines"];

// ---- a small JSON reader --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    fn arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn skip_ws(&mut self) {
        while matches!(self.bytes.get(self.pos), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Result<(), String> {
        if self.bytes.get(self.pos) == Some(&byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected {:?} at byte {}", byte as char, self.pos))
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(format!("bad literal at byte {}", self.pos))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.skip_ws();
        match self.bytes.get(self.pos) {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::Str),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => {
                let start = self.pos;
                while matches!(
                    self.bytes.get(self.pos),
                    Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                ) {
                    self.pos += 1;
                }
                let text =
                    std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|e| e.to_string())?;
                Ok(Json::Num(text.to_string()))
            }
            other => Err(format!("unexpected {:?} at byte {}", other, self.pos)),
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.eat(b'{')?;
        let mut fields = Vec::new();
        self.skip_ws();
        if self.bytes.get(self.pos) == Some(&b'}') {
            self.pos += 1;
            return Ok(Json::Obj(fields));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.eat(b':')?;
            fields.push((key, self.value()?));
            self.skip_ws();
            match self.bytes.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(fields));
                }
                _ => return Err(format!("bad object at byte {}", self.pos)),
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.eat(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.bytes.get(self.pos) == Some(&b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.bytes.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(format!("bad array at byte {}", self.pos)),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let end = self.pos + 4;
        let digits = self.bytes.get(self.pos..end).ok_or("short \\u escape")?;
        let text = std::str::from_utf8(digits).map_err(|e| e.to_string())?;
        self.pos = end;
        u32::from_str_radix(text, 16).map_err(|e| e.to_string())
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let byte = *self.bytes.get(self.pos).ok_or("unterminated string")?;
            self.pos += 1;
            match byte {
                b'"' => return String::from_utf8(out).map_err(|e| e.to_string()),
                b'\\' => {
                    let esc = *self.bytes.get(self.pos).ok_or("unterminated escape")?;
                    self.pos += 1;
                    let ch = match esc {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let mut code = self.hex4()?;
                            if (0xD800..0xDC00).contains(&code) {
                                self.eat(b'\\')?;
                                self.eat(b'u')?;
                                let low = self.hex4()?;
                                code = 0x10000
                                    + ((code - 0xD800) << 10)
                                    + (low.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            char::from_u32(code).ok_or("bad \\u escape")?
                        }
                        _ => return Err(format!("bad escape at byte {}", self.pos)),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(byte),
            }
        }
    }
}

fn parse_json(bytes: &[u8]) -> Result<Json, String> {
    let mut reader = Reader { bytes, pos: 0 };
    let value = reader.value()?;
    reader.skip_ws();
    if reader.pos == bytes.len() {
        Ok(value)
    } else {
        Err(format!("trailing data at byte {}", reader.pos))
    }
}

// ---- the checks ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn field<'a>(item: &'a Json, key: &str) -> &'a str {
    item.get(key)
        .and_then(Json::str)
        .unwrap_or_else(|| panic!("manifest item is missing string field {key:?}: {item:?}"))
}

/// Every directory under `dir` that holds a `call.json`, as repo-relative `/` paths.
fn item_dirs(root: &Path, dir: &Path, found: &mut BTreeSet<String>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display())) {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            item_dirs(root, &path, found);
        } else if path.file_name().is_some_and(|n| n == "call.json") {
            let rel = path
                .parent()
                .expect("parent")
                .strip_prefix(root)
                .expect("under root");
            found.insert(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn every_manifest_entry_exists_and_its_hashes_match() {
    let root = repo_root();
    let raw = fs::read(root.join("corpus/MANIFEST.json")).expect("corpus/MANIFEST.json");
    let manifest = parse_json(&raw).expect("MANIFEST.json parses");

    assert_eq!(
        manifest.get("python_commit").and_then(Json::str),
        Some(PYTHON_COMMIT)
    );
    let items = manifest
        .get("items")
        .and_then(Json::arr)
        .expect("items array");
    let declared = match manifest.get("item_count") {
        Some(Json::Num(n)) => n.parse::<usize>().expect("item_count is an integer"),
        other => panic!("item_count missing or not a number: {other:?}"),
    };
    assert_eq!(
        declared,
        items.len(),
        "item_count disagrees with the items array"
    );
    assert!(!items.is_empty(), "the corpus is empty");

    let mut ids = BTreeSet::new();
    let mut dirs = BTreeSet::new();
    let mut bad: Vec<String> = Vec::new();

    for item in items {
        let id = field(item, "id");
        assert!(ids.insert(id.to_string()), "duplicate id {id}");
        let dir = field(item, "dir");
        assert!(dirs.insert(dir.to_string()), "two items share dir {dir}");

        let family = field(item, "family");
        assert!(
            FAMILIES.contains(&family),
            "{id}: unknown family {family:?}"
        );
        assert!(
            dir.starts_with(&format!("corpus/{family}/")),
            "{id}: dir {dir} is outside corpus/{family}/"
        );
        assert_eq!(
            field(item, "python_commit"),
            PYTHON_COMMIT,
            "{id}: python_commit"
        );
        let status = field(item, "phase_0d_status");
        assert!(
            STATUSES.contains(&status),
            "{id}: unknown phase_0d_status {status:?}"
        );

        for (name_key, hash_key) in [
            ("input", "input_sha256"),
            ("call", "call_sha256"),
            ("expected", "expected_sha256"),
        ] {
            let path = root.join(dir).join(field(item, name_key));
            match fs::read(&path) {
                Err(e) => bad.push(format!("{id}: cannot read {}: {e}", path.display())),
                Ok(bytes) => {
                    let want = field(item, hash_key);
                    let got = sha256_hex(&bytes);
                    if got != want {
                        bad.push(format!(
                            "{id}: {name_key} sha256 is {got}, manifest says {want}"
                        ));
                    }
                    if name_key != "input" && parse_json(&bytes).is_err() {
                        bad.push(format!("{id}: {name_key} is not valid JSON"));
                    }
                }
            }
        }

        // expected.json answers exactly the callables that call.json lists.
        let call = fs::read(root.join(dir).join(field(item, "call")))
            .ok()
            .and_then(|b| parse_json(&b).ok());
        let expected = fs::read(root.join(dir).join(field(item, "expected")))
            .ok()
            .and_then(|b| parse_json(&b).ok());
        if let (Some(call), Some(Json::Obj(results))) = (
            call,
            expected.as_ref().and_then(|e| e.get("results")).cloned(),
        ) {
            let mut listed: Vec<&str> = call
                .get("callables")
                .and_then(Json::arr)
                .unwrap_or(&[])
                .iter()
                .filter_map(Json::str)
                .collect();
            let mut answered: Vec<&str> = results.iter().map(|(k, _)| k.as_str()).collect();
            listed.sort_unstable();
            answered.sort_unstable();
            if listed != answered {
                bad.push(format!(
                    "{id}: call.json callables {listed:?} != expected.json results {answered:?}"
                ));
            }
        } else {
            bad.push(format!("{id}: call.json / expected.json shape is wrong"));
        }
    }

    assert!(bad.is_empty(), "corpus problems:\n{}", bad.join("\n"));

    // The other direction: no item directory exists that the manifest does not list.
    let mut on_disk = BTreeSet::new();
    for family in FAMILIES {
        item_dirs(&root, &root.join("corpus").join(family), &mut on_disk);
    }
    let unlisted: Vec<_> = on_disk.difference(&dirs).collect();
    let missing: Vec<_> = dirs.difference(&on_disk).collect();
    assert!(
        unlisted.is_empty() && missing.is_empty(),
        "item directories differ from the manifest: on disk only {unlisted:?}, manifest only {missing:?}"
    );
}

#[test]
fn json_reader_handles_escapes_and_rejects_trailing_data() {
    let parsed = parse_json(b"{\"a\": [\"x\\n\\u00e9\\ud83d\\ude00\", 1.5e-5, true, null]}")
        .expect("parses");
    let items = parsed.get("a").and_then(Json::arr).expect("array");
    assert_eq!(items[0].str(), Some("x\n\u{e9}\u{1f600}"));
    assert_eq!(items[1], Json::Num("1.5e-5".to_string()));
    assert!(parse_json(b"{} x").is_err());
    assert!(parse_json(b"{\"a\": }").is_err());
}
