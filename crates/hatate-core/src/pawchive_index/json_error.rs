//! The text of the `json.JSONDecodeError` CPython 3.14 raises for a body that is not JSON
//! (`requests`' `Response.json()` raises it as a `ValueError` the crawl catches, and the result's
//! `error` carries `str(exc)`): `Expecting value: line 1 column 1 (char 0)`.
//!
//! `serde_json` decides whether a body parses; this only finds Python's words and position for
//! one that does not. It follows `json/decoder.py` and `json/scanner.py` as CPython's C scanner
//! behaves (its messages differ from the pure-Python ones for control characters and escapes).

enum Fail {
    /// `StopIteration(idx)`: no value starts here.
    Stop(usize),
    Err(String, usize),
}

struct Doc {
    s: Vec<char>,
}

fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

impl Doc {
    fn at(&self, i: usize) -> Option<char> {
        self.s.get(i).copied()
    }

    /// `nextchar in ' \t\n\r'`, where the empty string at the end counts as a member.
    fn in_ws(c: Option<char>) -> bool {
        c.is_none_or(is_ws)
    }

    /// `WHITESPACE.match(s, pos).end()`: a `pos` past the end counts as the end.
    fn ws_end(&self, pos: usize) -> usize {
        let mut i = pos.min(self.s.len());
        while self.at(i).is_some_and(is_ws) {
            i += 1;
        }
        i
    }

    fn starts_with(&self, i: usize, word: &str) -> bool {
        word.chars()
            .enumerate()
            .all(|(k, c)| self.at(i + k) == Some(c))
    }

    fn scan_once(&self, idx: usize) -> Result<usize, Fail> {
        let Some(c) = self.at(idx) else {
            return Err(Fail::Stop(idx));
        };
        match c {
            '"' => return self.scan_string(idx + 1),
            '{' => return self.object(idx + 1),
            '[' => return self.array(idx + 1),
            'n' if self.starts_with(idx, "null") => return Ok(idx + 4),
            't' if self.starts_with(idx, "true") => return Ok(idx + 4),
            'f' if self.starts_with(idx, "false") => return Ok(idx + 5),
            _ => {}
        }
        if let Some(end) = self.number(idx) {
            return Ok(end);
        }
        match c {
            'N' if self.starts_with(idx, "NaN") => Ok(idx + 3),
            'I' if self.starts_with(idx, "Infinity") => Ok(idx + 8),
            '-' if self.starts_with(idx, "-Infinity") => Ok(idx + 9),
            _ => Err(Fail::Stop(idx)),
        }
    }

    /// `-?(?:0|[1-9][0-9]*)(\.[0-9]+)?([eE][-+]?[0-9]+)?`
    fn number(&self, idx: usize) -> Option<usize> {
        let digit = |i: usize| self.at(i).is_some_and(|c| c.is_ascii_digit());
        let mut i = idx;
        if self.at(i) == Some('-') {
            i += 1;
        }
        match self.at(i) {
            Some('0') => i += 1,
            Some('1'..='9') => {
                while digit(i) {
                    i += 1;
                }
            }
            _ => return None,
        }
        if self.at(i) == Some('.') && digit(i + 1) {
            i += 1;
            while digit(i) {
                i += 1;
            }
        }
        if matches!(self.at(i), Some('e' | 'E')) {
            let mut j = i + 1;
            if matches!(self.at(j), Some('+' | '-')) {
                j += 1;
            }
            if digit(j) {
                while digit(j) {
                    j += 1;
                }
                i = j;
            }
        }
        Some(i)
    }

    fn scan_string(&self, start: usize) -> Result<usize, Fail> {
        let begin = start - 1;
        let len = self.s.len();
        let mut end = start;
        loop {
            // Up to the next quote, backslash or control character.
            let mut i = end;
            while let Some(c) = self.at(i) {
                if c == '"' || c == '\\' || (c as u32) < 0x20 {
                    break;
                }
                i += 1;
            }
            let Some(term) = self.at(i) else {
                return Err(Fail::Err("Unterminated string starting at".into(), begin));
            };
            end = i + 1;
            if term == '"' {
                return Ok(end);
            }
            if term != '\\' {
                return Err(Fail::Err("Invalid control character at".into(), end - 1));
            }
            let Some(esc) = self.at(end) else {
                return Err(Fail::Err("Unterminated string starting at".into(), begin));
            };
            if esc != 'u' {
                if !matches!(esc, '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't') {
                    return Err(Fail::Err("Invalid \\escape".into(), end - 1));
                }
                end += 1;
                continue;
            }
            // \uXXXX: C needs the four digits and one more character after them.
            let u = end;
            if u + 5 >= len || !self.hex4(u + 1) {
                return Err(Fail::Err("Invalid \\uXXXX escape".into(), u));
            }
            let code = self.hex_value(u + 1);
            end = u + 5;
            if (0xd800..=0xdbff).contains(&code)
                && end + 6 < len
                && self.at(end) == Some('\\')
                && self.at(end + 1) == Some('u')
            {
                if !self.hex4(end + 2) {
                    return Err(Fail::Err("Invalid \\uXXXX escape".into(), end + 1));
                }
                if (0xdc00..=0xdfff).contains(&self.hex_value(end + 2)) {
                    end += 6;
                }
            }
        }
    }

    fn hex4(&self, i: usize) -> bool {
        (0..4).all(|k| self.at(i + k).is_some_and(|c| c.is_ascii_hexdigit()))
    }

    fn hex_value(&self, i: usize) -> u32 {
        (0..4).fold(0, |n, k| n * 16 + self.s[i + k].to_digit(16).unwrap_or(0))
    }

    fn object(&self, start: usize) -> Result<usize, Fail> {
        let mut end = start;
        let mut next = self.at(end);
        if next != Some('"') {
            if Self::in_ws(next) {
                end = self.ws_end(end);
                next = self.at(end);
            }
            if next == Some('}') {
                return Ok(end + 1);
            } else if next != Some('"') {
                return Err(Fail::Err(
                    "Expecting property name enclosed in double quotes".into(),
                    end,
                ));
            }
        }
        end += 1;
        loop {
            end = self.scan_string(end)?;
            if self.at(end) != Some(':') {
                end = self.ws_end(end);
                if self.at(end) != Some(':') {
                    return Err(Fail::Err("Expecting ':' delimiter".into(), end));
                }
            }
            end += 1;
            if self.at(end).is_some_and(is_ws) {
                end += 1;
                if self.at(end).is_some_and(is_ws) {
                    end = self.ws_end(end + 1);
                }
            }
            end = match self.scan_once(end) {
                Ok(e) => e,
                Err(Fail::Stop(i)) => return Err(Fail::Err("Expecting value".into(), i)),
                Err(e) => return Err(e),
            };
            let next = match self.at(end) {
                Some(c) if is_ws(c) => {
                    end = self.ws_end(end + 1);
                    self.at(end)
                }
                other => other,
            };
            end += 1;
            if next == Some('}') {
                return Ok(end);
            } else if next != Some(',') {
                return Err(Fail::Err("Expecting ',' delimiter".into(), end - 1));
            }
            let comma = end - 1;
            end = self.ws_end(end);
            let next = self.at(end);
            end += 1;
            if next != Some('"') {
                if next == Some('}') {
                    return Err(Fail::Err(
                        "Illegal trailing comma before end of object".into(),
                        comma,
                    ));
                }
                return Err(Fail::Err(
                    "Expecting property name enclosed in double quotes".into(),
                    end - 1,
                ));
            }
        }
    }

    fn array(&self, start: usize) -> Result<usize, Fail> {
        let mut end = start;
        let mut next = self.at(end);
        if Self::in_ws(next) {
            end = self.ws_end(end + 1);
            next = self.at(end);
        }
        if next == Some(']') {
            return Ok(end + 1);
        }
        loop {
            end = match self.scan_once(end) {
                Ok(e) => e,
                Err(Fail::Stop(i)) => return Err(Fail::Err("Expecting value".into(), i)),
                Err(e) => return Err(e),
            };
            let mut next = self.at(end);
            if Self::in_ws(next) {
                end = self.ws_end(end + 1);
                next = self.at(end);
            }
            end += 1;
            if next == Some(']') {
                return Ok(end);
            } else if next != Some(',') {
                return Err(Fail::Err("Expecting ',' delimiter".into(), end - 1));
            }
            let comma = end - 1;
            // `try: if s[end] in ws: ...; nextchar = s[end:end + 1]  except IndexError: pass`
            let mut next = Some(',');
            if self.at(end).is_some() {
                if self.at(end).is_some_and(is_ws) {
                    end += 1;
                    if self.at(end).is_some() {
                        if self.at(end).is_some_and(is_ws) {
                            end = self.ws_end(end + 1);
                        }
                        next = self.at(end);
                    }
                } else {
                    next = self.at(end);
                }
            }
            if next == Some(']') {
                return Err(Fail::Err(
                    "Illegal trailing comma before end of array".into(),
                    comma,
                ));
            }
        }
    }
}

/// `str(JSONDecodeError)` for a text `json.loads` rejects, or `None` when this reading finds
/// nothing wrong (CPython also accepts `NaN` and `Infinity`, which `serde_json` does not).
pub fn message_for(text: &str) -> Option<String> {
    if text.starts_with('\u{feff}') {
        return Some(format(
            text,
            "Unexpected UTF-8 BOM (decode using utf-8-sig)",
            0,
        ));
    }
    let doc = Doc {
        s: text.chars().collect(),
    };
    let (msg, pos) = match doc.scan_once(doc.ws_end(0)) {
        Err(Fail::Stop(i)) => ("Expecting value".to_owned(), i),
        Err(Fail::Err(m, i)) => (m, i),
        Ok(end) => {
            let end = doc.ws_end(end);
            if end == doc.s.len() {
                return None;
            }
            ("Extra data".to_owned(), end)
        }
    };
    Some(format(text, &msg, pos))
}

/// `'%s: line %d column %d (char %d)'`, counted in characters.
fn format(text: &str, msg: &str, pos: usize) -> String {
    let before: Vec<char> = text.chars().take(pos).collect();
    let lineno = before.iter().filter(|c| **c == '\n').count() + 1;
    let colno = match before.iter().rposition(|c| *c == '\n') {
        Some(i) => pos - i,
        None => pos + 1,
    };
    format!("{msg}: line {lineno} column {colno} (char {pos})")
}
