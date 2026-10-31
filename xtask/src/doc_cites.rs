//! The one-shot converter of the old `file.rs:NNN` doc cites (DAN-2499).

use std::path::{Path, PathBuf};

use hatate_doccite::{Cite, Tree, legacy_cites, render};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Why a cite cannot be converted mechanically.
enum Hand {
    Blank,
    Comment,
    OutsideFn,
    PastEnd,
    BadPath(String),
    Trivial,
    InexactSpan,
}

impl Hand {
    fn label(&self) -> String {
        match self {
            Hand::Blank => "blank line".into(),
            Hand::Comment => "comment line".into(),
            Hand::OutsideFn => "outside any fn".into(),
            Hand::PastEnd => "past the end of the file".into(),
            Hand::BadPath(m) => m.clone(),
            Hand::Trivial => "line is only closing punctuation".into(),
            Hand::InexactSpan => "cite sits in a code span with other text".into(),
        }
    }
}

pub fn convert(write: bool) -> i32 {
    let root = root();
    let tree = Tree::new(&root);
    let mut docs: Vec<PathBuf> = std::fs::read_dir(root.join("docs"))
        .expect("docs/")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    docs.sort();
    let (mut done, mut ranges, mut non_unique) = (0usize, 0usize, 0usize);
    let mut hand: Vec<String> = Vec::new();
    for doc in docs {
        let text = std::fs::read_to_string(&doc).unwrap();
        let name = doc.file_name().unwrap().to_string_lossy().to_string();
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        let mut cites = legacy_cites(&text);
        // right to left, so a replacement does not move the cites before it
        cites.sort_by_key(|c| (std::cmp::Reverse(c.line), std::cmp::Reverse(c.start)));
        for c in cites {
            let line = lines[c.line - 1].clone();
            let at = format!("docs/{name}:{} `{}:{}`", c.line, c.path, c.first);
            let table = line.trim_start().starts_with('|');
            let one = |lines_of: &[&str]| -> Result<(Cite, bool), Hand> {
                let rel = tree.resolve(&c.path).map_err(Hand::BadPath)?;
                let src = tree.read(&rel);
                let all: Vec<&str> = src.lines().collect();
                let _ = lines_of;
                if c.first == 0 || c.first > all.len() {
                    return Err(Hand::PastEnd);
                }
                let text = all[c.first - 1].trim();
                if text.is_empty() {
                    return Err(Hand::Blank);
                }
                if text.starts_with("//") {
                    return Err(Hand::Comment);
                }
                if text.chars().all(|ch| "})];,{(".contains(ch)) {
                    return Err(Hand::Trivial);
                }
                let f = tree
                    .enclosing_fn(&rel, c.first)
                    .map_err(Hand::BadPath)?
                    .ok_or(Hand::OutsideFn)?;
                let n = all[f.start - 1..f.end]
                    .iter()
                    .filter(|l| l.contains(text))
                    .count();
                Ok((
                    Cite {
                        path: rel,
                        name: f.name,
                        fragment: text.to_string(),
                    },
                    n > 1,
                ))
            };
            match one(&[]) {
                Err(h) => hand.push(format!("{at}: {}", h.label())),
                Ok((cite, dup)) => {
                    // the span the cite sits in: replace it whole, else the bare token
                    let b = line.as_bytes();
                    let (s, e) =
                        if c.start > 0 && b[c.start - 1] == b'`' && b.get(c.end) == Some(&b'`') {
                            (c.start - 1, c.end + 1)
                        } else {
                            (c.start, c.end)
                        };
                    let in_span_with_text = {
                        // inside a code span with other text: an odd number of backticks before
                        let before = line[..c.start].matches('`').count();
                        before % 2 == 1 && (s, e) == (c.start, c.end)
                    };
                    if in_span_with_text {
                        hand.push(format!("{at}: {}", Hand::InexactSpan.label()));
                        continue;
                    }
                    if cite.fragment.contains("\\|") {
                        hand.push(format!("{at}: fragment holds a literal \\|"));
                        continue;
                    }
                    lines[c.line - 1] =
                        format!("{}{}{}", &line[..s], render(&cite, table), &line[e..]);
                    done += 1;
                    if c.last != c.first {
                        ranges += 1;
                    }
                    if dup {
                        non_unique += 1;
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
    println!(
        "converted {done} cites ({ranges} were ranges, kept as their first line; {non_unique} fragments occur more than once in their fn)"
    );
    println!("{} cites need a hand fix:", hand.len());
    for h in &hand {
        println!("  {h}");
    }
    0
}
