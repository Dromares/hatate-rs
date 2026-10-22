//! Python's `str.casefold()` and the case rules of `re.IGNORECASE`, for the parsers that lean on
//! them (`core/boorus/eshuushuu.py` is the first).
//!
//! Rust's `str::to_lowercase` is not `casefold` (it leaves `ß`, `ς`, `ſ`, the Cherokee small
//! letters and the rest as they are), and the `regex` crate's `(?i)` folds by a different rule from
//! `re.IGNORECASE`. Python's rule is spelled out in CPython's `Modules/_sre` and `Lib/re/_compiler.py`:
//! a pattern character that is cased matches a text character whose lower case is the pattern
//! character's lower case, or one of the characters `re._casefix._EXTRA_CASES` lists for it; an
//! uncased one matches itself. "Lower case" there is the FIRST character of the full lower-case
//! mapping (`Py_UNICODE_TOLOWER`), so `İ` lowers to `i` and `ß` upper-cases to `S`.
//!
//! The two tables below were read off CPython 3.14.8 (Unicode 16.0), the interpreter that
//! recorded the corpus: every code point whose `casefold()` is not its `lower()`, and
//! `_EXTRA_CASES` whole. Everything else is taken from the standard library, so a code point
//! added to Unicode after 16.0 is the one place the two can differ.

use std::sync::LazyLock;

use regex::Regex;

/// Every code point whose `casefold()` differs from its `lower()`, with the `casefold()`.
const CASEFOLD_EXCEPTIONS: [(char, &str); 297] = [
    ('\u{b5}', "\u{3bc}"),
    ('\u{df}', "\u{73}\u{73}"),
    ('\u{149}', "\u{2bc}\u{6e}"),
    ('\u{17f}', "\u{73}"),
    ('\u{1f0}', "\u{6a}\u{30c}"),
    ('\u{345}', "\u{3b9}"),
    ('\u{390}', "\u{3b9}\u{308}\u{301}"),
    ('\u{3b0}', "\u{3c5}\u{308}\u{301}"),
    ('\u{3c2}', "\u{3c3}"),
    ('\u{3d0}', "\u{3b2}"),
    ('\u{3d1}', "\u{3b8}"),
    ('\u{3d5}', "\u{3c6}"),
    ('\u{3d6}', "\u{3c0}"),
    ('\u{3f0}', "\u{3ba}"),
    ('\u{3f1}', "\u{3c1}"),
    ('\u{3f5}', "\u{3b5}"),
    ('\u{587}', "\u{565}\u{582}"),
    ('\u{13a0}', "\u{13a0}"),
    ('\u{13a1}', "\u{13a1}"),
    ('\u{13a2}', "\u{13a2}"),
    ('\u{13a3}', "\u{13a3}"),
    ('\u{13a4}', "\u{13a4}"),
    ('\u{13a5}', "\u{13a5}"),
    ('\u{13a6}', "\u{13a6}"),
    ('\u{13a7}', "\u{13a7}"),
    ('\u{13a8}', "\u{13a8}"),
    ('\u{13a9}', "\u{13a9}"),
    ('\u{13aa}', "\u{13aa}"),
    ('\u{13ab}', "\u{13ab}"),
    ('\u{13ac}', "\u{13ac}"),
    ('\u{13ad}', "\u{13ad}"),
    ('\u{13ae}', "\u{13ae}"),
    ('\u{13af}', "\u{13af}"),
    ('\u{13b0}', "\u{13b0}"),
    ('\u{13b1}', "\u{13b1}"),
    ('\u{13b2}', "\u{13b2}"),
    ('\u{13b3}', "\u{13b3}"),
    ('\u{13b4}', "\u{13b4}"),
    ('\u{13b5}', "\u{13b5}"),
    ('\u{13b6}', "\u{13b6}"),
    ('\u{13b7}', "\u{13b7}"),
    ('\u{13b8}', "\u{13b8}"),
    ('\u{13b9}', "\u{13b9}"),
    ('\u{13ba}', "\u{13ba}"),
    ('\u{13bb}', "\u{13bb}"),
    ('\u{13bc}', "\u{13bc}"),
    ('\u{13bd}', "\u{13bd}"),
    ('\u{13be}', "\u{13be}"),
    ('\u{13bf}', "\u{13bf}"),
    ('\u{13c0}', "\u{13c0}"),
    ('\u{13c1}', "\u{13c1}"),
    ('\u{13c2}', "\u{13c2}"),
    ('\u{13c3}', "\u{13c3}"),
    ('\u{13c4}', "\u{13c4}"),
    ('\u{13c5}', "\u{13c5}"),
    ('\u{13c6}', "\u{13c6}"),
    ('\u{13c7}', "\u{13c7}"),
    ('\u{13c8}', "\u{13c8}"),
    ('\u{13c9}', "\u{13c9}"),
    ('\u{13ca}', "\u{13ca}"),
    ('\u{13cb}', "\u{13cb}"),
    ('\u{13cc}', "\u{13cc}"),
    ('\u{13cd}', "\u{13cd}"),
    ('\u{13ce}', "\u{13ce}"),
    ('\u{13cf}', "\u{13cf}"),
    ('\u{13d0}', "\u{13d0}"),
    ('\u{13d1}', "\u{13d1}"),
    ('\u{13d2}', "\u{13d2}"),
    ('\u{13d3}', "\u{13d3}"),
    ('\u{13d4}', "\u{13d4}"),
    ('\u{13d5}', "\u{13d5}"),
    ('\u{13d6}', "\u{13d6}"),
    ('\u{13d7}', "\u{13d7}"),
    ('\u{13d8}', "\u{13d8}"),
    ('\u{13d9}', "\u{13d9}"),
    ('\u{13da}', "\u{13da}"),
    ('\u{13db}', "\u{13db}"),
    ('\u{13dc}', "\u{13dc}"),
    ('\u{13dd}', "\u{13dd}"),
    ('\u{13de}', "\u{13de}"),
    ('\u{13df}', "\u{13df}"),
    ('\u{13e0}', "\u{13e0}"),
    ('\u{13e1}', "\u{13e1}"),
    ('\u{13e2}', "\u{13e2}"),
    ('\u{13e3}', "\u{13e3}"),
    ('\u{13e4}', "\u{13e4}"),
    ('\u{13e5}', "\u{13e5}"),
    ('\u{13e6}', "\u{13e6}"),
    ('\u{13e7}', "\u{13e7}"),
    ('\u{13e8}', "\u{13e8}"),
    ('\u{13e9}', "\u{13e9}"),
    ('\u{13ea}', "\u{13ea}"),
    ('\u{13eb}', "\u{13eb}"),
    ('\u{13ec}', "\u{13ec}"),
    ('\u{13ed}', "\u{13ed}"),
    ('\u{13ee}', "\u{13ee}"),
    ('\u{13ef}', "\u{13ef}"),
    ('\u{13f0}', "\u{13f0}"),
    ('\u{13f1}', "\u{13f1}"),
    ('\u{13f2}', "\u{13f2}"),
    ('\u{13f3}', "\u{13f3}"),
    ('\u{13f4}', "\u{13f4}"),
    ('\u{13f5}', "\u{13f5}"),
    ('\u{13f8}', "\u{13f0}"),
    ('\u{13f9}', "\u{13f1}"),
    ('\u{13fa}', "\u{13f2}"),
    ('\u{13fb}', "\u{13f3}"),
    ('\u{13fc}', "\u{13f4}"),
    ('\u{13fd}', "\u{13f5}"),
    ('\u{1c80}', "\u{432}"),
    ('\u{1c81}', "\u{434}"),
    ('\u{1c82}', "\u{43e}"),
    ('\u{1c83}', "\u{441}"),
    ('\u{1c84}', "\u{442}"),
    ('\u{1c85}', "\u{442}"),
    ('\u{1c86}', "\u{44a}"),
    ('\u{1c87}', "\u{463}"),
    ('\u{1c88}', "\u{a64b}"),
    ('\u{1e96}', "\u{68}\u{331}"),
    ('\u{1e97}', "\u{74}\u{308}"),
    ('\u{1e98}', "\u{77}\u{30a}"),
    ('\u{1e99}', "\u{79}\u{30a}"),
    ('\u{1e9a}', "\u{61}\u{2be}"),
    ('\u{1e9b}', "\u{1e61}"),
    ('\u{1e9e}', "\u{73}\u{73}"),
    ('\u{1f50}', "\u{3c5}\u{313}"),
    ('\u{1f52}', "\u{3c5}\u{313}\u{300}"),
    ('\u{1f54}', "\u{3c5}\u{313}\u{301}"),
    ('\u{1f56}', "\u{3c5}\u{313}\u{342}"),
    ('\u{1f80}', "\u{1f00}\u{3b9}"),
    ('\u{1f81}', "\u{1f01}\u{3b9}"),
    ('\u{1f82}', "\u{1f02}\u{3b9}"),
    ('\u{1f83}', "\u{1f03}\u{3b9}"),
    ('\u{1f84}', "\u{1f04}\u{3b9}"),
    ('\u{1f85}', "\u{1f05}\u{3b9}"),
    ('\u{1f86}', "\u{1f06}\u{3b9}"),
    ('\u{1f87}', "\u{1f07}\u{3b9}"),
    ('\u{1f88}', "\u{1f00}\u{3b9}"),
    ('\u{1f89}', "\u{1f01}\u{3b9}"),
    ('\u{1f8a}', "\u{1f02}\u{3b9}"),
    ('\u{1f8b}', "\u{1f03}\u{3b9}"),
    ('\u{1f8c}', "\u{1f04}\u{3b9}"),
    ('\u{1f8d}', "\u{1f05}\u{3b9}"),
    ('\u{1f8e}', "\u{1f06}\u{3b9}"),
    ('\u{1f8f}', "\u{1f07}\u{3b9}"),
    ('\u{1f90}', "\u{1f20}\u{3b9}"),
    ('\u{1f91}', "\u{1f21}\u{3b9}"),
    ('\u{1f92}', "\u{1f22}\u{3b9}"),
    ('\u{1f93}', "\u{1f23}\u{3b9}"),
    ('\u{1f94}', "\u{1f24}\u{3b9}"),
    ('\u{1f95}', "\u{1f25}\u{3b9}"),
    ('\u{1f96}', "\u{1f26}\u{3b9}"),
    ('\u{1f97}', "\u{1f27}\u{3b9}"),
    ('\u{1f98}', "\u{1f20}\u{3b9}"),
    ('\u{1f99}', "\u{1f21}\u{3b9}"),
    ('\u{1f9a}', "\u{1f22}\u{3b9}"),
    ('\u{1f9b}', "\u{1f23}\u{3b9}"),
    ('\u{1f9c}', "\u{1f24}\u{3b9}"),
    ('\u{1f9d}', "\u{1f25}\u{3b9}"),
    ('\u{1f9e}', "\u{1f26}\u{3b9}"),
    ('\u{1f9f}', "\u{1f27}\u{3b9}"),
    ('\u{1fa0}', "\u{1f60}\u{3b9}"),
    ('\u{1fa1}', "\u{1f61}\u{3b9}"),
    ('\u{1fa2}', "\u{1f62}\u{3b9}"),
    ('\u{1fa3}', "\u{1f63}\u{3b9}"),
    ('\u{1fa4}', "\u{1f64}\u{3b9}"),
    ('\u{1fa5}', "\u{1f65}\u{3b9}"),
    ('\u{1fa6}', "\u{1f66}\u{3b9}"),
    ('\u{1fa7}', "\u{1f67}\u{3b9}"),
    ('\u{1fa8}', "\u{1f60}\u{3b9}"),
    ('\u{1fa9}', "\u{1f61}\u{3b9}"),
    ('\u{1faa}', "\u{1f62}\u{3b9}"),
    ('\u{1fab}', "\u{1f63}\u{3b9}"),
    ('\u{1fac}', "\u{1f64}\u{3b9}"),
    ('\u{1fad}', "\u{1f65}\u{3b9}"),
    ('\u{1fae}', "\u{1f66}\u{3b9}"),
    ('\u{1faf}', "\u{1f67}\u{3b9}"),
    ('\u{1fb2}', "\u{1f70}\u{3b9}"),
    ('\u{1fb3}', "\u{3b1}\u{3b9}"),
    ('\u{1fb4}', "\u{3ac}\u{3b9}"),
    ('\u{1fb6}', "\u{3b1}\u{342}"),
    ('\u{1fb7}', "\u{3b1}\u{342}\u{3b9}"),
    ('\u{1fbc}', "\u{3b1}\u{3b9}"),
    ('\u{1fbe}', "\u{3b9}"),
    ('\u{1fc2}', "\u{1f74}\u{3b9}"),
    ('\u{1fc3}', "\u{3b7}\u{3b9}"),
    ('\u{1fc4}', "\u{3ae}\u{3b9}"),
    ('\u{1fc6}', "\u{3b7}\u{342}"),
    ('\u{1fc7}', "\u{3b7}\u{342}\u{3b9}"),
    ('\u{1fcc}', "\u{3b7}\u{3b9}"),
    ('\u{1fd2}', "\u{3b9}\u{308}\u{300}"),
    ('\u{1fd3}', "\u{3b9}\u{308}\u{301}"),
    ('\u{1fd6}', "\u{3b9}\u{342}"),
    ('\u{1fd7}', "\u{3b9}\u{308}\u{342}"),
    ('\u{1fe2}', "\u{3c5}\u{308}\u{300}"),
    ('\u{1fe3}', "\u{3c5}\u{308}\u{301}"),
    ('\u{1fe4}', "\u{3c1}\u{313}"),
    ('\u{1fe6}', "\u{3c5}\u{342}"),
    ('\u{1fe7}', "\u{3c5}\u{308}\u{342}"),
    ('\u{1ff2}', "\u{1f7c}\u{3b9}"),
    ('\u{1ff3}', "\u{3c9}\u{3b9}"),
    ('\u{1ff4}', "\u{3ce}\u{3b9}"),
    ('\u{1ff6}', "\u{3c9}\u{342}"),
    ('\u{1ff7}', "\u{3c9}\u{342}\u{3b9}"),
    ('\u{1ffc}', "\u{3c9}\u{3b9}"),
    ('\u{ab70}', "\u{13a0}"),
    ('\u{ab71}', "\u{13a1}"),
    ('\u{ab72}', "\u{13a2}"),
    ('\u{ab73}', "\u{13a3}"),
    ('\u{ab74}', "\u{13a4}"),
    ('\u{ab75}', "\u{13a5}"),
    ('\u{ab76}', "\u{13a6}"),
    ('\u{ab77}', "\u{13a7}"),
    ('\u{ab78}', "\u{13a8}"),
    ('\u{ab79}', "\u{13a9}"),
    ('\u{ab7a}', "\u{13aa}"),
    ('\u{ab7b}', "\u{13ab}"),
    ('\u{ab7c}', "\u{13ac}"),
    ('\u{ab7d}', "\u{13ad}"),
    ('\u{ab7e}', "\u{13ae}"),
    ('\u{ab7f}', "\u{13af}"),
    ('\u{ab80}', "\u{13b0}"),
    ('\u{ab81}', "\u{13b1}"),
    ('\u{ab82}', "\u{13b2}"),
    ('\u{ab83}', "\u{13b3}"),
    ('\u{ab84}', "\u{13b4}"),
    ('\u{ab85}', "\u{13b5}"),
    ('\u{ab86}', "\u{13b6}"),
    ('\u{ab87}', "\u{13b7}"),
    ('\u{ab88}', "\u{13b8}"),
    ('\u{ab89}', "\u{13b9}"),
    ('\u{ab8a}', "\u{13ba}"),
    ('\u{ab8b}', "\u{13bb}"),
    ('\u{ab8c}', "\u{13bc}"),
    ('\u{ab8d}', "\u{13bd}"),
    ('\u{ab8e}', "\u{13be}"),
    ('\u{ab8f}', "\u{13bf}"),
    ('\u{ab90}', "\u{13c0}"),
    ('\u{ab91}', "\u{13c1}"),
    ('\u{ab92}', "\u{13c2}"),
    ('\u{ab93}', "\u{13c3}"),
    ('\u{ab94}', "\u{13c4}"),
    ('\u{ab95}', "\u{13c5}"),
    ('\u{ab96}', "\u{13c6}"),
    ('\u{ab97}', "\u{13c7}"),
    ('\u{ab98}', "\u{13c8}"),
    ('\u{ab99}', "\u{13c9}"),
    ('\u{ab9a}', "\u{13ca}"),
    ('\u{ab9b}', "\u{13cb}"),
    ('\u{ab9c}', "\u{13cc}"),
    ('\u{ab9d}', "\u{13cd}"),
    ('\u{ab9e}', "\u{13ce}"),
    ('\u{ab9f}', "\u{13cf}"),
    ('\u{aba0}', "\u{13d0}"),
    ('\u{aba1}', "\u{13d1}"),
    ('\u{aba2}', "\u{13d2}"),
    ('\u{aba3}', "\u{13d3}"),
    ('\u{aba4}', "\u{13d4}"),
    ('\u{aba5}', "\u{13d5}"),
    ('\u{aba6}', "\u{13d6}"),
    ('\u{aba7}', "\u{13d7}"),
    ('\u{aba8}', "\u{13d8}"),
    ('\u{aba9}', "\u{13d9}"),
    ('\u{abaa}', "\u{13da}"),
    ('\u{abab}', "\u{13db}"),
    ('\u{abac}', "\u{13dc}"),
    ('\u{abad}', "\u{13dd}"),
    ('\u{abae}', "\u{13de}"),
    ('\u{abaf}', "\u{13df}"),
    ('\u{abb0}', "\u{13e0}"),
    ('\u{abb1}', "\u{13e1}"),
    ('\u{abb2}', "\u{13e2}"),
    ('\u{abb3}', "\u{13e3}"),
    ('\u{abb4}', "\u{13e4}"),
    ('\u{abb5}', "\u{13e5}"),
    ('\u{abb6}', "\u{13e6}"),
    ('\u{abb7}', "\u{13e7}"),
    ('\u{abb8}', "\u{13e8}"),
    ('\u{abb9}', "\u{13e9}"),
    ('\u{abba}', "\u{13ea}"),
    ('\u{abbb}', "\u{13eb}"),
    ('\u{abbc}', "\u{13ec}"),
    ('\u{abbd}', "\u{13ed}"),
    ('\u{abbe}', "\u{13ee}"),
    ('\u{abbf}', "\u{13ef}"),
    ('\u{fb00}', "\u{66}\u{66}"),
    ('\u{fb01}', "\u{66}\u{69}"),
    ('\u{fb02}', "\u{66}\u{6c}"),
    ('\u{fb03}', "\u{66}\u{66}\u{69}"),
    ('\u{fb04}', "\u{66}\u{66}\u{6c}"),
    ('\u{fb05}', "\u{73}\u{74}"),
    ('\u{fb06}', "\u{73}\u{74}"),
    ('\u{fb13}', "\u{574}\u{576}"),
    ('\u{fb14}', "\u{574}\u{565}"),
    ('\u{fb15}', "\u{574}\u{56b}"),
    ('\u{fb16}', "\u{57e}\u{576}"),
    ('\u{fb17}', "\u{574}\u{56d}"),
];

/// `re._casefix._EXTRA_CASES`: for a lower-case character, the other characters that
/// `re.IGNORECASE` also lets it match (they are equal under upper-casing, not under lower-casing).
const EXTRA_CASES: [(char, &[char]); 50] = [
    ('\u{69}', &['\u{131}']),
    ('\u{73}', &['\u{17f}']),
    ('\u{b5}', &['\u{3bc}']),
    ('\u{131}', &['\u{69}']),
    ('\u{17f}', &['\u{73}']),
    ('\u{345}', &['\u{3b9}', '\u{1fbe}']),
    ('\u{390}', &['\u{1fd3}']),
    ('\u{3b0}', &['\u{1fe3}']),
    ('\u{3b2}', &['\u{3d0}']),
    ('\u{3b5}', &['\u{3f5}']),
    ('\u{3b8}', &['\u{3d1}']),
    ('\u{3b9}', &['\u{345}', '\u{1fbe}']),
    ('\u{3ba}', &['\u{3f0}']),
    ('\u{3bc}', &['\u{b5}']),
    ('\u{3c0}', &['\u{3d6}']),
    ('\u{3c1}', &['\u{3f1}']),
    ('\u{3c2}', &['\u{3c3}']),
    ('\u{3c3}', &['\u{3c2}']),
    ('\u{3c6}', &['\u{3d5}']),
    ('\u{3d0}', &['\u{3b2}']),
    ('\u{3d1}', &['\u{3b8}']),
    ('\u{3d5}', &['\u{3c6}']),
    ('\u{3d6}', &['\u{3c0}']),
    ('\u{3f0}', &['\u{3ba}']),
    ('\u{3f1}', &['\u{3c1}']),
    ('\u{3f5}', &['\u{3b5}']),
    ('\u{432}', &['\u{1c80}']),
    ('\u{434}', &['\u{1c81}']),
    ('\u{43e}', &['\u{1c82}']),
    ('\u{441}', &['\u{1c83}']),
    ('\u{442}', &['\u{1c84}', '\u{1c85}']),
    ('\u{44a}', &['\u{1c86}']),
    ('\u{463}', &['\u{1c87}']),
    ('\u{1c80}', &['\u{432}']),
    ('\u{1c81}', &['\u{434}']),
    ('\u{1c82}', &['\u{43e}']),
    ('\u{1c83}', &['\u{441}']),
    ('\u{1c84}', &['\u{442}', '\u{1c85}']),
    ('\u{1c85}', &['\u{442}', '\u{1c84}']),
    ('\u{1c86}', &['\u{44a}']),
    ('\u{1c87}', &['\u{463}']),
    ('\u{1c88}', &['\u{a64b}']),
    ('\u{1e61}', &['\u{1e9b}']),
    ('\u{1e9b}', &['\u{1e61}']),
    ('\u{1fbe}', &['\u{345}', '\u{3b9}']),
    ('\u{1fd3}', &['\u{390}']),
    ('\u{1fe3}', &['\u{3b0}']),
    ('\u{a64b}', &['\u{1c88}']),
    ('\u{fb05}', &['\u{fb06}']),
    ('\u{fb06}', &['\u{fb05}']),
];

/// `Py_UNICODE_TOLOWER`: the first character of the full lower-case mapping.
pub fn lower1(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// `Py_UNICODE_TOUPPER`: the first character of the full upper-case mapping.
pub fn upper1(c: char) -> char {
    c.to_uppercase().next().unwrap_or(c)
}

/// `_sre.unicode_iscased`.
fn is_cased(c: char) -> bool {
    c != lower1(c) || c != upper1(c)
}

/// `s.casefold()`.
pub fn casefold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match CASEFOLD_EXCEPTIONS.binary_search_by_key(&c, |(k, _)| *k) {
            Ok(i) => out.push_str(CASEFOLD_EXCEPTIONS[i].1),
            Err(_) => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// Does the pattern character `pattern`, compiled with `re.IGNORECASE`, match `text`?
pub fn ignorecase_matches(pattern: char, text: char) -> bool {
    if !is_cased(pattern) {
        return pattern == text;
    }
    let lo = lower1(pattern);
    let seen = lower1(text);
    seen == lo
        || EXTRA_CASES
            .binary_search_by_key(&lo, |(k, _)| *k)
            .is_ok_and(|i| EXTRA_CASES[i].1.contains(&seen))
}

/// `\w` in a `str` pattern: letters, numbers and `_` (`Py_UNICODE_ISALNUM` or `_`). Not the
/// `regex` crate's `\w`, which also counts combining marks and joiners.
pub fn is_word_char(c: char) -> bool {
    static WORD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}_]$").expect("word char regex"));
    c == '_'
        || c.is_ascii_alphanumeric()
        || (!c.is_ascii() && WORD.is_match(c.encode_utf8(&mut [0; 4])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn casefold_is_not_lowercase() {
        assert_eq!(casefold("Straße"), "strasse");
        assert_eq!(casefold("\u{1e9e}"), "ss");
        assert_eq!(casefold("ΟΔΟΣ"), casefold("οδος"));
        assert_eq!(casefold("\u{17f}"), "s");
        assert_eq!(casefold("\u{131}"), "\u{131}");
        assert_eq!(casefold("\u{130}"), "i\u{307}");
        assert_eq!(casefold("\u{ab70}"), "\u{13a0}");
        assert_eq!(casefold("Sumomo KPA"), "sumomo kpa");
    }

    #[test]
    fn the_tables_are_sorted_for_the_binary_search() {
        assert!(CASEFOLD_EXCEPTIONS.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(EXTRA_CASES.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn ignorecase_follows_the_sre_rule() {
        assert!(ignorecase_matches('b', 'B'));
        assert!(ignorecase_matches('B', 'b'));
        assert!(!ignorecase_matches('b', 'c'));
        // Equal under lower-casing only.
        assert!(ignorecase_matches('k', '\u{212a}'));
        assert!(ignorecase_matches('i', '\u{130}'));
        // In `_EXTRA_CASES`.
        assert!(ignorecase_matches('i', '\u{131}'));
        assert!(ignorecase_matches('s', '\u{17f}'));
        // An uncased character matches itself only.
        assert!(ignorecase_matches(':', ':'));
        assert!(!ignorecase_matches(':', ';'));
        assert!(ignorecase_matches('1', '1'));
    }

    #[test]
    fn word_characters_are_python_alnum_and_underscore() {
        assert!(is_word_char('a') && is_word_char('9') && is_word_char('_'));
        assert!(is_word_char('\u{e9}') && is_word_char('\u{661}') && is_word_char('\u{4e00}'));
        assert!(!is_word_char(' ') && !is_word_char('.') && !is_word_char(':'));
        // A combining mark is a word character to the `regex` crate and not to Python.
        assert!(!is_word_char('\u{301}'));
        assert!(!is_word_char('\u{93e}'));
    }
}
