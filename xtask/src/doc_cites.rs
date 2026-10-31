//! The one-shot converter of the old `file.rs:NNN` doc cites (DAN-2499).
//!
//! A cite converts at the tree's current sources: the cited line's trimmed text becomes the
//! fragment and the innermost item around it (a fn, or for a line outside every fn a struct, enum,
//! const or static) the name. A cite that points at closing punctuation moves up to the nearest
//! code line (at most 3 lines, the window the old gate used). A cite whose line is blank, a
//! comment, or in no item had drifted: for those the converter asks `git blame` which commit last
//! wrote the doc line, reads the cited line in the sources at that commit, and proposes that text if
//! it is still inside the same named item. Everything it proposes this way is listed, so a human
//! reads each one; what it cannot place is listed as needing a hand fix.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use std::rc::Rc;

use hatate_doccite::{Cite, FnSpan, LegacyCite, Parsed, Tree, fn_spans, legacy_cites, render};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `doc line -> commit` from `git blame --line-porcelain`.
fn blame(doc: &str) -> HashMap<usize, String> {
    let mut m = HashMap::new();
    let Some(out) = git(&["blame", "--line-porcelain", "--", doc]) else {
        return m;
    };
    for l in out.lines() {
        let mut it = l.split(' ');
        if let (Some(sha), Some(_orig), Some(fin)) = (it.next(), it.next(), it.next()) {
            if sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
                if let Ok(n) = fin.parse::<usize>() {
                    m.entry(n).or_insert_with(|| sha.to_string());
                }
            }
        }
    }
    m
}

enum Why {
    Blank,
    Comment,
    OutsideItem,
    PastEnd,
    Path(String),
}

impl Why {
    fn label(&self) -> String {
        match self {
            Why::Blank => "blank line".into(),
            Why::Comment => "comment line".into(),
            Why::OutsideItem => "in no fn or item".into(),
            Why::PastEnd => "past the end of the file".into(),
            Why::Path(m) => m.clone(),
        }
    }
}

fn trivial(t: &str) -> bool {
    t.chars().all(|ch| "})];,{(".contains(ch))
}

/// The cite for line `first` (1-based) of `src` as `rel`: the item around it and its trimmed text.
/// `moved` is how many lines up a closing-punctuation cite had to go.
fn from_source(rel: &str, parsed: &Parsed, first: usize) -> Result<(Cite, usize), Why> {
    let all: Vec<&str> = parsed.src.lines().collect();
    if first == 0 || first > all.len() {
        return Err(Why::PastEnd);
    }
    let mut at = first;
    while trivial(all[at - 1].trim()) && !all[at - 1].trim().is_empty() && first - at < 3 && at > 1
    {
        at -= 1;
    }
    let text = all[at - 1].trim();
    if text.is_empty() {
        return Err(Why::Blank);
    }
    if text.starts_with("//") {
        return Err(Why::Comment);
    }
    if trivial(text) {
        return Err(Why::Blank);
    }
    let f: &FnSpan = parsed
        .spans
        .iter()
        .filter(|s| s.start <= at && at <= s.end)
        .min_by_key(|s| s.end - s.start)
        .ok_or(Why::OutsideItem)?;
    Ok((
        Cite {
            path: rel.to_string(),
            kind: f.kind.to_string(),
            name: f.name.clone(),
            fragment: text.to_string(),
        },
        first - at,
    ))
}

/// A source file at a commit: the path it had there, parsed. `None` when it is not one file.
type OldSource = Option<(String, Rc<Parsed>)>;

struct Old {
    files: HashMap<String, Vec<String>>,
    parsed: HashMap<(String, String), OldSource>,
}

impl Old {
    /// The source of the file `path` names at commit `sha`, with the path it had there.
    fn source(&mut self, sha: &str, path: &str) -> OldSource {
        let key = (sha.to_string(), path.to_string());
        if let Some(v) = self.parsed.get(&key) {
            return v.clone();
        }
        let v = self.load(sha, path);
        self.parsed.insert(key, v.clone());
        v
    }

    fn load(&mut self, sha: &str, path: &str) -> OldSource {
        let list = self
            .files
            .entry(sha.to_string())
            .or_insert_with(|| {
                git(&["ls-tree", "-r", "--name-only", sha, "--", "crates", "xtask"])
                    .unwrap_or_default()
                    .lines()
                    .map(str::to_string)
                    .collect()
            })
            .clone();
        let tail = format!("/{}", path.strip_prefix("src/").unwrap_or(path));
        let hits: Vec<&String> = list
            .iter()
            .filter(|f| f.as_str() == path || f.ends_with(&tail))
            .collect();
        if hits.len() != 1 {
            return None;
        }
        let rel = hits[0].clone();
        let src = git(&["show", &format!("{sha}:{rel}")])?;
        let spans = fn_spans(&src).ok()?;
        Some((rel, Rc::new(Parsed { src, spans })))
    }
}

pub fn convert(write: bool) -> i32 {
    let root = root();
    let tree = Tree::new(&root);
    let mut old = Old {
        files: HashMap::new(),
        parsed: HashMap::new(),
    };
    let mut docs: Vec<PathBuf> = std::fs::read_dir(root.join("docs"))
        .expect("docs/")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    docs.sort();
    let (mut done, mut ranges, mut non_unique, mut moved_up) = (0usize, 0usize, 0usize, 0usize);
    let (mut drift_same, mut drift_diff, mut drift_none) = (0usize, 0usize, 0usize);
    let mut recovered: Vec<String> = Vec::new();
    let mut moved: Vec<String> = Vec::new();
    let mut drifted: Vec<String> = Vec::new();
    let mut hand: Vec<String> = Vec::new();
    for doc in docs {
        let text = std::fs::read_to_string(&doc).unwrap();
        let name = doc.file_name().unwrap().to_string_lossy().to_string();
        let blamed = blame(&format!("docs/{name}"));
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        let mut cites: Vec<LegacyCite> = legacy_cites(&text);
        // right to left, so a replacement does not move the cites before it
        cites.sort_by_key(|c| (std::cmp::Reverse(c.line), std::cmp::Reverse(c.start)));
        for c in cites {
            let line = lines[c.line - 1].clone();
            let at = format!("docs/{name}:{} `{}:{}`", c.line, c.path, c.first);
            let table = line.trim_start().starts_with('|');
            let rel = match tree.resolve(&c.path) {
                Ok(r) => r,
                Err(m) => {
                    hand.push(format!("{at}: {m}"));
                    continue;
                }
            };
            // the doc line's own commit: what the cited line said when the cite was written
            let then = blamed.get(&c.line).and_then(|sha| {
                let (orel, op) = old.source(sha, &c.path)?;
                Some((sha.clone(), from_source(&orel, &op, c.first)))
            });
            let now = match tree.parsed(&rel) {
                Ok(p) => from_source(&rel, &p, c.first),
                Err(m) => Err(Why::Path(m)),
            };
            // The cite's intent is the line it named when the doc line was written. Where that
            // text is still inside the same named item it wins over the line at base: the base
            // line is then a cite that had drifted and now names something else.
            let mut kept = then.as_ref().and_then(|(sha, r)| match r {
                Ok((tc, _)) => {
                    let tc = Cite {
                        path: rel.clone(),
                        ..tc.clone()
                    };
                    tree.check(&tc).is_ok().then(|| (tc, sha[..8].to_string()))
                }
                Err(_) => None,
            });
            let chosen: Result<(Cite, usize, Option<String>), Why> = match now {
                Ok((cite, up)) => {
                    match &kept {
                        Some((tc, _)) if tc.fragment == cite.fragment => drift_same += 1,
                        Some((tc, sha)) => {
                            drift_diff += 1;
                            drifted.push(format!(
                                "{at}: base `{}` in {} {}; at {sha} `{}` in {} {}",
                                cite.fragment, cite.kind, cite.name, tc.fragment, tc.kind, tc.name
                            ));
                        }
                        None => drift_none += 1,
                    }
                    match kept.take() {
                        Some((tc, _)) if tc.fragment != cite.fragment => Ok((tc, 0, None)),
                        _ => Ok((cite, up, None)),
                    }
                }
                Err(e) => match kept {
                    Some((tc, sha)) => Ok((tc, 0, Some(sha))),
                    None => Err(e),
                },
            };
            match chosen {
                Err(w) => hand.push(format!("{at}: {}", w.label())),
                Ok((cite, up, from)) => {
                    let b = line.as_bytes();
                    let (s, e) =
                        if c.start > 0 && b[c.start - 1] == b'`' && b.get(c.end) == Some(&b'`') {
                            (c.start - 1, c.end + 1)
                        } else {
                            (c.start, c.end)
                        };
                    if (s, e) == (c.start, c.end) && line[..c.start].matches('`').count() % 2 == 1 {
                        hand.push(format!("{at}: sits in a code span with other text"));
                        continue;
                    }
                    if cite.fragment.contains("\\|") {
                        hand.push(format!("{at}: fragment holds a literal \\|"));
                        continue;
                    }
                    let n = tree.hits(&cite).map(|h| h.len()).unwrap_or(0);
                    if n == 0 {
                        hand.push(format!("{at}: converted cite is not found in its item"));
                        continue;
                    }
                    lines[c.line - 1] =
                        format!("{}{}{}", &line[..s], render(&cite, table), &line[e..]);
                    done += 1;
                    if c.last != c.first {
                        ranges += 1;
                    }
                    if n > 1 {
                        non_unique += 1;
                    }
                    if up > 0 {
                        moved_up += 1;
                        moved.push(format!(
                            "{at}: up {up} -> {} `{}`",
                            cite.name, cite.fragment
                        ));
                    }
                    if let Some(sha) = from {
                        recovered.push(format!(
                            "{at}: from {sha} -> {} {} `{}`",
                            cite.kind, cite.name, cite.fragment
                        ));
                    }
                }
            }
        }
        if write {
            let mut out = lines.join("\n");
            if text.ends_with('\n') {
                out.push('\n');
            }
            std::fs::write(&doc, out).unwrap();
        }
    }
    hand.reverse();
    recovered.reverse();
    moved.reverse();
    println!(
        "converted {done} cites ({ranges} were ranges, kept as their first line; {non_unique} fragments occur more than once in their item; {moved_up} moved up off closing punctuation; {} recovered from the doc line's commit)",
        recovered.len()
    );
    println!(
        "drift at the doc line's commit, for the cites good at base: {drift_same} same text, {drift_diff} different text, {drift_none} not comparable"
    );
    println!("{} cites need a hand fix:", hand.len());
    for h in &hand {
        println!("  HAND {h}");
    }
    for r in &recovered {
        println!("  RECOVERED {r}");
    }
    drifted.reverse();
    for d in &drifted {
        println!("  DRIFT {d}");
    }
    for m in &moved {
        println!("  MOVED {m}");
    }
    0
}
