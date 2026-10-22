//! The first record `next(csv.reader(io.StringIO(text), quotechar='"', escapechar="\\"))`
//! returns, with Python's other dialect defaults (delimiter `,`, `doublequote`, not strict, no
//! `skipinitialspace`, `QUOTE_MINIMAL`). A port of the state machine in CPython's `Modules/_csv.c`
//! (3.14), because a Pawchive tag list is read with it and `str::split(',')` disagrees on every
//! quoted element, escape and stray newline.
//!
//! `StringIO` hands the reader one line at a time, each keeping its `\n`; the reader treats `\n`
//! and `\r` inside a line as the end of the record, a quoted field may run over several lines,
//! and an input that ends inside a record still yields it (the dialect is not strict). The two
//! ways the reader raises (`csv.Error`) are [`CsvError`]: a field longer than the default
//! `field_size_limit` of 131072 characters, and a character after a `\n` or `\r` that ended a
//! field.

/// `csv.field_size_limit()` at its default.
const FIELD_SIZE_LIMIT: usize = 131_072;

/// `csv.Error`, which is all the caller needs to know: it reads it as "no tags".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsvError {
    FieldTooLarge,
    NewlineInUnquotedField,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    StartRecord,
    StartField,
    EscapedChar,
    InField,
    InQuotedField,
    EscapeInQuotedField,
    QuoteInQuotedField,
    EatCrnl,
    AfterEscapedCrnl,
}

struct Reader {
    state: State,
    field: String,
    field_len: usize,
    fields: Vec<String>,
}

/// What the reader is fed in place of a character at the end of a line.
const EOL: Option<char> = None;

impl Reader {
    fn add_char(&mut self, c: char) -> Result<(), CsvError> {
        if self.field_len >= FIELD_SIZE_LIMIT {
            return Err(CsvError::FieldTooLarge);
        }
        self.field.push(c);
        self.field_len += 1;
        Ok(())
    }

    fn save_field(&mut self) {
        self.fields.push(std::mem::take(&mut self.field));
        self.field_len = 0;
    }

    /// `parse_process_char`.
    fn process(&mut self, c: Option<char>) -> Result<(), CsvError> {
        use State::*;
        let is_newline = matches!(c, Some('\n' | '\r'));
        let is_end = is_newline || c == EOL;
        match self.state {
            StartRecord => {
                if c == EOL {
                    return Ok(());
                }
                if is_newline {
                    self.state = EatCrnl;
                    return Ok(());
                }
                self.state = StartField;
                self.start_field(c, is_newline, is_end)
            }
            StartField => self.start_field(c, is_newline, is_end),
            EscapedChar => {
                if is_newline {
                    self.add_char(c.expect("a newline"))?;
                    self.state = AfterEscapedCrnl;
                    return Ok(());
                }
                self.add_char(c.unwrap_or('\n'))?;
                self.state = InField;
                Ok(())
            }
            AfterEscapedCrnl if c == EOL => Ok(()),
            AfterEscapedCrnl | InField => {
                if is_end {
                    self.save_field();
                    self.state = if c == EOL { StartRecord } else { EatCrnl };
                } else if c == Some('\\') {
                    self.state = EscapedChar;
                } else if c == Some(',') {
                    self.save_field();
                    self.state = StartField;
                } else {
                    // `AfterEscapedCrnl` falls through into `InField` and stays in its own state.
                    self.add_char(c.expect("not the end of the line"))?;
                }
                Ok(())
            }
            InQuotedField => {
                match c {
                    EOL => {}
                    Some('\\') => self.state = EscapeInQuotedField,
                    Some('"') => self.state = QuoteInQuotedField,
                    Some(c) => self.add_char(c)?,
                }
                Ok(())
            }
            EscapeInQuotedField => {
                self.add_char(c.unwrap_or('\n'))?;
                self.state = InQuotedField;
                Ok(())
            }
            QuoteInQuotedField => {
                if c == Some('"') {
                    self.add_char('"')?;
                    self.state = InQuotedField;
                } else if c == Some(',') {
                    self.save_field();
                    self.state = StartField;
                } else if is_end {
                    self.save_field();
                    self.state = if c == EOL { StartRecord } else { EatCrnl };
                } else {
                    self.add_char(c.expect("not the end of the line"))?;
                    self.state = InField;
                }
                Ok(())
            }
            EatCrnl => {
                if is_newline {
                    Ok(())
                } else if c == EOL {
                    self.state = StartRecord;
                    Ok(())
                } else {
                    Err(CsvError::NewlineInUnquotedField)
                }
            }
        }
    }

    /// The `START_FIELD` arm, which `START_RECORD` falls into.
    fn start_field(
        &mut self,
        c: Option<char>,
        is_newline: bool,
        is_end: bool,
    ) -> Result<(), CsvError> {
        use State::*;
        if is_end {
            self.save_field();
            self.state = if c == EOL { StartRecord } else { EatCrnl };
        } else if c == Some('"') {
            self.state = InQuotedField;
        } else if c == Some('\\') {
            self.state = EscapedChar;
        } else if c == Some(',') {
            self.save_field();
        } else {
            debug_assert!(!is_newline);
            self.add_char(c.expect("not the end of the line"))?;
            self.state = InField;
        }
        Ok(())
    }
}

/// The first record of `text`, as `next(reader)` returns it. An empty `Vec` is what the reader
/// gives for a blank first line, and also what `next` gives by raising `StopIteration` when the
/// input ends with nothing pending; the caller cannot tell them apart and does not need to.
pub fn first_record(text: &str) -> Result<Vec<String>, CsvError> {
    let mut reader = Reader {
        state: State::StartRecord,
        field: String::new(),
        field_len: 0,
        fields: Vec::new(),
    };
    for line in text.split_inclusive('\n') {
        for c in line.chars() {
            reader.process(Some(c))?;
        }
        reader.process(EOL)?;
        if reader.state == State::StartRecord {
            return Ok(reader.fields);
        }
    }
    // The input ran out inside a record. A field in progress, or an open quote, is saved and
    // returned (the dialect is not strict); otherwise `next` raises StopIteration.
    if reader.field_len != 0 || reader.state == State::InQuotedField {
        reader.save_field();
        Ok(reader.fields)
    } else {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(text: &str) -> Vec<String> {
        first_record(text).expect("reads")
    }

    #[test]
    fn plain_and_quoted_fields() {
        assert_eq!(row("a,b,c"), ["a", "b", "c"]);
        assert_eq!(
            row(r#"Bondage,Damsel,"Total Versext",gefesselt"#),
            ["Bondage", "Damsel", "Total Versext", "gefesselt"]
        );
        assert_eq!(row(r#""a, b",c"#), ["a, b", "c"]);
        assert_eq!(row(r#""a""b",c"#), ["a\"b", "c"]);
        assert_eq!(row("a,"), ["a", ""]);
        assert_eq!(row(",a"), ["", "a"]);
        assert_eq!(row(" a , b "), [" a ", " b "]);
    }

    #[test]
    fn a_quote_inside_an_unquoted_field_is_a_character() {
        assert_eq!(row(r#"a"b,c"#), ["a\"b", "c"]);
        // After a closing quote the rest of the field is kept, the dialect not being strict.
        assert_eq!(row(r#""a"b,c"#), ["ab", "c"]);
    }

    #[test]
    fn the_escape_character_takes_the_next_one_literally() {
        assert_eq!(row(r"a\,b,c"), ["a,b", "c"]);
        assert_eq!(row(r#"a\"b"#), ["a\"b"]);
        assert_eq!(row(r#""a\"b",c"#), ["a\"b", "c"]);
        // At the very end of the input it is a newline.
        assert_eq!(row("a\\"), ["a\n"]);
        assert_eq!(row("a\\\nb,c"), ["a\nb", "c"]);
    }

    #[test]
    fn only_the_first_record_is_read_and_a_quoted_field_may_span_lines() {
        assert_eq!(row("a,b\nc,d"), ["a", "b"]);
        assert_eq!(row("a,\"b\nc\",d\ne"), ["a", "b\nc", "d"]);
        assert_eq!(row("a\r\nb"), ["a"]);
        assert_eq!(row("a\n\rb"), ["a"]);
        assert_eq!(row("\n"), Vec::<String>::new());
        assert_eq!(row("\nabc"), Vec::<String>::new());
    }

    #[test]
    fn input_that_ends_inside_a_record_still_yields_it() {
        assert_eq!(row("\"a"), ["a"]);
        assert_eq!(row("\"a,b"), ["a,b"]);
        assert_eq!(row("a,\"b\n"), ["a", "b\n"]);
    }

    #[test]
    fn a_character_after_a_carriage_return_is_an_error() {
        assert_eq!(first_record("a\rb"), Err(CsvError::NewlineInUnquotedField));
        assert_eq!(first_record("\rb"), Err(CsvError::NewlineInUnquotedField));
        // A carriage return inside quotes is data.
        assert_eq!(row("\"a\rb\",c"), ["a\rb", "c"]);
    }

    #[test]
    fn nul_is_an_ordinary_character_and_a_field_has_a_size_limit() {
        assert_eq!(row("a\0b"), ["a\0b"]);
        assert_eq!(row(&"x".repeat(131_072)).len(), 1);
        assert_eq!(
            first_record(&"x".repeat(131_073)),
            Err(CsvError::FieldTooLarge)
        );
        // The limit is per field, not per record.
        assert_eq!(row(&format!("{},{}", "x".repeat(131_072), "y")).len(), 2);
    }
}
