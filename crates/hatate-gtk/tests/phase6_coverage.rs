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
        let open = r.home.starts_with("**OPEN**");
        assert!(
            r.pr.contains('#') || (open && r.pr == "OPEN (DAN-2448)"),
            "{at}: no PR"
        );
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
fn every_open_row_is_named_in_section_5_and_nothing_else_is() {
    let (rows, _) = table();
    let doc = read("docs/phase6-coverage.md");
    let five = doc.split("\n## 5. ").nth(1).expect("section 5");
    let five_1 = five.split("\n### 5.2").next().unwrap();
    let open: Vec<&Row> = rows
        .iter()
        .filter(|r| r.home.starts_with("**OPEN**"))
        .collect();
    assert!(
        !open.is_empty(),
        "the open items were closed: delete section 5.1 and this test"
    );
    for r in &open {
        let short = r.name.rsplit('.').next().unwrap();
        assert!(
            five_1.contains(&format!(":{}` `{short}`", r.line)),
            "{}:{} {} is OPEN but not named in section 5.1",
            r.file,
            r.line,
            r.name
        );
    }
    let named = five_1.matches("` `_").count();
    assert_eq!(
        named,
        open.len(),
        "section 5.1 names {named} defs, the table has {} OPEN rows",
        open.len()
    );
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
        let cites: Vec<&str> = cite.split(", ").collect();
        assert!(!cites.is_empty(), "{who}: a pinned row cites an assertion");
        for c in cites {
            let (path, n) = c
                .trim_matches('`')
                .rsplit_once(':')
                .unwrap_or_else(|| panic!("{who}: bad citation {c:?}"));
            let src = read(path);
            let n: usize = n
                .parse()
                .unwrap_or_else(|_| panic!("{who}: bad line in {c:?}"));
            // The cited line is an assertion, or sits in one that opened up to three lines above
            // (a long `assert_eq!` is formatted over several lines).
            let all: Vec<&str> = src.lines().collect();
            assert!(
                n >= 1 && n <= all.len(),
                "{who}: {c} is past the end of the file"
            );
            let window = &all[n.saturating_sub(4)..n];
            assert!(
                window.iter().any(|l| l.contains("assert")
                    || l.contains("matches!")
                    || l.contains("no-such-state")),
                "{who}: {c} is not in an assertion: {window:?}"
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

/// The backticked `path:line` citations in a cell.
fn cites(cell: &str) -> Vec<(String, usize)> {
    cell.split(", ")
        .filter(|c| c.starts_with('`') && c.contains(':'))
        .map(|c| {
            let (path, n) = c.trim_matches('`').rsplit_once(':').unwrap();
            (path.to_string(), n.parse().unwrap_or(0))
        })
        .collect()
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
                !cites(&r[3]).is_empty(),
                "{who}: a pinned row cites an assertion"
            );
        }
        if matches!(kind, "pinned" | "differential" | "mapped elsewhere") {
            assert!(!cites(&r[3]).is_empty(), "{who}: {kind} row cites nothing");
        }
        for (path, n) in cites(&r[3]) {
            let src = read(&path);
            let all: Vec<&str> = src.lines().collect();
            assert!(
                n >= 1 && n <= all.len(),
                "{who}: {path}:{n} is past the end"
            );
            let window = &all[n.saturating_sub(4)..n];
            let ok = path.ends_with(".md")
                || window.iter().any(|l| {
                    l.contains("assert") || l.contains(" fn ") || l.trim_start().starts_with("fn ")
                });
            assert!(
                ok,
                "{who}: {path}:{n} is neither an assertion nor a fn: {window:?}"
            );
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
