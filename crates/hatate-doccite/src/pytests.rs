//! The Python-test walks of the presubmit gate. [`all_tests`] and [`check_baseline`] (DAN-2544) cover the whole
//! suite: every test is named in a doc or listed in the shrink-only baseline `pytest_baseline.txt`, with no symbol
//! list to keep. [`walk_with`] (DAN-2499, DAN-2485) is the symbol-filtered walk that 7.c's stricter name-only check
//! still uses: which tests of the Python repo's `tests/test_*.py` touch a given symbol, so a doc can be failed for
//! a test it never names.
//!
//! The walk follows what a test calls, not only its own body (DAN-2356: a body-only walk found 69 hits
//! where the class-helper-aware walk found 193): `setUp` and `setUpClass`, `self.helper()` methods of the
//! class and of its same-file bases, and module-level helper functions, transitively. A test "touches" a
//! symbol when the symbol is a name, an attribute, or a word inside a string literal anywhere in that
//! closure (string literals count because the GUI tests drive widgets by `objectName` and by patch targets).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rustpython_parser::ast::{self, Constant, Expr, Ranged, Stmt, Visitor};
use rustpython_parser::{Parse, text_size::TextSize};

/// One test of the Python repo and the symbols its closure touches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyTest {
    /// `test_x.py`, relative to `tests/`.
    pub file: String,
    pub line: usize,
    /// `Class.test_name` or `test_name`.
    pub id: String,
    /// `test_name` alone: what the docs cite.
    pub name: String,
    /// The symbols (from the set given to [`walk`]) found in the closure.
    pub hits: Vec<String>,
}

/// What one function body mentions and calls.
#[derive(Default, Clone)]
struct Body {
    line: usize,
    names: BTreeSet<String>,
    self_calls: BTreeSet<String>,
    name_calls: BTreeSet<String>,
}

#[derive(Default)]
struct Collect {
    /// Count the words of string literals as mentions.
    strings: bool,
    names: BTreeSet<String>,
    self_calls: BTreeSet<String>,
    name_calls: BTreeSet<String>,
}

fn words(s: &str, into: &mut BTreeSet<String>) {
    let mut cur = String::new();
    for ch in s.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else if !cur.is_empty() {
            if !cur.starts_with(|c: char| c.is_ascii_digit()) {
                into.insert(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
}

impl Visitor for Collect {
    fn visit_expr(&mut self, node: Expr) {
        match &node {
            Expr::Name(n) => {
                self.names.insert(n.id.to_string());
            }
            Expr::Attribute(a) => {
                self.names.insert(a.attr.to_string());
            }
            Expr::Constant(c) => {
                if let (true, Constant::Str(s)) = (self.strings, &c.value) {
                    words(s, &mut self.names);
                }
            }
            Expr::Call(c) => match c.func.as_ref() {
                Expr::Attribute(a) => {
                    if matches!(a.value.as_ref(), Expr::Name(v) if v.id.as_str() == "self" || v.id.as_str() == "cls")
                    {
                        self.self_calls.insert(a.attr.to_string());
                    }
                }
                Expr::Name(n) => {
                    self.name_calls.insert(n.id.to_string());
                }
                _ => {}
            },
            _ => {}
        }
        self.generic_visit_expr(node);
    }
}

fn body_of(stmts: &[Stmt], line: usize, strings: bool) -> Body {
    let mut c = Collect {
        strings,
        ..Collect::default()
    };
    for s in stmts {
        c.visit_stmt(s.clone());
    }
    Body {
        line,
        names: c.names,
        self_calls: c.self_calls,
        name_calls: c.name_calls,
    }
}

fn line_of(src: &str, at: TextSize) -> usize {
    let at = usize::from(at).min(src.len());
    src[..at].bytes().filter(|&b| b == b'\n').count() + 1
}

struct Class {
    bases: Vec<String>,
    methods: BTreeMap<String, Body>,
}

/// The tests of one file with the symbols in `symbols` their closure touches. Tests with no hit are not
/// returned.
pub fn walk_source(
    file: &str,
    src: &str,
    symbols: &BTreeSet<String>,
    strings: bool,
) -> Result<Vec<PyTest>, String> {
    let module = ast::Suite::parse(src, file).map_err(|e| format!("{file}: {e}"))?;
    let mut funcs: BTreeMap<String, Body> = BTreeMap::new();
    let mut classes: BTreeMap<String, Class> = BTreeMap::new();
    for stmt in &module {
        match stmt {
            Stmt::FunctionDef(f) => {
                funcs.insert(
                    f.name.to_string(),
                    body_of(&f.body, line_of(src, f.range().start()), strings),
                );
            }
            Stmt::ClassDef(c) => {
                let mut methods = BTreeMap::new();
                for m in &c.body {
                    if let Stmt::FunctionDef(f) = m {
                        methods.insert(
                            f.name.to_string(),
                            body_of(&f.body, line_of(src, f.range().start()), strings),
                        );
                    }
                }
                let bases = c
                    .bases
                    .iter()
                    .filter_map(|b| match b {
                        Expr::Name(n) => Some(n.id.to_string()),
                        _ => None,
                    })
                    .collect();
                classes.insert(c.name.to_string(), Class { bases, methods });
            }
            _ => {}
        }
    }

    // A method of `class` or of a same-file base.
    fn method<'a>(
        classes: &'a BTreeMap<String, Class>,
        class: &str,
        name: &str,
        depth: u8,
    ) -> Option<&'a Body> {
        let c = classes.get(class)?;
        if let Some(m) = c.methods.get(name) {
            return Some(m);
        }
        if depth == 0 {
            return None;
        }
        c.bases
            .iter()
            .find_map(|b| method(classes, b, name, depth - 1))
    }

    fn closure(
        classes: &BTreeMap<String, Class>,
        funcs: &BTreeMap<String, Body>,
        class: Option<&str>,
        start: &Body,
        out: &mut BTreeSet<String>,
        seen: &mut BTreeSet<String>,
    ) {
        out.extend(start.names.iter().cloned());
        for call in &start.self_calls {
            if let Some(cls) = class {
                let key = format!("{cls}.{call}");
                if let Some(m) = method(classes, cls, call, 4) {
                    if seen.insert(key) {
                        closure(classes, funcs, class, m, out, seen);
                    }
                }
            }
        }
        for call in &start.name_calls {
            if let Some(f) = funcs.get(call) {
                if seen.insert(format!("::{call}")) {
                    closure(classes, funcs, class, f, out, seen);
                }
            }
        }
    }

    let mut tests = Vec::new();
    let mut hit_of = |class: Option<&str>, name: &str, body: &Body| {
        let mut names = BTreeSet::new();
        let mut seen = BTreeSet::new();
        closure(&classes, &funcs, class, body, &mut names, &mut seen);
        if let Some(cls) = class {
            for setup in ["setUp", "setUpClass", "setup_method"] {
                if let Some(m) = method(&classes, cls, setup, 4) {
                    closure(&classes, &funcs, class, m, &mut names, &mut seen);
                }
            }
        }
        let hits: Vec<String> = names.intersection(symbols).cloned().collect();
        if !hits.is_empty() {
            tests.push(PyTest {
                file: file.to_string(),
                line: body.line,
                id: class.map_or(name.to_string(), |c| format!("{c}.{name}")),
                name: name.to_string(),
                hits,
            });
        }
    };
    for (cname, c) in &classes {
        for (mname, m) in &c.methods {
            if mname.starts_with("test") {
                hit_of(Some(cname), mname, m);
            }
        }
    }
    for (fname, f) in &funcs {
        if fname.starts_with("test") {
            hit_of(None, fname, f);
        }
    }
    tests.sort_by(|a, b| (a.line, &a.id).cmp(&(b.line, &b.id)));
    Ok(tests)
}

/// Every `tests/test_*.py` of the oracle checkout, choosing whether the words of string literals count as mentions.
pub fn walk_with(
    oracle: &Path,
    symbols: &BTreeSet<String>,
    strings: bool,
) -> Result<Vec<PyTest>, String> {
    let dir = oracle.join("tests");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|x| x == "py")
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("test_"))
        })
        .collect();
    files.sort();
    let mut all = Vec::new();
    for p in files {
        let src = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        all.extend(walk_source(&name, &src, symbols, strings)?);
    }
    Ok(all)
}

/// Every name defined (function, method, class, anywhere) in the given files of the oracle, unfiltered.
pub fn defs_in(oracle: &Path, files: &[&str]) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    for f in files {
        let p = oracle.join(f);
        let src = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let module = ast::Suite::parse(&src, f).map_err(|e| format!("{f}: {e}"))?;
        let mut d = Defs(BTreeSet::new());
        for s in module {
            d.visit_stmt(s);
        }
        out.extend(d.0);
    }
    Ok(out)
}

/// Collects the names of `def` and `class` statements.
struct Defs(BTreeSet<String>);

impl Visitor for Defs {
    fn visit_stmt_function_def(&mut self, node: ast::StmtFunctionDef) {
        self.0.insert(node.name.to_string());
        self.generic_visit_stmt_function_def(node);
    }
    fn visit_stmt_class_def(&mut self, node: ast::StmtClassDef) {
        self.0.insert(node.name.to_string());
        self.generic_visit_stmt_class_def(node);
    }
}

/// One test the Python repo defines: a `test*` function or a `test*` method of a top-level class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyTestDef {
    /// `test_x.py`, relative to `tests/`.
    pub file: String,
    pub line: usize,
    pub class: Option<String>,
    pub name: String,
}

impl PyTestDef {
    /// `Class.test_name` or `test_name`.
    pub fn id(&self) -> String {
        self.class
            .as_ref()
            .map_or(self.name.clone(), |c| format!("{c}.{}", self.name))
    }

    /// `test_x.py::Class.test_name`: the line of the baseline file.
    pub fn key(&self) -> String {
        format!("{}::{}", self.file, self.id())
    }
}

/// The tests of one file, with no symbol filter.
pub fn tests_in_source(file: &str, src: &str) -> Result<Vec<PyTestDef>, String> {
    let module = ast::Suite::parse(src, file).map_err(|e| format!("{file}: {e}"))?;
    let def = |class: Option<&str>, name: &str, at: TextSize| PyTestDef {
        file: file.to_string(),
        line: line_of(src, at),
        class: class.map(str::to_string),
        name: name.to_string(),
    };
    let mut out = Vec::new();
    for stmt in &module {
        match stmt {
            Stmt::FunctionDef(f) if f.name.starts_with("test") => {
                out.push(def(None, &f.name, f.range().start()));
            }
            Stmt::AsyncFunctionDef(f) if f.name.starts_with("test") => {
                out.push(def(None, &f.name, f.range().start()));
            }
            Stmt::ClassDef(c) => {
                for m in &c.body {
                    match m {
                        Stmt::FunctionDef(f) if f.name.starts_with("test") => {
                            out.push(def(Some(&c.name), &f.name, f.range().start()));
                        }
                        Stmt::AsyncFunctionDef(f) if f.name.starts_with("test") => {
                            out.push(def(Some(&c.name), &f.name, f.range().start()));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Every test of every `tests/test_*.py` of the oracle checkout (DAN-2544: the whole suite, not the tests that
/// touch a listed symbol).
pub fn all_tests(oracle: &Path) -> Result<Vec<PyTestDef>, String> {
    let dir = oracle.join("tests");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|x| x == "py")
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("test_"))
        })
        .collect();
    files.sort();
    let mut all = Vec::new();
    for p in files {
        let src = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        all.extend(tests_in_source(&name, &src)?);
    }
    Ok(all)
}

/// A test is mapped when a doc names it, or names its class as a whole (a doc that maps `TestFoo` maps its tests).
pub fn is_mapped(t: &PyTestDef, docs: &str) -> bool {
    docs.contains(&t.name) || t.class.as_ref().is_some_and(|c| docs.contains(c.as_str()))
}

/// The `file::Class.test_name` lines of a baseline file: blank lines and `#` comment lines are skipped.
pub fn baseline_entries(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

/// What is wrong with a baseline against the oracle's tests and the docs. All four lists empty is a pass.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct BaselineReport {
    /// (a) tests no doc names that the baseline does not list.
    pub unlisted: Vec<String>,
    /// (b) baseline entries a doc now names: delete the line.
    pub now_mapped: Vec<String>,
    /// (c) baseline entries that are no test of the oracle (stale or a typo).
    pub stale: Vec<String>,
    /// (d) baseline entries that appear more than once.
    pub duplicate: Vec<String>,
}

impl BaselineReport {
    pub fn is_clean(&self) -> bool {
        self.unlisted.is_empty()
            && self.now_mapped.is_empty()
            && self.stale.is_empty()
            && self.duplicate.is_empty()
    }
}

/// The shrink-only baseline check (DAN-2544): a test must be named in a doc or listed in the baseline, and a
/// listed test must be unmapped, real and listed once.
pub fn check_baseline(tests: &[PyTestDef], docs: &str, baseline: &str) -> BaselineReport {
    let mut report = BaselineReport::default();
    let mut listed: BTreeSet<&str> = BTreeSet::new();
    for e in baseline_entries(baseline) {
        if !listed.insert(e) {
            report.duplicate.push(e.to_string());
        }
    }
    let mut real: BTreeMap<String, bool> = BTreeMap::new();
    for t in tests {
        let mapped = is_mapped(t, docs);
        real.insert(t.key(), mapped);
        if !mapped && !listed.contains(t.key().as_str()) {
            report
                .unlisted
                .push(format!("{}:{} {}", t.file, t.line, t.id()));
        }
    }
    for e in &listed {
        match real.get(*e) {
            None => report.stale.push(e.to_string()),
            Some(true) => report.now_mapped.push(e.to_string()),
            Some(false) => {}
        }
    }
    report
}

/// The python test modules that test an `ops/` module of the Python repo (`ops/` is not ported, CLAUDE.md).
const OPS_MODULES: &[&str] = &[
    "ancestry_guard",
    "done_issue_sweep",
    "held_issue_sweep",
    "limit_triage",
    "ops_watchdogs",
    "payload_headroom",
    "stranded_routine_tick_sweep",
];

/// The python test modules that test the Python repo's own tooling (`scripts/*.sh`, packaging, the README,
/// the lint gate): not an `ops/` module, not an app module, and nothing the Rust app ports.
const TOOLING_MODULES: &[&str] = &[
    "agent_preflight",
    "appimage_packaging",
    "check_github_identity",
    "ci_local_matrix",
    "no_undefined_names",
    "privacy_audit",
    "readme_layout",
    "run_tests_sh_interpreter_resolution",
    "verify_closed",
];

/// `needle` in `text` as a whole identifier: not a prefix of `test_net_extra`.
fn names_word(text: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(i) = text[from..].find(needle) {
        let end = from + i + needle.len();
        if !text[end..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return true;
        }
        from = end;
    }
    false
}

/// The `# reason` header of one test module: why its tests are in the baseline. An app module names the docs that
/// name the module (`test_x` or `test_x.py`) and says nothing about a doc that does not.
fn module_reason(file: &str, docs: &[(String, String)]) -> String {
    let stem = file.trim_end_matches(".py");
    let short = stem.trim_start_matches("test_");
    if OPS_MODULES.contains(&short) {
        return "`ops/` is not ported (CLAUDE.md)".to_string();
    }
    if TOOLING_MODULES.contains(&short) {
        return "tests the Python repo's own tooling (scripts/, packaging, README, lint), not ported; not an `ops/` module"
            .to_string();
    }
    let naming: Vec<&str> = docs
        .iter()
        .filter(|(_, text)| names_word(text, stem))
        .map(|(n, _)| n.as_str())
        .collect();
    if naming.is_empty() {
        "no doc covers this module; accepted gap, Phase 7".to_string()
    } else {
        format!(
            "a doc names this module (not these tests): {}",
            naming.join(", ")
        )
    }
}

/// The text of the baseline file for the current oracle and docs: every unmapped test, grouped by module with a
/// `# reason` header each. `docs` is `(file name, text)` per `docs/*.md`.
pub fn render_baseline(tests: &[PyTestDef], docs: &[(String, String)]) -> String {
    let all: String = docs.iter().map(|(_, t)| format!("{t}\n")).collect();
    let mut out = String::from(
        "# Python tests no doc names (DAN-2544): a shrink-only baseline for the presubmit gate\n\
         # (crates/hatate-doccite/tests/presubmit.rs). One `file::Class.test_name` per line.\n\
         # A line is deleted when a doc names the test; the gate fails on a baselined test a doc now names,\n\
         # on an unmapped test that is not listed, on a line that is no oracle test, and on a duplicate.\n\
         # Regenerate with `cargo test -p hatate-doccite --test presubmit write_baseline -- --ignored`.\n",
    );
    let mut file = "";
    for t in tests.iter().filter(|t| !is_mapped(t, &all)) {
        if t.file != file {
            file = &t.file;
            out.push_str(&format!("\n# {file}: {}\n", module_reason(file, docs)));
        }
        out.push_str(&t.key());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn syms(s: &[&str]) -> BTreeSet<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    const SRC: &str = r#"
import unittest

def helper(w):
    return w.build_widget()

class Base(unittest.TestCase):
    def base_helper(self):
        return self.w.deep_symbol

class T(Base):
    def setUp(self):
        self.x = SetupSymbol()

    def test_body(self):
        self.assertTrue(body_symbol)

    def test_through_class_helper(self):
        self.base_helper()

    def test_through_module_helper(self):
        helper(self.w)

    def test_in_a_string(self):
        self.find("string_symbol")

    def test_nothing(self):
        pass

def test_free():
    helper(1)
"#;

    #[test]
    fn the_walk_follows_setup_class_helpers_bases_and_module_helpers() {
        let s = syms(&[
            "SetupSymbol",
            "body_symbol",
            "deep_symbol",
            "build_widget",
            "string_symbol",
        ]);
        let t = walk_source("test_x.py", SRC, &s, true).unwrap();
        let by: BTreeMap<_, _> = t.iter().map(|t| (t.id.as_str(), t.hits.clone())).collect();
        // setUp's symbol reaches every test of the class.
        assert_eq!(by["T.test_body"], ["SetupSymbol", "body_symbol"]);
        assert_eq!(
            by["T.test_through_class_helper"],
            ["SetupSymbol", "deep_symbol"]
        );
        assert_eq!(
            by["T.test_through_module_helper"],
            ["SetupSymbol", "build_widget"]
        );
        assert_eq!(by["T.test_in_a_string"], ["SetupSymbol", "string_symbol"]);
        assert_eq!(by["T.test_nothing"], ["SetupSymbol"]);
        assert_eq!(by["test_free"], ["build_widget"]);
    }

    #[test]
    fn a_body_only_walk_would_miss_the_helper_hits() {
        let s = syms(&["deep_symbol", "build_widget"]);
        let t = walk_source("test_x.py", SRC, &s, true).unwrap();
        let ids: Vec<_> = t.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "T.test_through_class_helper",
                "T.test_through_module_helper",
                "test_free"
            ]
        );
    }

    const BASE_SRC: &str = r#"
class TestA:
    def test_one(self): pass
    def test_two(self): pass

def test_free(): pass
"#;

    fn base_tests() -> Vec<PyTestDef> {
        tests_in_source("test_x.py", BASE_SRC).unwrap()
    }

    #[test]
    fn every_test_is_found_with_its_class_and_key() {
        let keys: Vec<_> = base_tests().iter().map(|t| t.key()).collect();
        assert_eq!(
            keys,
            [
                "test_x.py::TestA.test_one",
                "test_x.py::TestA.test_two",
                "test_x.py::test_free"
            ]
        );
    }

    const BASELINE: &str = "# header\n\ntest_x.py::TestA.test_two\ntest_x.py::test_free\n";

    #[test]
    fn a_baseline_that_matches_the_docs_is_clean() {
        // `test_one` is named, `test_two` and `test_free` are baselined.
        let r = check_baseline(&base_tests(), "`test_one`", BASELINE);
        assert_eq!(r, BaselineReport::default());
    }

    #[test]
    fn a_class_named_in_a_doc_maps_its_tests() {
        let r = check_baseline(
            &base_tests(),
            "`TestA` as a whole",
            "test_x.py::test_free\n",
        );
        assert!(r.is_clean(), "{r:?}");
    }

    #[test]
    fn an_unmapped_unlisted_test_fails() {
        let r = check_baseline(&base_tests(), "`test_one`", "test_x.py::test_free\n");
        assert_eq!(r.unlisted, ["test_x.py:4 TestA.test_two"]);
    }

    #[test]
    fn a_baselined_test_a_doc_now_names_fails() {
        let r = check_baseline(&base_tests(), "`test_one` `test_free`", BASELINE);
        assert_eq!(r.now_mapped, ["test_x.py::test_free"]);
    }

    #[test]
    fn a_baseline_line_that_is_no_oracle_test_fails() {
        let b = format!("{BASELINE}test_x.py::TestA.test_gone\n");
        let r = check_baseline(&base_tests(), "`test_one`", &b);
        assert_eq!(r.stale, ["test_x.py::TestA.test_gone"]);
    }

    #[test]
    fn a_duplicate_baseline_line_fails() {
        let b = format!("{BASELINE}test_x.py::test_free\n");
        let r = check_baseline(&base_tests(), "`test_one`", &b);
        assert_eq!(r.duplicate, ["test_x.py::test_free"]);
    }
}
