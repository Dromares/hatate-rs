//! The Python-test walk of the presubmit gate (DAN-2499, DAN-2485): which tests of the Python repo's
//! `tests/test_*.py` touch a symbol a Phase 6 doc ports, so a doc can be failed for a test it never names.
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
                if let Constant::Str(s) = &c.value {
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

fn body_of(stmts: &[Stmt], line: usize) -> Body {
    let mut c = Collect::default();
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
) -> Result<Vec<PyTest>, String> {
    let module = ast::Suite::parse(src, file).map_err(|e| format!("{file}: {e}"))?;
    let mut funcs: BTreeMap<String, Body> = BTreeMap::new();
    let mut classes: BTreeMap<String, Class> = BTreeMap::new();
    for stmt in &module {
        match stmt {
            Stmt::FunctionDef(f) => {
                funcs.insert(
                    f.name.to_string(),
                    body_of(&f.body, line_of(src, f.range().start())),
                );
            }
            Stmt::ClassDef(c) => {
                let mut methods = BTreeMap::new();
                for m in &c.body {
                    if let Stmt::FunctionDef(f) = m {
                        methods.insert(
                            f.name.to_string(),
                            body_of(&f.body, line_of(src, f.range().start())),
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

/// Every `tests/test_*.py` of the oracle checkout.
pub fn walk(oracle: &Path, symbols: &BTreeSet<String>) -> Result<Vec<PyTest>, String> {
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
        all.extend(walk_source(&name, &src, symbols)?);
    }
    Ok(all)
}

/// The names defined in the oracle's `gui/` (functions, methods, classes, anywhere), minus the ones that are
/// not specific to it: dunders, a name another top-level directory (`core/`, `workers/`, ...) or `main.py` also defines, and a short stoplist of the generic
/// names whose hits were noise in every earlier walk (DAN-2348: `install`, `handlers`, `_load`, ...).
pub fn gui_symbols(oracle: &Path) -> Result<BTreeSet<String>, String> {
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
    fn defs_of(dir: &Path, recurse: bool, out: &mut BTreeSet<String>) -> Result<(), String> {
        let rd = match std::fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => return Ok(()),
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if recurse && p.file_name().is_some_and(|n| n != "__pycache__") {
                    defs_of(&p, recurse, out)?;
                }
            } else if p.extension().is_some_and(|x| x == "py") {
                let src =
                    std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                let module = ast::Suite::parse(&src, &p.to_string_lossy())
                    .map_err(|e| format!("{}: {e}", p.display()))?;
                let mut d = Defs(BTreeSet::new());
                for s in module {
                    d.visit_stmt(s);
                }
                out.extend(d.0);
            }
        }
        Ok(())
    }
    let mut gui = BTreeSet::new();
    defs_of(&oracle.join("gui"), true, &mut gui)?;
    // Every other top-level directory of the checkout (core, workers, ops, ...) and `main.py`: a name defined
    // there too is not specific to the GUI.
    let mut elsewhere = BTreeSet::new();
    if let Ok(rd) = std::fs::read_dir(oracle) {
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            let skip = ["gui", "tests", "docs", "resources"];
            if p.is_dir()
                && !name.starts_with('.')
                && !name.contains("venv")
                && !skip.contains(&name.as_str())
            {
                defs_of(&p, true, &mut elsewhere)?;
            }
        }
    }
    let main = oracle.join("main.py");
    if let Ok(src) = std::fs::read_to_string(&main) {
        if let Ok(module) = ast::Suite::parse(&src, "main.py") {
            let mut d = Defs(BTreeSet::new());
            for s in module {
                d.visit_stmt(s);
            }
            elsewhere.extend(d.0);
        }
    }
    const GENERIC: &[&str] = &[
        "run",
        "stop",
        "start",
        "install",
        "handlers",
        "_load",
        "_on_error",
        "closeEvent",
        "_populate",
        "_selected",
        "main",
        "setup",
        "teardown",
        "get",
        "set",
        "update",
        "clear",
        "reset",
        "show",
        "hide",
        "close",
        "open",
        "save",
        "load",
    ];
    Ok(gui
        .into_iter()
        .filter(|n| !(n.starts_with("__") && n.ends_with("__")))
        .filter(|n| !elsewhere.contains(n) && !GENERIC.contains(&n.as_str()))
        .filter(|n| is_specific(n))
        .collect())
}

/// A name specific enough to mean the GUI when a test mentions it: it has an underscore (`_build_log_card`)
/// or is CamelCase (`ReviewView`). A bare lowercase word (`data`, `warning`, `filter`) is a word in a
/// string literal far more often than a call, and made 100+ of the first walk's 206 hits noise.
fn is_specific(n: &str) -> bool {
    let inner = n.trim_start_matches('_');
    inner.contains('_')
        || (inner.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && inner.chars().any(|c| c.is_ascii_lowercase()))
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
        let t = walk_source("test_x.py", SRC, &s).unwrap();
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
        let t = walk_source("test_x.py", SRC, &s).unwrap();
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
}
