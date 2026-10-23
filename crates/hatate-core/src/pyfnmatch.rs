//! `fnmatch.fnmatchcase` (CPython 3.14 `Lib/fnmatch.py`), which is what the tag blacklist
//! matches with.
//!
//! Python does not match the pattern itself: `translate` rewrites it into a regular expression
//! and `re` runs that. Where the two disagree with a textbook glob is in the bracket sets, so
//! this ports `translate`'s set handling step for step (the empty-range merge, the `!` and
//! `^` rules, the escapes) and then reads the set text the way `re` does. Everything else is a
//! plain glob: `*` any run (newlines included, `(?s)`), `?` one character, anything else
//! itself. Characters are code points, as in Python.

/// One element of the translated pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Star,
    Any,
    Lit(char),
    /// `(?!)`: an empty set, which never matches.
    Never,
    Set {
        negate: bool,
        items: Vec<(char, char)>,
    },
}

impl Part {
    fn matches(&self, c: char) -> bool {
        match self {
            Part::Star => false,
            Part::Any => true,
            Part::Lit(l) => *l == c,
            Part::Never => false,
            Part::Set { negate, items } => {
                items.iter().any(|&(lo, hi)| lo <= c && c <= hi) != *negate
            }
        }
    }
}

/// `fnmatch.fnmatchcase(name, pat)`.
pub fn fnmatchcase(name: &str, pat: &str) -> bool {
    let parts = translate(pat);
    let name: Vec<char> = name.chars().collect();
    let (mut p, mut n) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < parts.len() {
            if parts[p] == Part::Star {
                star = Some((p, n));
                p += 1;
                continue;
            }
            if parts[p].matches(name[n]) {
                p += 1;
                n += 1;
                continue;
            }
        }
        match star {
            Some((sp, sn)) => {
                star = Some((sp, sn + 1));
                p = sp + 1;
                n = sn + 1;
            }
            None => return false,
        }
    }
    while p < parts.len() && parts[p] == Part::Star {
        p += 1;
    }
    p == parts.len()
}

fn translate(pat: &str) -> Vec<Part> {
    let pat: Vec<char> = pat.chars().collect();
    let n = pat.len();
    let mut parts: Vec<Part> = Vec::new();
    let mut i = 0;
    while i < n {
        let c = pat[i];
        i += 1;
        match c {
            '*' => {
                // Consecutive stars collapse.
                if parts.last() != Some(&Part::Star) {
                    parts.push(Part::Star);
                }
            }
            '?' => parts.push(Part::Any),
            '[' => {
                let mut j = i;
                if j < n && pat[j] == '!' {
                    j += 1;
                }
                if j < n && pat[j] == ']' {
                    j += 1;
                }
                while j < n && pat[j] != ']' {
                    j += 1;
                }
                if j >= n {
                    parts.push(Part::Lit('['));
                } else {
                    parts.push(set_part(&pat, i, j));
                    i = j + 1;
                }
            }
            other => parts.push(Part::Lit(other)),
        }
    }
    parts
}

/// The set `pat[i..j]` (without the brackets), as `translate` turns it into regex text and `re`
/// reads that text back.
fn set_part(pat: &[char], i: usize, j: usize) -> Part {
    let raw: Vec<char> = pat[i..j].to_vec();
    // `stuff` is regex set text: each element is (char, joiner). The one unescaped `-` between
    // two chunks is a range joiner; everything else is a literal, because `translate` escapes
    // backslashes, hyphens and the set operators `&`, `~` and `|`.
    let mut stuff: Vec<(char, bool)> = Vec::new();
    if !raw.contains(&'-') {
        for &c in &raw {
            stuff.push((c, false));
        }
    } else {
        let mut chunks: Vec<Vec<char>> = Vec::new();
        let mut start = i;
        let mut k = if pat[i] == '!' { i + 2 } else { i + 1 };
        loop {
            let found = if k < j {
                pat[k..j].iter().position(|&c| c == '-').map(|p| p + k)
            } else {
                None
            };
            let Some(hyphen) = found else { break };
            chunks.push(pat[start..hyphen].to_vec());
            start = hyphen + 1;
            k = hyphen + 3;
        }
        let tail = &pat[start.min(j)..j];
        if !tail.is_empty() {
            chunks.push(tail.to_vec());
        } else if let Some(last) = chunks.last_mut() {
            last.push('-');
        }
        // Remove empty ranges, which are invalid in a regex.
        let mut k = chunks.len();
        while k > 1 {
            k -= 1;
            let before_last = chunks[k - 1].last().copied();
            let next_first = chunks[k].first().copied();
            if let (Some(a), Some(b)) = (before_last, next_first) {
                if a > b {
                    let mut merged = chunks[k - 1].clone();
                    merged.pop();
                    merged.extend_from_slice(&chunks[k][1..]);
                    chunks[k - 1] = merged;
                    chunks.remove(k);
                }
            }
        }
        for (index, chunk) in chunks.iter().enumerate() {
            if index > 0 {
                stuff.push(('-', true));
            }
            for &c in chunk {
                stuff.push((c, false));
            }
        }
    }
    if stuff.is_empty() {
        return Part::Never;
    }
    if stuff.len() == 1 && stuff[0] == ('!', false) {
        return Part::Any;
    }
    let mut negate = false;
    let mut body: &[(char, bool)] = &stuff;
    if body[0] == ('!', false) {
        negate = true;
        body = &body[1..];
    }
    read_set(negate, body)
}

/// Reads the set body the way `re` does: a range is `lo-hi` with the joiner between two
/// characters, every other character stands for itself.
fn read_set(negate: bool, body: &[(char, bool)]) -> Part {
    let mut items: Vec<(char, char)> = Vec::new();
    let mut index = 0;
    while index < body.len() {
        let (lo, _) = body[index];
        index += 1;
        let joiner = body.get(index).copied() == Some(('-', true)) && index + 1 < body.len();
        if joiner {
            let (hi, _) = body[index + 1];
            index += 2;
            items.push((lo, hi));
        } else {
            items.push((lo, lo));
        }
    }
    Part::Set { negate, items }
}

#[cfg(test)]
mod tests {
    use super::fnmatchcase;

    #[test]
    fn globs_match_like_fnmatch() {
        assert!(fnmatchcase("bad_id", "bad_*"));
        assert!(fnmatchcase("a_request", "*_request"));
        assert!(fnmatchcase("meta:highres", "meta:*"));
        assert!(!fnmatchcase("highres", "meta:*"));
        assert!(fnmatchcase("a\nb", "a?b"));
        assert!(fnmatchcase("a\nb", "a*"));
        assert!(fnmatchcase("", "*"));
        assert!(!fnmatchcase("", "?"));
        assert!(fnmatchcase("ab", "a**b"));
        assert!(fnmatchcase("abc", "*b*"));
        assert!(!fnmatchcase("abc", "*b"));
    }

    #[test]
    fn bracket_sets_follow_translate() {
        assert!(fnmatchcase("b", "[abc]"));
        assert!(!fnmatchcase("d", "[abc]"));
        assert!(fnmatchcase("d", "[!abc]"));
        assert!(fnmatchcase("m", "[a-z]"));
        assert!(!fnmatchcase("M", "[a-z]"));
        // `]` first in a set is a literal.
        assert!(fnmatchcase("]", "[]a]"));
        // No closing bracket: a literal `[`.
        assert!(fnmatchcase("[a", "[a"));
        assert!(fnmatchcase("[!]x", "[!]x"));
        assert!(!fnmatchcase("x", "[!]x"));
        assert!(fnmatchcase("-", "[a\\-]"));
        // A reversed range is dropped rather than an error.
        assert!(!fnmatchcase("m", "[z-a]"));
        assert!(fnmatchcase("c", "[a--c]"));
        // `^` and `[` first are literals.
        assert!(fnmatchcase("^", "[^a]"));
        assert!(fnmatchcase("a", "[^a]"));
        assert!(!fnmatchcase("b", "[^a]"));
    }
}
