//! The presubmit gate (DAN-2485, DAN-2499): the Python-test walk and the other-docs scan of
//! `docs/*.md`, as tests, so a Phase 6/7 PR cannot reach review with a Python test no doc names.
//! The headless Tab walk is `hatate-gtk`'s `tests/presubmit_tabs.rs` (it needs the window crate).
//!
//! The Python repo is read from a checkout outside this tree, at the commit the port is pinned to
//! (`HATATE_PY_ORACLE=<dir>`). Without it the walk prints a loud skip; with `CI` set a missing or wrong
//! checkout fails the test instead (the same rule as `HATATE_GTK_SMOKE` in `crates/hatate-gtk`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use hatate_doccite::pytests;

const ORACLE_ENV: &str = "HATATE_PY_ORACLE";
const ORACLE_COMMIT: &str = "76c85c4";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn doc_files() -> Vec<PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(repo_root().join("docs"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    files.sort();
    files
}

fn all_docs() -> String {
    doc_files()
        .iter()
        .map(|f| std::fs::read_to_string(f).unwrap() + "\n")
        .collect()
}

/// The oracle checkout, or `None` after a loud skip. Under `CI` a missing or mispinned checkout is a failure.
fn oracle() -> Option<PathBuf> {
    let ci = std::env::var_os("CI").is_some();
    let Some(dir) = std::env::var_os(ORACLE_ENV).map(PathBuf::from) else {
        assert!(
            !ci,
            "CI is set but {ORACLE_ENV} is not: the Python tests at {ORACLE_COMMIT} cannot be walked"
        );
        eprintln!(
            "SKIPPED: {ORACLE_ENV} is not set; the Python-test walk did not run (set it to a checkout of {ORACLE_COMMIT})"
        );
        return None;
    };
    assert!(
        dir.join("tests").is_dir(),
        "{ORACLE_ENV}={} has no tests/ directory",
        dir.display()
    );
    let head = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["rev-parse", "HEAD"])
        .output();
    if let Ok(o) = head {
        let head = String::from_utf8_lossy(&o.stdout).trim().to_string();
        assert!(
            head.starts_with(ORACLE_COMMIT),
            "{ORACLE_ENV} is at {head}, the port is pinned to {ORACLE_COMMIT}"
        );
    }
    Some(dir)
}

/// The `test_...` names in the code spans of `text` (a `Class.test_x` or `mod.Class.test_x` counts as `test_x`).
fn test_names(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, span) in text.split('`').enumerate() {
        if i % 2 == 1 {
            let last = span.rsplit('.').next().unwrap_or(span);
            if last.starts_with("test_")
                && last.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                out.push(last.to_string());
            }
        }
    }
    out
}

const BASELINE: &str = "pytest_baseline.txt";

fn baseline_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(BASELINE)
}

fn doc_texts() -> Vec<(String, String)> {
    doc_files()
        .iter()
        .map(|f| {
            (
                f.file_name().unwrap().to_string_lossy().to_string(),
                std::fs::read_to_string(f).unwrap(),
            )
        })
        .collect()
}

/// Every Python test of `tests/test_*.py` (DAN-2544: the whole suite, with no symbol list) is named in `docs/*.md`,
/// by its own name or by its class (a doc that maps `TestFoo` as a whole maps its tests), or is listed in
/// `pytest_baseline.txt`. The baseline only shrinks: the gate also fails on a listed test a doc now names, on a
/// line that is no test of the oracle, and on a duplicate line.
#[test]
fn every_python_test_is_named_in_the_docs_or_the_baseline() {
    let Some(o) = oracle() else { return };
    let tests = pytests::all_tests(&o).unwrap();
    let baseline = std::fs::read_to_string(baseline_path()).unwrap();
    let r = pytests::check_baseline(&tests, &all_docs(), &baseline);
    eprintln!(
        "python-test walk: {} tests, {} baselined, {} unlisted, {} now mapped, {} stale, {} duplicate",
        tests.len(),
        pytests::baseline_entries(&baseline).len(),
        r.unlisted.len(),
        r.now_mapped.len(),
        r.stale.len(),
        r.duplicate.len()
    );
    assert!(
        tests.len() >= 3000,
        "the walk found only {} tests; the parser regressed",
        tests.len()
    );
    assert!(
        r.unlisted.is_empty(),
        "{} Python test(s) are named in no doc and are not in {BASELINE}:\n{}",
        r.unlisted.len(),
        r.unlisted.join("\n")
    );
    assert!(
        r.now_mapped.is_empty(),
        "{} test(s) in {BASELINE} are now named in a doc; delete these lines:\n{}",
        r.now_mapped.len(),
        r.now_mapped.join("\n")
    );
    assert!(
        r.stale.is_empty(),
        "{} line(s) of {BASELINE} are no test of the oracle at {ORACLE_COMMIT}:\n{}",
        r.stale.len(),
        r.stale.join("\n")
    );
    assert!(
        r.duplicate.is_empty(),
        "{} line(s) of {BASELINE} appear more than once:\n{}",
        r.duplicate.len(),
        r.duplicate.join("\n")
    );
}

/// Regenerates `pytest_baseline.txt` from the oracle and the docs (every unmapped test, a `# reason` header per
/// module). Not a check: run it by hand with `--ignored` only to write the first baseline or to re-derive it.
#[test]
#[ignore = "writes crates/hatate-doccite/pytest_baseline.txt"]
fn write_baseline() {
    let o = oracle().expect("write_baseline needs HATATE_PY_ORACLE");
    let tests = pytests::all_tests(&o).unwrap();
    let text = pytests::render_baseline(&tests, &doc_texts());
    std::fs::write(baseline_path(), &text).unwrap();
    eprintln!(
        "wrote {} entries to {}",
        pytests::baseline_entries(&text).len(),
        baseline_path().display()
    );
}

/// The 7.c modules (DAN-2515): the Python tests that mention `parser_probe`, `crashlog` or `version` must be named
/// in `docs/phase7-7c.md` itself, by their own name or by their class. 7.c's walk was missing from the first gate, which
/// only walked the `gui/` symbol list, and nine `crashlog`/`version` tests reached review in no doc at all (DAN-2512).
const PHASE7_C_SYMBOLS: &[&str] = &["parser_probe", "crashlog", "version", "__version__"];

#[test]
fn every_python_test_that_touches_a_7c_module_is_named_in_the_7c_doc() {
    let Some(o) = oracle() else { return };
    let symbols: BTreeSet<String> = PHASE7_C_SYMBOLS.iter().map(|s| s.to_string()).collect();
    let tests = pytests::walk_with(&o, &symbols, false).unwrap();
    let docs = all_docs();
    let unmapped: Vec<String> = tests
        .iter()
        .filter(|t| !docs.contains(&t.name))
        .map(|t| {
            format!(
                "{}:{} {} (touches {})",
                t.file,
                t.line,
                t.id,
                t.hits.join(", ")
            )
        })
        .collect();
    eprintln!(
        "7.c walk: {} tests, {} unmapped",
        tests.len(),
        unmapped.len()
    );
    assert!(
        unmapped.is_empty(),
        "{} Python test(s) touch a 7.c module and docs/phase7-7c.md names none of them:\n{}",
        unmapped.len(),
        unmapped.join("\n")
    );
}

/// The other-docs scan (`scan_docs`, DAN-2396): a row of any `docs/*.md` whose disposition says `6.x` and names a
/// Python test hands that test to `docs/gtk-6x.md`, whose "Python test map" section must then name it. A walk
/// by symbol misses these, because a hand-over is made by name (DAN-2394 failed PR #74 for 37 of them).
#[test]
fn every_test_another_doc_hands_to_a_phase_is_in_that_phases_map() {
    let mut owners = 0;
    let mut checked = 0;
    let mut misses = Vec::new();
    for owner in doc_files() {
        let fname = owner.file_name().unwrap().to_string_lossy().to_string();
        let Some(tag) = fname
            .strip_prefix("gtk-6")
            .and_then(|r| r.strip_suffix(".md"))
        else {
            continue;
        };
        let text = std::fs::read_to_string(&owner).unwrap();
        let Some(start) = text
            .lines()
            .position(|l| l.starts_with("## ") && l.contains("Python test map"))
        else {
            continue;
        };
        owners += 1;
        let section: String = text
            .lines()
            .skip(start + 1)
            .take_while(|l| !l.starts_with("## "))
            .map(|l| format!("{l}\n"))
            .collect();
        let have: BTreeSet<String> = test_names(&section).into_iter().collect();
        let needle = format!("6.{tag}");
        // Section 6.1 of the exit check (`phase6-coverage.md`) is where the hand-offs no phase map took up were
        // closed, one disposition each (shot set, differential, pinned, not asserted).
        let exit_check =
            std::fs::read_to_string(repo_root().join("docs/phase6-coverage.md")).unwrap();
        let closed: BTreeSet<String> = test_names(&exit_check).into_iter().collect();
        for other in doc_files() {
            if other == owner {
                continue;
            }
            let ofn = other.file_name().unwrap().to_string_lossy().to_string();
            // `phase6-coverage.md` dispositions every test of the port itself; it hands nothing over.
            if ofn == "phase6-coverage.md" {
                continue;
            }
            let other_text = std::fs::read_to_string(&other).unwrap();
            let own_map = if ofn.starts_with("gtk-6") {
                map_lines(&other_text)
            } else {
                BTreeSet::new()
            };
            for (n, line) in other_text.lines().enumerate() {
                // A row of another gtk-6x.md's own Python test map is that phase's disposition ("not asserted:
                // 6.e's path"), not a hand-over to this one. Earlier docs (queue-view, gtk-shell, ...) hand over.
                if own_map.contains(&n)
                    || !line.trim_start().starts_with('|')
                    || !says(line, &needle)
                {
                    continue;
                }
                // A glob (`test_review_*`) names test modules and a range of phases; it is a family
                // disposition, not a hand-over of the tests listed beside it.
                if line.contains("_*`") {
                    continue;
                }
                for name in test_names(line) {
                    checked += 1;
                    if !have.contains(&name) && !closed.contains(&name) {
                        misses.push(format!("{ofn}:{} {name} (handed to {needle}, in neither {fname} nor phase6-coverage.md)", n + 1));
                    }
                }
            }
        }
    }
    eprintln!(
        "scan_docs: {owners} phase maps, {checked} hand-overs, {} missing",
        misses.len()
    );
    assert!(
        owners >= 5,
        "found only {owners} gtk-6*.md docs with a Python test map"
    );
    assert!(
        misses.is_empty(),
        "{} test(s) handed to a phase are not in its map:\n{}",
        misses.len(),
        misses.join("\n")
    );
}

/// The 0-based line numbers of a doc's "Python test map" section, if it has one.
fn map_lines(text: &str) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    let mut inside = false;
    for (n, l) in text.lines().enumerate() {
        if l.starts_with("## ") {
            inside = l.contains("Python test map");
        }
        if inside {
            out.insert(n);
        }
    }
    out
}

/// `needle` (`6.g`) in `line`, as a word: not part of `16.g` or `6.g2`.
fn says(line: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(i) = line[from..].find(needle) {
        let at = from + i;
        let before = line[..at].chars().next_back();
        let after = line[at + needle.len()..].chars().next();
        let clean_before = !before.is_some_and(|c| c.is_ascii_digit() || c == '.');
        let clean_after = !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        if clean_before && clean_after {
            return true;
        }
        from = at + needle.len();
    }
    false
}
