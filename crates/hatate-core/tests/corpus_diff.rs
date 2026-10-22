//! The corpus differential runner (DAN-2000, Phase 2 of DAN-1961).
//!
//! Reads `corpus/MANIFEST.json`, runs every item of every parser that has a Rust port, and
//! compares what the Rust function returns with the `expected.json` Python produced, under the
//! serialisation rules in `corpus/README.md`:
//!
//! - numbers are compared as parsed numbers (`1` equals `1.0`), object key order does not
//!   matter, a tuple and a list are both arrays;
//! - a `raises` item passes only when the Rust function returns the error variant that
//!   [`ERROR_VARIANTS`] maps from that Python class name.
//!
//! No skip lists, no per-item allow-lists, no tolerance. A missing input file, a hash that does
//! not match the manifest, or a callable in `call.json` with no mapping for a ported parser is a
//! FAILURE of that item, and the run's panic message names each failing item id with the JSON
//! diff, so a CI log is enough to debug from.
//!
//! **Adding a parser:** write `tests/corpus_ports/<site>.rs` (a `PORT` mapping each dotted Python
//! callable to its Rust function), add `pub mod <site>;` to `corpus_ports/mod.rs`, and add
//! `&corpus_ports::<site>::PORT` to [`PORTS`]. The coverage test then stops listing it.

mod corpus_ports;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};

// ---- the table -------------------------------------------------------------------------

/// What a ported callable returns: its result as JSON, or the name of the Rust error variant it
/// returned (compared through [`ERROR_VARIANTS`]).
pub type Outcome = Result<Value, &'static str>;

/// A Rust function standing in for one Python callable.
pub type CallFn = fn(&Call) -> Outcome;

/// The ported callables of one parser. A booru port is one site module (`unit` = the module's
/// short name, as the manifest's `parser`); an engine port is one callable (`unit` = its dotted
/// name), because engine modules hold several callables that are ported separately.
pub struct Port {
    pub family: &'static str,
    pub unit: &'static str,
    pub calls: &'static [(&'static str, CallFn)],
}

/// Every parser that has a Rust port.
static PORTS: &[&Port] = &[
    &corpus_ports::danbooru::PORT,
    &corpus_ports::gelbooru::PORT,
    &corpus_ports::safebooru::PORT,
    &corpus_ports::rule34::PORT,
    &corpus_ports::xbooru::PORT,
];

/// The Python exception class a `raises` item names, and the Rust error variant that stands for
/// it. `BooruError` and `BooruContentGoneError` are `hatate_core::boorus::BooruError::{Fetch,
/// ContentGone}`; later tickets add `IqdbError` and the others with their ports.
static ERROR_VARIANTS: &[(&str, &str)] = &[
    ("BooruError", "BooruError::Fetch"),
    ("BooruContentGoneError", "BooruError::ContentGone"),
];

/// The first argument of an item: the bytes of `input.*`.
pub struct Input {
    pub bytes: Vec<u8>,
}

/// One call, as `call.json` describes it.
pub struct Call<'a> {
    pub input: &'a Input,
    pub extra_args: &'a [Value],
    pub kwargs: &'a Map<String, Value>,
    pub context: &'a Map<String, Value>,
}

impl Call<'_> {
    /// The input as text: a booru's response body, whatever its extension.
    pub fn body(&self) -> &str {
        std::str::from_utf8(&self.input.bytes).expect("input.* is not UTF-8")
    }

    /// The post-page URL, `extra_args[0]` for a booru item.
    pub fn url(&self) -> &str {
        self.extra_args
            .first()
            .and_then(Value::as_str)
            .expect("extra_args[0] is not a string")
    }
}

/// The `api_post` of a Gelbooru-family item's `context`: the Data-API post for the URL, or `null`
/// for "the API gave nothing".
pub fn api_post(call: &Call) -> hatate_core::boorus::Context {
    let post = call.context.get("api_post").and_then(Value::as_object);
    hatate_core::boorus::Context::with_api_post(post.cloned())
}

/// A result as the runner compares it.
pub fn ok<T: Serialize>(value: T) -> Outcome {
    Ok(serde_json::to_value(value).expect("result serialises to JSON"))
}

// ---- the manifest ----------------------------------------------------------------------

struct Item {
    id: String,
    family: String,
    /// What the item counts toward: the booru module, or the engine callable.
    unit: String,
    dir: String,
    input: String,
    call: String,
    expected: String,
    input_sha256: String,
    call_sha256: String,
    expected_sha256: String,
    python_callables: Vec<String>,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn str_field(item: &Value, key: &str) -> String {
    item[key]
        .as_str()
        .unwrap_or_else(|| panic!("manifest item has no string {key:?}: {item}"))
        .to_string()
}

fn load_items(root: &Path) -> Vec<Item> {
    let raw = fs::read(root.join("corpus/MANIFEST.json")).expect("corpus/MANIFEST.json");
    let manifest: Value = serde_json::from_slice(&raw).expect("MANIFEST.json parses");
    manifest["items"]
        .as_array()
        .expect("items array")
        .iter()
        .map(|item| {
            let python_callables: Vec<String> = item["python_callables"]
                .as_array()
                .expect("python_callables")
                .iter()
                .map(|c| c.as_str().expect("callable name").to_string())
                .collect();
            let family = str_field(item, "family");
            let unit = if family == "engines" {
                python_callables.first().cloned().unwrap_or_default()
            } else {
                str_field(item, "parser")
            };
            Item {
                id: str_field(item, "id"),
                family,
                unit,
                dir: str_field(item, "dir"),
                input: str_field(item, "input"),
                call: str_field(item, "call"),
                expected: str_field(item, "expected"),
                input_sha256: str_field(item, "input_sha256"),
                call_sha256: str_field(item, "call_sha256"),
                expected_sha256: str_field(item, "expected_sha256"),
                python_callables,
            }
        })
        .collect()
}

// ---- comparing -------------------------------------------------------------------------

fn numbers_equal(a: &Number, b: &Number) -> bool {
    match (a.as_i64(), b.as_i64()) {
        (Some(x), Some(y)) => x == y,
        _ => match (a.as_u64(), b.as_u64()) {
            (Some(x), Some(y)) => x == y,
            _ => a.as_f64() == b.as_f64(),
        },
    }
}

fn shown(v: &Value) -> String {
    let text = v.to_string();
    if text.chars().count() > 300 {
        format!("{}...", text.chars().take(300).collect::<String>())
    } else {
        text
    }
}

/// Appends one line per difference between `expected` and `actual`, each with its JSON path.
fn diff(path: &str, expected: &Value, actual: &Value, out: &mut Vec<String>) {
    match (expected, actual) {
        (Value::Number(a), Value::Number(b)) if numbers_equal(a, b) => {}
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                out.push(format!(
                    "{path}: array length expected {}, got {}  (expected {}, got {})",
                    a.len(),
                    b.len(),
                    shown(expected),
                    shown(actual)
                ));
            }
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                diff(&format!("{path}[{i}]"), x, y, out);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for (key, x) in a {
                match b.get(key) {
                    Some(y) => diff(&format!("{path}.{key}"), x, y, out),
                    None => out.push(format!("{path}.{key}: missing, expected {}", shown(x))),
                }
            }
            for (key, y) in b {
                if !a.contains_key(key) {
                    out.push(format!("{path}.{key}: unexpected, got {}", shown(y)));
                }
            }
        }
        (a, b) if a == b => {}
        (a, b) => out.push(format!("{path}: expected {}, got {}", shown(a), shown(b))),
    }
}

/// Checks one callable's outcome against its `expected.json` entry.
fn compare(callable: &str, expected: &Value, actual: &Outcome) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(returned) = expected.get("returns") {
        match actual {
            Ok(value) => diff(callable, returned, value, &mut out),
            Err(variant) => out.push(format!(
                "{callable}: expected a return value {}, got the error {variant}",
                shown(returned)
            )),
        }
    } else if let Some(class) = expected.get("raises").and_then(Value::as_str) {
        match (
            ERROR_VARIANTS.iter().find(|(py, _)| *py == class),
            actual,
        ) {
            (None, _) => out.push(format!(
                "{callable}: Python raised {class} and no Rust variant is mapped to it in ERROR_VARIANTS"
            )),
            (Some((_, want)), Err(got)) if want == got => {}
            (Some((_, want)), Err(got)) => {
                out.push(format!("{callable}: expected the error {want} ({class}), got {got}"));
            }
            (Some((_, want)), Ok(value)) => out.push(format!(
                "{callable}: expected the error {want} ({class}), got the value {}",
                shown(value)
            )),
        }
    } else {
        out.push(format!(
            "{callable}: expected.json entry has neither returns nor raises"
        ));
    }
    out
}

// ---- running ---------------------------------------------------------------------------

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn read_checked(
    root: &Path,
    item: &Item,
    name: &str,
    want: &str,
    out: &mut Vec<String>,
) -> Option<Vec<u8>> {
    let path = root.join(&item.dir).join(name);
    match fs::read(&path) {
        Err(e) => {
            out.push(format!("cannot read {}: {e}", path.display()));
            None
        }
        Ok(bytes) => {
            let got = sha256_hex(&bytes);
            if got != want {
                out.push(format!("{name}: sha256 is {got}, manifest says {want}"));
                return None;
            }
            Some(bytes)
        }
    }
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_else(|| "a panic with no message".to_string())
}

/// Runs every callable of one item. Empty means the item passed.
fn run_item(root: &Path, item: &Item, port: &Port) -> Vec<String> {
    let mut out = Vec::new();
    let input = read_checked(root, item, &item.input, &item.input_sha256, &mut out);
    let call_bytes = read_checked(root, item, &item.call, &item.call_sha256, &mut out);
    let expected_bytes = read_checked(root, item, &item.expected, &item.expected_sha256, &mut out);
    let (Some(input), Some(call_bytes), Some(expected_bytes)) = (input, call_bytes, expected_bytes)
    else {
        return out;
    };
    let (call, expected): (Value, Value) = match (
        serde_json::from_slice(&call_bytes),
        serde_json::from_slice(&expected_bytes),
    ) {
        (Ok(c), Ok(e)) => (c, e),
        (c, e) => {
            return vec![format!(
                "call.json / expected.json do not parse as JSON: {:?} {:?}",
                c.err(),
                e.err()
            )];
        }
    };
    let input = Input { bytes: input };
    let empty = Map::new();
    let extra_args = call["extra_args"].as_array().map_or(&[][..], Vec::as_slice);
    let args = Call {
        input: &input,
        extra_args,
        kwargs: call["kwargs"].as_object().unwrap_or(&empty),
        context: call["context"].as_object().unwrap_or(&empty),
    };

    let callables: Vec<&str> = call["callables"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let results = expected["results"].as_object().unwrap_or(&empty);
    let listed: BTreeSet<&str> = callables.iter().copied().collect();
    let manifest: BTreeSet<&str> = item.python_callables.iter().map(String::as_str).collect();
    let answered: BTreeSet<&str> = results.keys().map(String::as_str).collect();
    if listed != answered || listed != manifest {
        out.push(format!(
            "callables disagree: call.json {listed:?}, expected.json {answered:?}, manifest {manifest:?}"
        ));
    }

    for callable in callables {
        let Some((_, function)) = port.calls.iter().find(|(name, _)| *name == callable) else {
            out.push(format!(
                "{callable}: listed in call.json and the Rust port maps nothing to it"
            ));
            continue;
        };
        let Some(want) = results.get(callable) else {
            continue; // already reported as a disagreement above
        };
        match catch_unwind(AssertUnwindSafe(|| function(&args))) {
            Ok(actual) => out.extend(compare(callable, want, &actual)),
            Err(payload) => out.push(format!(
                "{callable}: the Rust function panicked: {}",
                panic_text(&*payload)
            )),
        }
    }
    out
}

/// `(passed, total)` per unit, and the failing items with their lines.
#[derive(Default)]
struct Report {
    rows: BTreeMap<String, (usize, usize)>,
    failures: Vec<(String, Vec<String>)>,
}

impl Report {
    fn table(&self) -> String {
        let width = self.rows.keys().map(String::len).max().unwrap_or(0);
        self.rows
            .iter()
            .map(|(unit, (passed, total))| format!("  {unit:<width$}  {passed}/{total}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn failure_text(&self) -> String {
        self.failures
            .iter()
            .map(|(id, lines)| format!("FAIL {id}\n    {}", lines.join("\n    ")))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn port_for<'a>(ports: &[&'a Port], item: &Item) -> Option<&'a Port> {
    ports
        .iter()
        .copied()
        .find(|p| p.family == item.family && p.unit == item.unit)
}

/// Runs every item whose unit has a port.
fn run_all(root: &Path, items: &[Item], ports: &[&Port]) -> Report {
    let mut report = Report::default();
    for item in items {
        let Some(port) = port_for(ports, item) else {
            continue;
        };
        let row = report.rows.entry(item.unit.clone()).or_default();
        row.1 += 1;
        let lines = run_item(root, item, port);
        if lines.is_empty() {
            row.0 += 1;
        } else {
            report.failures.push((item.id.clone(), lines));
        }
    }
    // A port that matched no item at all is a typo, not a pass.
    for port in ports {
        report.rows.entry(port.unit.to_string()).or_default();
    }
    report
}

/// The corpus parsers with no Rust port: booru modules, and engine callables.
fn unported(items: &[Item], ports: &[&Port]) -> Vec<String> {
    let all: BTreeSet<String> = items
        .iter()
        .map(|i| format!("{}:{}", i.family, i.unit))
        .collect();
    let done: BTreeSet<String> = ports
        .iter()
        .map(|p| format!("{}:{}", p.family, p.unit))
        .collect();
    all.difference(&done).cloned().collect()
}

// ---- the tests -------------------------------------------------------------------------

#[test]
fn every_ported_parser_matches_its_corpus_items() {
    let root = repo_root();
    let items = load_items(&root);
    let report = run_all(&root, &items, PORTS);
    println!(
        "corpus_diff, passed/total per ported parser:\n{}",
        report.table()
    );
    for (unit, (_, total)) in &report.rows {
        assert!(
            *total > 0,
            "the port {unit:?} matches no corpus item (a typo in `unit`?)"
        );
    }
    assert!(
        report.failures.is_empty(),
        "corpus differential FAILED, passed/total per parser:\n{}\n\n{}",
        report.table(),
        report.failure_text()
    );
}

/// Prints the corpus parsers that have no Rust port yet. Asserts nothing: each Phase 2 ticket
/// shrinks the list. Run with `-- --nocapture` to see it.
#[test]
fn coverage_report_lists_the_parsers_with_no_rust_port() {
    let items = load_items(&repo_root());
    let missing = unported(&items, PORTS);
    let boorus = missing.iter().filter(|u| u.starts_with("boorus:")).count();
    println!(
        "corpus parsers with no Rust port: {} ({} booru modules, {} engine callables)",
        missing.len(),
        boorus,
        missing.len() - boorus
    );
    for unit in &missing {
        println!("  {unit}");
    }
}

// ---- the runner's own tests --------------------------------------------------------------

#[cfg(test)]
mod runner {
    use super::*;
    use serde_json::json;

    fn returns(v: Value) -> Value {
        json!({ "returns": v })
    }

    fn lines(expected: Value, actual: Outcome) -> Vec<String> {
        compare("f", &expected, &actual)
    }

    #[test]
    fn numbers_compare_as_numbers_and_key_order_does_not_matter() {
        let want = returns(json!({"a": 1, "b": [2.5, 1e-5], "c": null}));
        let got = Ok(json!({"c": null, "b": [2.5, 0.00001], "a": 1.0}));
        assert_eq!(lines(want, got), Vec::<String>::new());
    }

    #[test]
    fn a_difference_names_its_path() {
        let got = Ok(json!({"a": [1, {"x": "no"}], "extra": 1}));
        let found = lines(returns(json!({"a": [1, {"x": "yes"}], "gone": 2})), got);
        assert!(
            found
                .iter()
                .any(|l| l.starts_with("f.a[1].x: expected \"yes\", got \"no\"")),
            "{found:?}"
        );
        assert!(
            found.iter().any(|l| l.starts_with("f.gone: missing")),
            "{found:?}"
        );
        assert!(
            found.iter().any(|l| l.starts_with("f.extra: unexpected")),
            "{found:?}"
        );
    }

    #[test]
    fn null_is_not_a_missing_value_and_a_length_is_checked() {
        assert!(!lines(returns(json!(null)), Ok(json!(""))).is_empty());
        assert!(!lines(returns(json!([1, 2])), Ok(json!([1]))).is_empty());
        assert!(!lines(returns(json!([1])), Ok(json!([1, 2]))).is_empty());
        assert!(!lines(returns(json!(1)), Ok(json!(2))).is_empty());
        assert!(!lines(returns(json!("a")), Err("BooruError::Fetch")).is_empty());
    }

    #[test]
    fn raises_passes_only_on_the_mapped_variant() {
        let want = json!({"raises": "BooruContentGoneError"});
        assert!(lines(want.clone(), Err("BooruError::ContentGone")).is_empty());
        assert!(!lines(want.clone(), Err("BooruError::Fetch")).is_empty());
        assert!(!lines(want, Ok(json!(null))).is_empty());
        // A Python class nobody mapped is a failure that says so.
        let found = lines(json!({"raises": "NoSuchError"}), Err("X"));
        assert!(found[0].contains("no Rust variant is mapped"), "{found:?}");
    }

    // A one-item corpus in a temp dir, to prove the runner fails (and does not skip) on the
    // conditions the ticket names.
    struct Fixture {
        dir: tempfile::TempDir,
        item: Item,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().expect("temp dir");
        let item_dir = dir.path().join("corpus/boorus/t/001");
        fs::create_dir_all(&item_dir).unwrap();
        let input = b"BODY";
        let call = br#"{"callables": ["t.a"], "context": {}, "extra_args": ["u"], "kwargs": {}}"#;
        let expected = br#"{"results": {"t.a": {"returns": "BODY"}}}"#;
        fs::write(item_dir.join("input.txt"), input).unwrap();
        fs::write(item_dir.join("call.json"), call).unwrap();
        fs::write(item_dir.join("expected.json"), expected).unwrap();
        let item = Item {
            id: "t-001".into(),
            family: "boorus".into(),
            unit: "t".into(),
            dir: "corpus/boorus/t/001".into(),
            input: "input.txt".into(),
            call: "call.json".into(),
            expected: "expected.json".into(),
            input_sha256: sha256_hex(input),
            call_sha256: sha256_hex(call),
            expected_sha256: sha256_hex(expected),
            python_callables: vec!["t.a".into()],
        };
        Fixture { dir, item }
    }

    static ECHO: Port = Port {
        family: "boorus",
        unit: "t",
        calls: &[("t.a", |c: &Call| ok(c.body()))],
    };

    #[test]
    fn a_passing_item_passes() {
        let f = fixture();
        assert_eq!(run_item(f.dir.path(), &f.item, &ECHO), Vec::<String>::new());
    }

    #[test]
    fn a_missing_input_file_fails() {
        let f = fixture();
        fs::remove_file(f.dir.path().join("corpus/boorus/t/001/input.txt")).unwrap();
        let found = run_item(f.dir.path(), &f.item, &ECHO);
        assert!(found[0].starts_with("cannot read"), "{found:?}");
    }

    #[test]
    fn a_hash_mismatch_fails() {
        let f = fixture();
        fs::write(
            f.dir.path().join("corpus/boorus/t/001/expected.json"),
            b"{}",
        )
        .unwrap();
        let found = run_item(f.dir.path(), &f.item, &ECHO);
        assert!(found[0].contains("sha256 is"), "{found:?}");
    }

    #[test]
    fn a_callable_with_no_mapping_fails() {
        let f = fixture();
        static HALF: Port = Port {
            family: "boorus",
            unit: "t",
            calls: &[],
        };
        let found = run_item(f.dir.path(), &f.item, &HALF);
        assert!(
            found.iter().any(|l| l.contains("maps nothing to it")),
            "{found:?}"
        );
    }

    #[test]
    fn a_panicking_port_fails_the_item_and_names_the_panic() {
        let f = fixture();
        static BOOM: Port = Port {
            family: "boorus",
            unit: "t",
            calls: &[("t.a", |_: &Call| panic!("boom"))],
        };
        let found = run_item(f.dir.path(), &f.item, &BOOM);
        assert!(found[0].contains("panicked: boom"), "{found:?}");
    }

    #[test]
    fn a_wrong_answer_is_reported_per_item_and_per_parser() {
        let f = fixture();
        static WRONG: Port = Port {
            family: "boorus",
            unit: "t",
            calls: &[("t.a", |_: &Call| ok("OTHER"))],
        };
        let report = run_all(f.dir.path(), std::slice::from_ref(&f.item), &[&WRONG]);
        assert_eq!(report.rows["t"], (0, 1));
        assert_eq!(report.failures[0].0, "t-001");
        assert!(
            report
                .failure_text()
                .contains("expected \"BODY\", got \"OTHER\""),
            "{}",
            report.failure_text()
        );
    }

    #[test]
    fn the_coverage_list_is_what_has_no_port() {
        let f = fixture();
        assert_eq!(unported(std::slice::from_ref(&f.item), &[]), ["boorus:t"]);
        assert!(unported(std::slice::from_ref(&f.item), &[&ECHO]).is_empty());
    }
}
