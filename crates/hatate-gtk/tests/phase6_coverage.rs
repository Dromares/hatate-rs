//! The Phase 6 exit check's coverage gate (DAN-2223).
//!
//! `docs/phase6-gui-defs-76c85c4.txt` is the listing of every `def` in `gui/` of `Hatate-Linux-Redux`
//! at `76c85c4` (an `ast` walk, run once outside the repo). `docs/phase6-coverage.md` has one row per
//! `def`. This test fails if a file or a `def` is missing from the table, extra in it, listed twice, on a
//! different line than the listing, or has no Rust home; if a Rust path the table cites does not exist;
//! or if a Phase 6 row of `docs/phase5-coverage.md` does not point at the table. It reads no Python.
//! It needs no display, so it runs the same with and without `HATATE_GTK_SMOKE`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use hatate_doccite::{Cite, Tree, legacy_cites, parse_cites};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// `(file, line, qualified name)` of every row of the listing.
fn listing() -> BTreeSet<(String, u32, String)> {
    read("docs/phase6-gui-defs-76c85c4.txt")
        .lines()
        .map(|l| {
            let (loc, q) = l
                .split_once(' ')
                .unwrap_or_else(|| panic!("bad listing line {l:?}"));
            let (file, line) = loc
                .rsplit_once(':')
                .unwrap_or_else(|| panic!("bad listing line {l:?}"));
            (file.to_string(), line.parse().unwrap(), q.to_string())
        })
        .collect()
}

struct Row {
    file: String,
    line: u32,
    name: String,
    home: String,
    pr: String,
}

/// The `def` rows of section 4, with the file heading each sits under and the count it claims.
fn table() -> (Vec<Row>, BTreeMap<String, usize>) {
    let doc = read("docs/phase6-coverage.md");
    let section = doc
        .split("\n## 4. ")
        .nth(1)
        .expect("section 4")
        .split("\n## 5. ")
        .next()
        .unwrap();
    let (mut rows, mut claimed) = (Vec::new(), BTreeMap::new());
    let mut file = String::new();
    for l in section.lines() {
        if let Some(h) = l.strip_prefix("### `") {
            let (f, rest) = h.split_once("` (").expect("file heading");
            file = f.to_string();
            let n: usize = rest.trim_end_matches(" defs)").parse().expect("def count");
            claimed.insert(file.clone(), n);
        } else if l.starts_with("| ") && l[2..].starts_with(|c: char| c.is_ascii_digit()) {
            let cells: Vec<&str> = l.trim_matches('|').split(" | ").map(str::trim).collect();
            assert_eq!(cells.len(), 5, "row {l:?} does not have five cells");
            rows.push(Row {
                file: file.clone(),
                line: cells[0].parse().unwrap(),
                name: cells[1].trim_matches('`').to_string(),
                home: cells[2].to_string(),
                pr: cells[3].to_string(),
            });
        }
    }
    (rows, claimed)
}

fn rs_files() -> BTreeSet<String> {
    fn walk(dir: &Path, out: &mut BTreeSet<String>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                if e.file_name() != "target" {
                    walk(&p, out);
                }
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.insert(
                    p.strip_prefix(root().join("crates"))
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(&root().join("crates"), &mut out);
    out
}

#[test]
fn every_gui_def_has_exactly_one_row_and_no_row_is_extra() {
    let want = listing();
    let (rows, claimed) = table();
    let got: Vec<(String, u32, String)> = rows
        .iter()
        .map(|r| (r.file.clone(), r.line, r.name.clone()))
        .collect();
    let got_set: BTreeSet<_> = got.iter().cloned().collect();
    assert_eq!(got.len(), got_set.len(), "a def is listed twice");
    let missing: Vec<_> = want.difference(&got_set).collect();
    let extra: Vec<_> = got_set.difference(&want).collect();
    assert!(
        missing.is_empty(),
        "defs of the listing with no row: {missing:?}"
    );
    assert!(
        extra.is_empty(),
        "rows that are not in the listing (wrong name or line): {extra:?}"
    );
    let mut counted: BTreeMap<String, usize> = BTreeMap::new();
    for (f, _, _) in &want {
        *counted.entry(f.clone()).or_default() += 1;
    }
    assert_eq!(
        counted, claimed,
        "a file heading's def count differs from the listing"
    );
}

#[test]
fn every_gui_file_has_a_row_in_section_2() {
    let doc = read("docs/phase6-coverage.md");
    let section = doc
        .split("\n## 2. ")
        .nth(1)
        .expect("section 2")
        .split("\n## 3. ")
        .next()
        .unwrap();
    let files: BTreeSet<String> = listing().into_iter().map(|(f, _, _)| f).collect();
    for f in &files {
        assert!(
            section.contains(&format!("| `{f}` |")),
            "{f} has no row in section 2"
        );
    }
    let rows = section.lines().filter(|l| l.starts_with("| `gui/")).count();
    assert_eq!(
        rows,
        files.len(),
        "section 2 lists a file the listing has not"
    );
}

#[test]
fn every_row_has_a_rust_home_a_pr_and_names_only_files_that_exist() {
    let (rows, _) = table();
    let have = rs_files();
    let base: BTreeSet<&str> = have.iter().map(|p| p.rsplit('/').next().unwrap()).collect();
    for r in &rows {
        let at = format!("{}:{} {}", r.file, r.line, r.name);
        assert!(
            r.home.len() > 8 && !r.home.contains("TBD") && !r.home.contains("TODO"),
            "{at}: no Rust home"
        );
        assert!(
            !r.home.starts_with("**OPEN**"),
            "{at}: an OPEN row: the ones 6.j had left were ported by 6.h-2"
        );
        assert!(r.pr.contains('#'), "{at}: no PR");
        for span in r.home.split('`').skip(1).step_by(2) {
            let head = span.split([' ', ':']).next().unwrap_or("");
            if !head.ends_with(".rs") || head.contains("::") {
                continue;
            }
            if head.contains('/') {
                let path = head.strip_prefix("crates/").unwrap_or(head);
                let tail = path.strip_prefix("src/").unwrap_or(path);
                assert!(
                    have.contains(path) || have.iter().any(|p| p.ends_with(&format!("/{tail}"))),
                    "{at}: cites {head}, which does not exist"
                );
            } else {
                assert!(
                    base.contains(head),
                    "{at}: cites {head}, which is no file under crates/"
                );
            }
        }
    }
}

#[test]
fn the_phase6_rows_of_phase5_coverage_point_at_this_table() {
    let p5 = read("docs/phase5-coverage.md");
    let p6 = read("docs/phase6-coverage.md");
    let section = p6
        .split("\n## 3. ")
        .nth(1)
        .expect("section 3")
        .split("\n## 4. ")
        .next()
        .unwrap();
    let files: Vec<&str> = section
        .lines()
        .filter(|l| l.starts_with("| `"))
        .map(|l| l[3..].split('`').next().unwrap())
        .collect();
    assert_eq!(
        files.len(),
        20,
        "docs/phase5-coverage.md gave Phase 6 twenty files"
    );
    for f in files {
        let row = p5
            .lines()
            .find(|l| l.starts_with(&format!("| `{f}`")))
            .unwrap_or_else(|| panic!("{f} has no row in docs/phase5-coverage.md"));
        assert!(
            row.contains("phase6-coverage.md"),
            "{f}: its row does not point at docs/phase6-coverage.md"
        );
    }
}

#[test]
fn no_menu_action_is_deferred() {
    // 6.c registered unbuilt items as disabled actions owned by a later ticket; 6.j finds none left.
    let left = hatate_gtk::menu::deferred();
    assert!(
        left.is_empty(),
        "disabled placeholder actions still registered: {left:?}"
    );
    let src = read("crates/hatate-gtk/src/menu.rs");
    let markers: Vec<_> = src
        .lines()
        .filter(|l| l.contains("DEFERRED(") && !l.trim_start().starts_with("//!"))
        .collect();
    assert!(
        markers.is_empty(),
        "DEFERRED markers left in menu.rs: {markers:?}"
    );
}

/// The rows of section 6: `(python test file, line, qualified name, disposition kind, text, assertion
/// cell, red-under cell)`.
type TestRow = (String, u32, String, String, String, String, String);

fn section6() -> Vec<TestRow> {
    let doc = read("docs/phase6-coverage.md");
    let section = doc
        .split("\n## 6. ")
        .nth(1)
        .expect("section 6")
        .split("\n### 6.1 ")
        .next()
        .unwrap();
    let mut rows = Vec::new();
    for l in section.lines() {
        if !(l.starts_with("| ") && l[2..].starts_with(|c: char| c.is_ascii_digit())) {
            continue;
        }
        let cells: Vec<&str> = l.trim_matches('|').split(" | ").map(str::trim).collect();
        assert_eq!(cells.len(), 5, "row {l:?} does not have five cells");
        let (file, name) = cells[1]
            .trim_start_matches('`')
            .split_once("` `")
            .unwrap_or_else(|| panic!("bad test cell {:?}", cells[1]));
        let (kind, text) = cells[2]
            .strip_prefix("**")
            .and_then(|r| r.split_once("**: "))
            .unwrap_or_else(|| panic!("row {l:?}: the disposition is not `**kind**: reason`"));
        rows.push((
            file.trim_end_matches(".py").to_string(),
            cells[0].parse().unwrap(),
            name.trim_end_matches('`').to_string(),
            kind.to_string(),
            text.to_string(),
            cells[3].to_string(),
            cells[4].to_string(),
        ));
    }
    rows
}

#[test]
fn every_python_test_no_earlier_doc_named_has_exactly_one_row_in_section_6() {
    let want: BTreeSet<String> = read("docs/phase6-unmapped-tests-76c85c4.txt")
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(want.len(), 50, "the listing holds the 50 tests of run B");
    let rows = section6();
    let got: Vec<String> = rows
        .iter()
        .map(|r| format!("{}.py:{} {}", r.0, r.1, r.2))
        .collect();
    let got_set: BTreeSet<String> = got.iter().cloned().collect();
    assert_eq!(got.len(), got_set.len(), "a test is listed twice");
    let missing: Vec<_> = want.difference(&got_set).collect();
    let extra: Vec<_> = got_set.difference(&want).collect();
    assert!(missing.is_empty(), "tests with no row: {missing:?}");
    assert!(
        extra.is_empty(),
        "rows for tests not in the listing: {extra:?}"
    );
}

#[test]
fn every_row_of_section_6_says_what_it_is_and_a_pinned_row_cites_a_real_assertion() {
    let kinds = ["pinned", "not asserted", "judged in the shot set", "n/a"];
    let tree = Tree::new(&root());
    for (file, line, name, kind, text, cite, red) in section6() {
        let who = format!("{file}.py:{line} {name}");
        assert!(
            kinds.contains(&kind.as_str()),
            "{who}: disposition {kind:?}"
        );
        assert!(text.chars().count() >= 20, "{who}: the reason is a stub");
        if kind != "pinned" {
            continue;
        }
        assert!(
            red != "-" && red.contains('`'),
            "{who}: a pinned row names the mutation that turned it red, and the test"
        );
        let cs = cites(&cite);
        assert!(
            cites_anything(&cite),
            "{who}: a pinned row cites an assertion"
        );
        for c in &cs {
            assert_cite_lands_on(&who, &tree, c, false);
        }
        // a workflow step (`ci.yml`) stays a line: the assertion is on it or up to three lines above
        for (path, n) in doc_line_refs(&cite) {
            let src = read(&path);
            let all: Vec<&str> = src.lines().collect();
            assert!(
                n >= 1 && n <= all.len(),
                "{who}: {path}:{n} is past the end of the file"
            );
            let window = &all[n.saturating_sub(4)..n];
            assert!(
                path.ends_with(".md")
                    || window
                        .iter()
                        .any(|l| l.contains("assert") || l.contains("no-such-state")),
                "{who}: {path}:{n} is not in an assertion: {window:?}"
            );
        }
    }
}

/// The text of `doc` from the heading `from` up to the heading `to` (or the end).
fn between(doc: &str, from: &str, to: &str) -> String {
    let rest = doc
        .split(from)
        .nth(1)
        .unwrap_or_else(|| panic!("no heading {from:?}"));
    rest.split(to).next().unwrap().to_string()
}

/// The rows of a table whose first cell is a number: the cells, split on `" | "`.
fn numbered_rows(section: &str, cells: usize) -> Vec<Vec<String>> {
    section
        .lines()
        .filter(|l| l.starts_with("| ") && l[2..].starts_with(|c: char| c.is_ascii_digit()))
        .map(|l| {
            let c: Vec<String> = l
                .trim_matches('|')
                .split(" | ")
                .map(|x| x.trim().to_string())
                .collect();
            assert_eq!(c.len(), cells, "row {l:?} does not have {cells} cells");
            c
        })
        .collect()
}

/// `file.py:line Class.test` of a row whose test cell is `` `file.py` `Class.test` ``.
fn test_id(line: &str, cell: &str) -> String {
    let (file, name) = cell
        .trim_start_matches('`')
        .split_once("` `")
        .unwrap_or_else(|| panic!("bad test cell {cell:?}"));
    format!("{file}:{line} {}", name.trim_end_matches('`'))
}

/// The Rust cites in a cell: `file.rs` `fn name` `fragment`, three adjacent code spans (DAN-2485).
/// A cell is read as a table row would be, so `\|` in a fragment is a `|`.
fn cites(cell: &str) -> Vec<Cite> {
    parse_cites(&format!("| {cell}"))
        .into_iter()
        .map(|(_, c)| c)
        .collect()
}

/// The `file.md:line`, `file.txt:line` and `file.yml:line` references in a cell. Those are references to
/// another doc's or a workflow's line, not to Rust, and stay line numbers; the gate
/// (`every_rust_cite_in_docs_names_a_fn_and_a_fragment_inside_it`) covers `file.rs` only.
fn doc_line_refs(cell: &str) -> Vec<(String, usize)> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .filter_map(|t| {
            let (path, n) = t.rsplit_once(':')?;
            let ext_ok = [".md", ".txt", ".yml"].iter().any(|e| path.ends_with(e));
            let path_ok = path
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "./_-".contains(c));
            (ext_ok && path_ok && n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty())
                .then(|| (path.to_string(), n.parse().unwrap()))
        })
        .collect()
}

/// True when the cell cites something: a Rust place or a line of another doc.
fn cites_anything(cell: &str) -> bool {
    !cites(cell).is_empty() || !doc_line_refs(cell).is_empty()
}

/// A cite is good (its fragment is inside its named fn of its file) and the line it lands on is an
/// assertion, or sits in one that opened up to three lines above (a long `assert_eq!` is formatted
/// over several lines), or, with `or_fn`, is in or just under a `fn` line.
fn assert_cite_lands_on(who: &str, tree: &Tree, c: &Cite, or_fn: bool) {
    let hits = tree
        .hits(c)
        .unwrap_or_else(|e| panic!("{who}: {} {} {:?}: {e}", c.path, c.name, c.fragment));
    let src = tree.read(&hits[0].0);
    let all: Vec<&str> = src.lines().collect();
    let ok = hits.iter().any(|(_, n)| {
        let window = &all[n.saturating_sub(4)..*n];
        window.iter().any(|l| {
            l.contains("assert")
                || l.contains("matches!")
                || l.contains("no-such-state")
                || (or_fn && (l.contains(" fn ") || l.trim_start().starts_with("fn ")))
        })
    });
    assert!(
        ok,
        "{who}: {} {} {:?} is not in an assertion{}",
        c.path,
        c.name,
        c.fragment,
        if or_fn { " or a fn" } else { "" }
    );
}

#[test]
fn every_handed_off_test_has_exactly_one_row_in_section_6_1() {
    let doc = read("docs/phase6-coverage.md");
    let section = between(&doc, "\n### 6.1 ", "\n### 6.2 ");
    let want: BTreeSet<String> = read("docs/phase6-handoffs-76c85c4.txt")
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        want.len(),
        119,
        "the listing holds the 119 hand-offs of run B2"
    );
    let rows = numbered_rows(&section, 5);
    let got: Vec<String> = rows.iter().map(|r| test_id(&r[0], &r[1])).collect();
    let got_set: BTreeSet<String> = got.iter().cloned().collect();
    assert_eq!(got.len(), got_set.len(), "a test is listed twice");
    let missing: Vec<_> = want.difference(&got_set).collect();
    let extra: Vec<_> = got_set.difference(&want).collect();
    assert!(missing.is_empty(), "hand-offs with no row: {missing:?}");
    assert!(
        extra.is_empty(),
        "rows for tests not in the listing: {extra:?}"
    );
}

#[test]
fn every_row_of_section_6_1_says_what_it_is_and_cites_something_that_exists() {
    let doc = read("docs/phase6-coverage.md");
    let section = between(&doc, "\n### 6.1 ", "\n### 6.2 ");
    let kinds = [
        "pinned",
        "differential",
        "mapped elsewhere",
        "not asserted",
        "judged in the shot set",
        "n/a",
        "scan false positive",
    ];
    let ops = read("crates/hatate-gtk/tests/differential.rs");
    let shots = read("crates/hatate-gtk/examples/shot.rs");
    let tree = Tree::new(&root());
    for r in numbered_rows(&section, 5) {
        let who = test_id(&r[0], &r[1]);
        let (kind, text) = r[2]
            .strip_prefix("**")
            .and_then(|x| x.split_once("**: "))
            .unwrap_or_else(|| panic!("{who}: the disposition is not `**kind**: reason`"));
        assert!(kinds.contains(&kind), "{who}: disposition {kind:?}");
        assert!(text.chars().count() >= 40, "{who}: the reason is a stub");
        assert!(text.contains("[handed to "), "{who}: names no owner");
        // a `shot-family-*` the row names is a state of the renderer's table
        for cell in [text, r[3].as_str()] {
            for fam in cell
                .split("`")
                .skip(1)
                .step_by(2)
                .filter(|t| t.ends_with("-*"))
            {
                let state = fam.trim_end_matches("-*");
                assert!(
                    shots.contains(&format!("(\"{state}\", "))
                        || shots.contains(&format!("(\"{state}-")),
                    "{who}: no shot state {state:?}"
                );
            }
        }
        // an `` `op` op`` the row names is a step of the action differential
        for w in text.split("` op").collect::<Vec<_>>().windows(2) {
            let op = w[0].rsplit('`').next().unwrap();
            assert!(
                ops.contains(&format!("\"{op}\" =>")),
                "{who}: the differential has no op {op:?}"
            );
        }
        if kind == "pinned" {
            assert!(
                r[4] != "-" && r[4].contains('`'),
                "{who}: a pinned row names the mutation that turned it red, and the test"
            );
            assert!(
                cites_anything(&r[3]),
                "{who}: a pinned row cites an assertion"
            );
        }
        if matches!(kind, "pinned" | "differential" | "mapped elsewhere") {
            assert!(cites_anything(&r[3]), "{who}: {kind} row cites nothing");
        }
        for c in cites(&r[3]) {
            assert_cite_lands_on(&who, &tree, &c, true);
        }
        for (path, n) in doc_line_refs(&r[3]) {
            let all = read(&path).lines().count();
            assert!(n >= 1 && n <= all, "{who}: {path}:{n} is past the end");
        }
    }
}

#[test]
fn section_6_2_lists_the_gui_tests_outside_the_phase_6_list_exactly() {
    let doc = read("docs/phase6-coverage.md");
    let section = between(&doc, "\n### 6.2 ", "\n## 7. ");
    let want: BTreeSet<String> = read("docs/phase6-gui-tests-outside-list-76c85c4.txt")
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(want.len(), 86, "the listing holds the 86 tests of DAN-2452");
    let mut got = BTreeSet::new();
    let mut n = 0;
    for l in section.lines().filter(|l| l.starts_with("| `test_")) {
        let c: Vec<&str> = l.trim_matches('|').split(" | ").map(str::trim).collect();
        assert_eq!(c.len(), 5, "row {l:?} does not have five cells");
        let file = c[0].trim_matches('`');
        let class = c[2].trim_matches('*');
        assert!(
            ["design: shot set", "behaviour: unmapped, Phase 7"].contains(&class),
            "{file}: class {class:?}"
        );
        let ids: Vec<&str> = c[4].split(", ").map(|x| x.trim_matches('`')).collect();
        assert_eq!(
            c[1].parse::<usize>().unwrap(),
            ids.len(),
            "{file}: the count"
        );
        for id in ids {
            n += 1;
            let key = want
                .iter()
                .find(|w| w.starts_with(&format!("{file}:")) && w.ends_with(&format!(" {id}")))
                .unwrap_or_else(|| panic!("{file}: {id} is not in the listing"));
            assert!(got.insert(key.clone()), "{key} is listed twice");
        }
    }
    assert_eq!(n, want.len());
    let missing: Vec<_> = want.difference(&got).collect();
    assert!(missing.is_empty(), "tests with no row: {missing:?}");
    // a file that makes a behaviour claim is never classed as a design-gate file (Cloud, DAN-2452)
    for f in [
        "test_mcp_bridge",
        "test_tristate_restore",
        "test_unclean_shutdown_wiring",
    ] {
        let row = section
            .lines()
            .find(|l| l.starts_with(&format!("| `{f}.py`")))
            .unwrap_or_else(|| panic!("no row for {f}"));
        assert!(
            row.contains("**behaviour: unmapped, Phase 7**"),
            "{f}: {row}"
        );
    }
}

/// Every `` `op` op``, shot family (`` `x-*` ``) and `path:line` a cell names exists.
fn check_cell_refs(who: &str, tree: &Tree, cell: &str, ops: &str, shots: &str) {
    for w in cell.split("` op").collect::<Vec<_>>().windows(2) {
        let op = w[0].rsplit('`').next().unwrap();
        assert!(
            ops.contains(&format!("\"{op}\"")),
            "{who}: the differential has no op {op:?}"
        );
    }
    for t in cell.split('`').skip(1).step_by(2) {
        if let Some(state) = t.strip_suffix("-*") {
            assert!(
                shots.contains(&format!("(\"{state}\", "))
                    || shots.contains(&format!("(\"{state}-")),
                "{who}: no shot state {state:?}"
            );
        }
        if let Some(state) = t.strip_suffix("-dark.png") {
            assert!(
                shots.contains(&format!("(\"{state}\", ")),
                "{who}: no shot state {state:?}"
            );
        }
    }
    for c in cites(cell) {
        assert_cite_lands_on(who, tree, &c, true);
    }
    for (path, n) in doc_line_refs(cell) {
        let all = read(&path).lines().count();
        assert!(n >= 1 && n <= all, "{who}: {path}:{n} is past the end");
    }
}

#[test]
fn every_candidate_of_the_python_window_has_exactly_one_row_in_section_7_and_a_coverage_that_exists()
 {
    let doc = read("docs/phase6-coverage.md");
    let section = between(&doc, "\n### 7.2 ", "\n## 8. ");
    let want: Vec<String> = read("docs/phase6-feature-inventory-76c85c4.txt")
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        want.len(),
        226,
        "the listing holds the 226 candidates of run B2"
    );
    let ops = read("crates/hatate-gtk/tests/differential.rs");
    let shots = read("crates/hatate-gtk/examples/shot.rs");
    let tree = Tree::new(&root());
    let classes = ["op", "smoke", "unit", "differential", "n/a", "by design"];
    let mut got = Vec::new();
    for r in numbered_rows(&section, 6) {
        let (site, kind) = (r[1].trim_matches('`'), r[2].as_str());
        let who = format!("{site} {kind}");
        got.push(who.clone());
        let (class, why) = r[5]
            .strip_prefix("**")
            .and_then(|x| x.split_once("**: "))
            .unwrap_or_else(|| panic!("{who}: the coverage is not `**class**: reason`"));
        assert!(classes.contains(&class), "{who}: class {class:?}");
        assert!(why.chars().count() >= 20, "{who}: the reason is a stub");
        if matches!(class, "op" | "smoke" | "unit" | "differential") {
            assert!(
                cites_anything(&r[5]) || why.contains("op"),
                "{who}: cites nothing"
            );
        }
        check_cell_refs(&who, &tree, &r[5], &ops, &shots);
        // the Rust text cell names a place that exists
        for c in cites(&r[4]) {
            tree.check(&c)
                .unwrap_or_else(|e| panic!("{who}: {} {} {:?}: {e}", c.path, c.name, c.fragment));
        }
    }
    got.sort();
    let mut want = want;
    want.sort();
    assert_eq!(got, want, "section 7.2 and the listing differ");
}

#[test]
fn every_state_row_of_section_7_1_names_shots_and_a_scenario_that_exist() {
    let doc = read("docs/phase6-coverage.md");
    let section = between(&doc, "\n### 7.1 ", "\n### 7.2 ");
    let ops = read("crates/hatate-gtk/tests/differential.rs");
    let shots = read("crates/hatate-gtk/examples/shot.rs");
    let tree = Tree::new(&root());
    let rows = numbered_rows(&section, 5);
    assert!(
        rows.len() >= 18,
        "section 7.1 lists the surfaces of the window"
    );
    for r in rows {
        let who = format!("7.1 {}", r[0]);
        assert!(r[3].contains("-*`"), "{who}: names no shot family");
        assert!(cites_anything(&r[4]), "{who}: names no scenario");
        check_cell_refs(&who, &tree, &r[3], &ops, &shots);
        check_cell_refs(&who, &tree, &r[4], &ops, &shots);
    }
}

#[test]
fn section_8_lists_every_tab_stop_with_the_shot_that_shows_its_indicator() {
    let doc = read("docs/phase6-coverage.md");
    let section = doc.split("\n## 8. ").nth(1).expect("section 8");
    let want: Vec<String> = read("docs/phase6-tab-stops-76c85c4.txt")
        .lines()
        .map(str::to_string)
        .collect();
    assert!(
        want.len() >= 600,
        "the listing holds the stops of every surface"
    );
    let shots = read("crates/hatate-gtk/examples/shot.rs");
    let mut got = Vec::new();
    let mut surfaces = BTreeSet::new();
    for r in numbered_rows(section, 6) {
        let state = r[1].trim_matches('`');
        got.push(format!("{state} {}", r[2]));
        surfaces.insert(state.to_string());
        // each stop names its crop in both schemes, and nothing is left with `NO INDICATOR`
        for scheme in ["dark", "light"] {
            let file = format!("`{state}-{scheme}-tab{}.png`", r[2]);
            assert!(r[5].contains(&file), "{state} stop {}: no {file}", r[2]);
        }
        assert!(
            !r[5].contains("NO INDICATOR"),
            "{state} stop {} has no visible indicator (a finding, to be fixed)",
            r[2]
        );
        // the walk's surfaces are states of the renderer
        assert!(
            shots.contains(&format!("(\"{state}\", ")),
            "{state} is not a state of the renderer"
        );
    }
    assert_eq!(got, want, "section 8 and the listing differ");
    // every surface the walk covers has rows: the walk's own `TAB_STATES` is read, so a surface cannot be
    // left out of the table (the verifier found `failed` missing at 72742d0). `review-picker` has rows too (its 80
    // stops, not judged: a popover the walker cannot reach; section 8 says so).
    let tab_states = shots
        .split("const TAB_STATES: &[&str] = &[")
        .nth(1)
        .and_then(|rest| rest.split("];").next())
        .expect("TAB_STATES in shot.rs");
    let walked: Vec<&str> = tab_states.split('"').skip(1).step_by(2).collect();
    assert!(walked.len() >= 32, "TAB_STATES parsed to {walked:?}");
    for st in &walked {
        assert!(surfaces.contains(*st), "no Tab stops for {st}");
    }
    for st in &surfaces {
        assert!(
            walked.contains(&st.as_str()),
            "{st} has rows but is not in TAB_STATES"
        );
    }
    assert!(
        section.contains(&format!(
            "**{} stops ({} walks, one per scheme) over {} surfaces;",
            got.len(),
            2 * got.len(),
            surfaces.len()
        )),
        "section 8 states a stop and surface count that is not the table's"
    );
}

/// DAN-2485: a docs cite names a function, never a line. Every `docs/*.md` is scanned.
#[test]
fn every_rust_cite_in_docs_names_a_fn_and_a_fragment_inside_it() {
    let tree = Tree::new(&root());
    let mut docs: Vec<PathBuf> = std::fs::read_dir(root().join("docs"))
        .expect("docs/")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    docs.sort();
    assert!(docs.len() >= 20, "docs/ holds the phase docs: {docs:?}");
    let (mut bad, mut checked) = (Vec::new(), 0usize);
    for doc in docs {
        let name = doc.file_name().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(&doc).unwrap();
        for l in legacy_cites(&text) {
            bad.push(format!(
                "docs/{name}:{}: `{}:{}` is a line cite; write `file.rs` `fn name` `fragment`",
                l.line, l.path, l.first
            ));
        }
        for (line, c) in parse_cites(&text) {
            checked += 1;
            if let Err(e) = tree.check(&c) {
                bad.push(format!("docs/{name}:{line}: {e}"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{} bad cites:\n{}",
        bad.len(),
        bad.join("\n")
    );
    assert!(
        checked >= 1500,
        "the docs hold well over 1,500 cites; the scan found {checked}"
    );
}
