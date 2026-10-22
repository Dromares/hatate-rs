//! How one value is read from JSON (tolerantly) and written (Python-style).

use std::fmt;

use serde_json::{Map, Value};

use super::pyjson;
use super::report::LoadReport;

/// A value that is a single JSON node: a scalar, a list or a flat map.
pub(crate) trait Leaf: Sized {
    /// `None` when `v` has the wrong JSON type for this field.
    fn parse(v: &Value) -> Option<Self>;
    fn write(&self, out: &mut String, depth: usize);
}

/// A settings field: a [`Leaf`] or a nested section.
pub(crate) trait Field {
    /// Overwrite `self` from `v`; on a wrong type keep `self` and say so in `rep`.
    fn read(&mut self, v: &Value, key: &str, rep: &mut LoadReport);
    fn write(&self, out: &mut String, depth: usize);
}

impl<T: Leaf> Field for T {
    fn read(&mut self, v: &Value, key: &str, rep: &mut LoadReport) {
        match T::parse(v) {
            Some(x) => *self = x,
            None => rep.wrong_type(key),
        }
    }

    fn write(&self, out: &mut String, depth: usize) {
        Leaf::write(self, out, depth);
    }
}

/// A nested object whose key set is fixed (rule 6.1.4).
pub(crate) trait Section {
    fn read_object(&mut self, obj: &Map<String, Value>, prefix: &str, rep: &mut LoadReport);
    fn write_object(&self, out: &mut String, depth: usize);
}

pub(crate) fn join_key(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_owned()
    } else {
        format!("{prefix}.{key}")
    }
}

impl Leaf for bool {
    fn parse(v: &Value) -> Option<Self> {
        v.as_bool()
    }
    fn write(&self, out: &mut String, _: usize) {
        out.push_str(if *self { "true" } else { "false" });
    }
}

impl Leaf for i64 {
    fn parse(v: &Value) -> Option<Self> {
        if let Some(i) = v.as_i64() {
            return Some(i);
        }
        // `2.0` for an integer field: Python carries it, an integral float is the same number.
        let x = v.as_f64()?;
        (v.is_f64() && x.fract() == 0.0 && x.abs() < 9.0e15).then_some(x as i64)
    }
    fn write(&self, out: &mut String, _: usize) {
        out.push_str(&self.to_string());
    }
}

impl Leaf for f64 {
    fn parse(v: &Value) -> Option<Self> {
        v.as_f64()
    }
    fn write(&self, out: &mut String, _: usize) {
        pyjson::float(out, *self);
    }
}

impl Leaf for String {
    fn parse(v: &Value) -> Option<Self> {
        v.as_str().map(str::to_owned)
    }
    fn write(&self, out: &mut String, _: usize) {
        pyjson::string(out, self);
    }
}

fn write_list<T>(
    items: &[T],
    out: &mut String,
    depth: usize,
    mut item: impl FnMut(&T, &mut String),
) {
    if items.is_empty() {
        out.push_str("[]");
        return;
    }
    out.push('[');
    for (i, it) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        pyjson::newline(out, depth + 1);
        item(it, out);
    }
    pyjson::newline(out, depth);
    out.push(']');
}

impl Leaf for Vec<String> {
    fn parse(v: &Value) -> Option<Self> {
        v.as_array()?.iter().map(<String as Leaf>::parse).collect()
    }
    fn write(&self, out: &mut String, depth: usize) {
        write_list(self, out, depth, |s, o| pyjson::string(o, s));
    }
}

impl Leaf for Vec<f64> {
    fn parse(v: &Value) -> Option<Self> {
        v.as_array()?.iter().map(<f64 as Leaf>::parse).collect()
    }
    fn write(&self, out: &mut String, depth: usize) {
        write_list(self, out, depth, |x, o| pyjson::float(o, *x));
    }
}

/// A JSON object of one value type, in file order (Python dicts keep insertion order).
#[derive(Clone, PartialEq, Default)]
pub struct OrderedMap<V>(pub Vec<(String, V)>);

impl<V: fmt::Debug> fmt::Debug for OrderedMap<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.0.iter().map(|(k, v)| (k, v)))
            .finish()
    }
}

impl<V> OrderedMap<V> {
    pub fn get(&self, key: &str) -> Option<&V> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<V: Leaf> Leaf for OrderedMap<V> {
    fn parse(v: &Value) -> Option<Self> {
        let obj = v.as_object()?;
        obj.iter()
            .map(|(k, v)| V::parse(v).map(|v| (k.clone(), v)))
            .collect::<Option<Vec<_>>>()
            .map(OrderedMap)
    }
    fn write(&self, out: &mut String, depth: usize) {
        if self.0.is_empty() {
            out.push_str("{}");
            return;
        }
        out.push('{');
        for (i, (k, v)) in self.0.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            pyjson::newline(out, depth + 1);
            pyjson::string(out, k);
            out.push_str(": ");
            v.write(out, depth + 1);
        }
        pyjson::newline(out, depth);
        out.push('}');
    }
}

/// A credential (spec 2.5 "Secrets"). Its `Debug` never shows the value, and it has no
/// `Display`, so a `{:?}` of a whole `Settings`, an error or a log line cannot leak it
/// (rule 6.1.10). The only way to the text is [`Secret::expose`].
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Secret(value.into())
    }

    /// The credential itself. Use it to talk to the service it belongs to, nowhere else.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl From<&str> for Secret {
    fn from(s: &str) -> Self {
        Secret(s.to_owned())
    }
}

impl Leaf for Secret {
    fn parse(v: &Value) -> Option<Self> {
        v.as_str().map(Secret::from)
    }
    fn write(&self, out: &mut String, _: usize) {
        pyjson::string(out, &self.0);
    }
}
