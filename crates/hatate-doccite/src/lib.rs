//! The docs cite gate (DAN-2485, DAN-2499).
//!
//! A cite in `docs/*.md` names a Rust place by function, never by line number:
//!
//! (file) (fn name) (fragment), each in its own code span, one space apart, for example the three spans
//! `crates/hatate-gtk/tests/smoke.rs` `fn review_keys` `assert_eq!(rows, 3);`
//!
//! That is three adjacent code spans: the file, `fn <name>`, and a fragment of source text (the
//! trimmed text of the cited line). A cite is good when the file exists, has a `fn` of that name,
//! and the fragment occurs inside that fn's body. A line number drifts every time someone edits
//! above it; a fragment inside a named fn only goes stale when the cited thing really moved.
//!
//! In a markdown table a `|` inside the fragment is written `\|`; [`parse_cites`] undoes that.
//! Test and tooling code only: no shipped crate depends on this one except as a dev-dependency.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;
use syn::visit::Visit;

/// One `fn` in a source file: its name and its 1-based inclusive line range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnSpan {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

struct Collect(Vec<FnSpan>);

impl Collect {
    fn push(&mut self, name: &syn::Ident, whole: proc_macro2::Span) {
        self.0.push(FnSpan {
            name: name.to_string(),
            start: whole.start().line,
            end: whole.end().line,
        });
    }
}

impl<'a> Visit<'a> for Collect {
    fn visit_item_fn(&mut self, f: &'a syn::ItemFn) {
        self.push(&f.sig.ident, f.span());
        syn::visit::visit_item_fn(self, f);
    }
    fn visit_impl_item_fn(&mut self, f: &'a syn::ImplItemFn) {
        self.push(&f.sig.ident, f.span());
        syn::visit::visit_impl_item_fn(self, f);
    }
    fn visit_trait_item_fn(&mut self, f: &'a syn::TraitItemFn) {
        if f.default.is_some() {
            self.push(&f.sig.ident, f.span());
        }
        syn::visit::visit_trait_item_fn(self, f);
    }
}

/// Every `fn` of `src` that has a body, nested ones included.
pub fn fn_spans(src: &str) -> Result<Vec<FnSpan>, String> {
    let file = syn::parse_file(src).map_err(|e| format!("does not parse: {e}"))?;
    let mut c = Collect(Vec::new());
    c.visit_file(&file);
    Ok(c.0)
}

/// A cite as written in a doc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cite {
    /// The path as written (`crates/hatate-gtk/tests/smoke.rs`, or a suffix of it).
    pub path: String,
    /// The fn name, without the `fn `.
    pub name: String,
    pub fragment: String,
}

/// Every code span of a line: `(start byte, end byte, content)`, the content with the one
/// padding space CommonMark strips.
fn code_spans(line: &str) -> Vec<(usize, usize, String)> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'`' {
            i += 1;
            continue;
        }
        let open = i;
        while i < b.len() && b[i] == b'`' {
            i += 1;
        }
        let n = i - open;
        // the closing run is the next run of exactly n backticks
        let mut j = i;
        let mut close = None;
        while j < b.len() {
            if b[j] != b'`' {
                j += 1;
                continue;
            }
            let s = j;
            while j < b.len() && b[j] == b'`' {
                j += 1;
            }
            if j - s == n {
                close = Some(s);
                break;
            }
        }
        match close {
            Some(s) => {
                let mut c = line[i..s].to_string();
                if c.len() >= 2 && c.starts_with(' ') && c.ends_with(' ') && c.trim() != "" {
                    c = c[1..c.len() - 1].to_string();
                }
                out.push((open, s + n, c));
                i = s + n;
            }
            None => {}
        }
    }
    out
}

fn is_rs_path(s: &str) -> bool {
    s.ends_with(".rs")
        && s.len() > 3
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "./_-".contains(c))
}

fn fn_name(s: &str) -> Option<&str> {
    let n = s.strip_prefix("fn ")?;
    (!n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(n)
}

/// The new-form cites of `text`, line by line: three adjacent code spans, one space apart.
/// `(1-based line, cite)`.
pub fn parse_cites(text: &str) -> Vec<(usize, Cite)> {
    let mut out = Vec::new();
    for (ln, line) in text.lines().enumerate() {
        let table = line.trim_start().starts_with('|');
        let spans = code_spans(line);
        for w in spans.windows(3) {
            let (a, b, c) = (&w[0], &w[1], &w[2]);
            if !is_rs_path(&a.2) || fn_name(&b.2).is_none() {
                continue;
            }
            if line[a.1..b.0] != *" " || line[b.1..c.0] != *" " {
                continue;
            }
            let mut fragment = c.2.clone();
            if table {
                fragment = fragment.replace("\\|", "|");
            }
            out.push((
                ln + 1,
                Cite {
                    path: a.2.clone(),
                    name: fn_name(&b.2).unwrap().to_string(),
                    fragment,
                },
            ));
        }
        // `path.rs` `fn name` with no fragment is an unfinished cite
    }
    out
}

/// A `path.rs:N` cite of the old form: `(1-based line, byte range in the line, path, first line,
/// last line)`. It matches inside a code span or bare.
pub fn legacy_cites(text: &str) -> Vec<LegacyCite> {
    let mut out = Vec::new();
    for (ln, line) in text.lines().enumerate() {
        let b = line.as_bytes();
        let mut i = 0;
        while i < b.len() {
            // a path token: [A-Za-z0-9_/.-]+ ending in ".rs" then ":" digits
            let is_p = |c: u8| c.is_ascii_alphanumeric() || b"./_-".contains(&c);
            if !is_p(b[i]) || (i > 0 && is_p(b[i - 1])) {
                i += 1;
                continue;
            }
            let mut j = i;
            while j < b.len() && is_p(b[j]) {
                j += 1;
            }
            let tok = &line[i..j];
            if tok.ends_with(".rs") && j < b.len() && b[j] == b':' {
                let mut k = j + 1;
                while k < b.len() && b[k].is_ascii_digit() {
                    k += 1;
                }
                if k > j + 1 {
                    let first: usize = line[j + 1..k].parse().unwrap();
                    let mut last = first;
                    let mut end = k;
                    if end + 1 < b.len() && b[end] == b'-' && b[end + 1].is_ascii_digit() {
                        let mut m = end + 1;
                        while m < b.len() && b[m].is_ascii_digit() {
                            m += 1;
                        }
                        last = line[end + 1..m].parse().unwrap();
                        end = m;
                    }
                    out.push(LegacyCite {
                        line: ln + 1,
                        start: i,
                        end,
                        path: tok.to_string(),
                        first,
                        last,
                    });
                    i = end;
                    continue;
                }
            }
            i = j.max(i + 1);
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyCite {
    /// 1-based line of the doc.
    pub line: usize,
    /// Byte range of `path.rs:N[-M]` within that line.
    pub start: usize,
    pub end: usize,
    pub path: String,
    pub first: usize,
    pub last: usize,
}

/// Where the cited Rust files live.
pub struct Tree {
    root: PathBuf,
    files: BTreeSet<String>,
}

impl Tree {
    /// The tree rooted at the repo root: every `.rs` under `crates/` and `xtask/`.
    pub fn new(root: &Path) -> Tree {
        fn walk(dir: &Path, root: &Path, out: &mut BTreeSet<String>) {
            let Ok(rd) = std::fs::read_dir(dir) else {
                return;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if p.file_name().is_some_and(|n| n == "target") {
                        continue;
                    }
                    walk(&p, root, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.insert(
                        p.strip_prefix(root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
        let mut files = BTreeSet::new();
        for d in ["crates", "xtask"] {
            walk(&root.join(d), root, &mut files);
        }
        Tree {
            root: root.to_path_buf(),
            files,
        }
    }

    /// The repo-relative path a cite's path names: itself, `crates/` + it, or the one file that
    /// ends in `/` + it. `Err` says why not (missing or ambiguous).
    pub fn resolve(&self, path: &str) -> Result<String, String> {
        if self.files.contains(path) {
            return Ok(path.to_string());
        }
        let under = format!("crates/{path}");
        if self.files.contains(&under) {
            return Ok(under);
        }
        let tail = format!("/{}", path.strip_prefix("src/").unwrap_or(path));
        let hits: Vec<&String> = self.files.iter().filter(|f| f.ends_with(&tail)).collect();
        match hits.len() {
            0 => Err(format!("{path} is no file under crates/ or xtask/")),
            1 => Ok(hits[0].clone()),
            _ => Err(format!(
                "{path} is ambiguous: {}",
                hits.iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
    }

    /// The 1-based lines of `cite`'s fn bodies that contain its fragment. `Err` when the cite is bad.
    pub fn hits(&self, cite: &Cite) -> Result<Vec<(String, usize)>, String> {
        let rel = self.resolve(&cite.path)?;
        let src = self.read(&rel);
        let spans = fn_spans(&src).map_err(|e| format!("{rel} {e}"))?;
        let lines: Vec<&str> = src.lines().collect();
        let mut named = false;
        let mut hits = Vec::new();
        for s in spans.iter().filter(|s| s.name == cite.name) {
            named = true;
            for n in s.start..=s.end.min(lines.len()) {
                if lines[n - 1].contains(&cite.fragment) {
                    hits.push((rel.clone(), n));
                }
            }
        }
        if !named {
            return Err(format!("{rel} has no fn {}", cite.name));
        }
        if hits.is_empty() {
            return Err(format!(
                "{} is not inside fn {} of {rel}",
                cite.fragment, cite.name
            ));
        }
        Ok(hits)
    }

    /// Check one cite; the message says what is wrong.
    pub fn check(&self, cite: &Cite) -> Result<(), String> {
        self.hits(cite).map(|_| ())
    }

    /// The fn that contains line `n` of `rel` (the innermost one), if any.
    pub fn enclosing_fn(&self, rel: &str, n: usize) -> Result<Option<FnSpan>, String> {
        let spans = fn_spans(&self.read(rel)).map_err(|e| format!("{rel} {e}"))?;
        Ok(spans
            .into_iter()
            .filter(|s| s.start <= n && n <= s.end)
            .min_by_key(|s| s.end - s.start))
    }
}

/// The span text for `cite`: the three code spans, `|` escaped when it goes in a table row.
pub fn render(cite: &Cite, in_table: bool) -> String {
    let span = |s: &str| {
        let longest = s.split(|c| c != '`').map(str::len).max().unwrap_or(0);
        if longest == 0 {
            format!("`{s}`")
        } else {
            let d = "`".repeat(longest + 1);
            format!("{d} {s} {d}")
        }
    };
    let frag = if in_table {
        cite.fragment.replace('|', "\\|")
    } else {
        cite.fragment.clone()
    };
    format!(
        "{} {} {}",
        span(&cite.path),
        span(&format!("fn {}", cite.name)),
        span(&frag)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cite_is_three_adjacent_spans() {
        let c = parse_cites("see `a/b.rs` `fn go` `let x = 1;`, and `c.rs` `fn no`\n");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].1.name, "go");
        assert_eq!(c[0].1.fragment, "let x = 1;");
    }

    #[test]
    fn a_pipe_in_a_table_fragment_is_escaped_and_read_back() {
        let cite = Cite {
            path: "a.rs".into(),
            name: "f".into(),
            fragment: "a | b".into(),
        };
        let row = format!("| 1 | {} |", render(&cite, true));
        assert!(row.contains("a \\| b"));
        assert_eq!(parse_cites(&row)[0].1, cite);
    }

    #[test]
    fn a_backtick_in_a_fragment_gets_a_longer_delimiter() {
        let cite = Cite {
            path: "a.rs".into(),
            name: "f".into(),
            fragment: "/// the `x` field".into(),
        };
        assert_eq!(parse_cites(&render(&cite, false))[0].1, cite);
    }

    #[test]
    fn fn_spans_cover_the_body_and_nested_fns() {
        let src = "fn a() {\n    fn b() {\n        1;\n    }\n}\nstruct S;\nimpl S {\n    fn c(&self) {\n        2;\n    }\n}\n";
        let s = fn_spans(src).unwrap();
        let by = |n: &str| s.iter().find(|f| f.name == n).unwrap().clone();
        assert_eq!((by("a").start, by("a").end), (1, 5));
        assert_eq!((by("b").start, by("b").end), (2, 4));
        assert_eq!((by("c").start, by("c").end), (8, 10));
    }

    #[test]
    fn legacy_cites_are_found_bare_and_in_spans_with_ranges() {
        let l = legacy_cites("x `crates/a/b.rs:12` and smoke.rs:3-9, c.rs:4");
        assert_eq!(l.len(), 3);
        assert_eq!((l[0].first, l[0].last), (12, 12));
        assert_eq!(
            (l[1].path.as_str(), l[1].first, l[1].last),
            ("smoke.rs", 3, 9)
        );
    }
}
