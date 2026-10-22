//! The coercions the JSON-API sites (Sankaku, Anime-Pictures) apply to a record's fields, which
//! are the same few `int(...)` guards written the same way in each Python module.

use serde_json::{Map, Value};

use crate::pycompat;

/// The category number of a tag entry: `int(raw_type) if raw_type is not None else -1`, with a
/// value `int()` rejects also -1. -1 is in no site's table, so the tag falls into "general".
pub(super) fn tag_type_id(raw: Option<&Value>) -> i64 {
    raw.and_then(pycompat::int).unwrap_or(-1)
}

/// `int(v) if v else None` for a byte size, where a value `int()` rejects or one that is not
/// positive is "unknown" and not an error.
pub(super) fn size_bytes(raw: Option<&Value>) -> Option<i64> {
    let raw = raw.filter(|v| pycompat::truthy(v))?;
    pycompat::int(raw).filter(|size| *size > 0)
}

/// The `width` and `height` keys as a pair, or `(None, None)` unless both are usable.
///
/// A value that is absent, `null`, `0` or empty counts as missing; one `int()` rejects makes the
/// whole pair unknown, and so does a pair that has only one side.
pub(super) fn dimensions(post: &Map<String, Value>) -> (Option<i64>, Option<i64>) {
    let side = |key: &str| match post.get(key).filter(|v| pycompat::truthy(v)) {
        None => Some(None),
        Some(v) => pycompat::int(v).map(|n| (n != 0).then_some(n)),
    };
    match (side("width"), side("height")) {
        (Some(Some(width)), Some(Some(height))) => (Some(width), Some(height)),
        _ => (None, None),
    }
}

/// The value when it is a string that is an absolute `http(s)` URL: the only kind a download or
/// a preview can be made from. A filename or a protocol-relative path is not.
pub(super) fn absolute_url(raw: Option<&Value>) -> Option<String> {
    let value = raw?.as_str()?;
    (value.starts_with("http://") || value.starts_with("https://")).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_tag_type_that_is_not_a_number_is_minus_one() {
        assert_eq!(tag_type_id(Some(&json!(4))), 4);
        assert_eq!(tag_type_id(Some(&json!("3"))), 3);
        assert_eq!(tag_type_id(Some(&json!(true))), 1);
        assert_eq!(tag_type_id(Some(&json!(2.9))), 2);
        for odd in [json!(null), json!("x"), json!([4]), json!({"a": 1})] {
            assert_eq!(tag_type_id(Some(&odd)), -1, "{odd}");
        }
        assert_eq!(tag_type_id(None), -1);
    }

    #[test]
    fn a_size_is_a_positive_integer_or_unknown() {
        assert_eq!(size_bytes(Some(&json!(123))), Some(123));
        assert_eq!(size_bytes(Some(&json!("456"))), Some(456));
        assert_eq!(size_bytes(Some(&json!(7.9))), Some(7));
        for odd in [
            json!(0),
            json!(-5),
            json!("-5"),
            json!("big"),
            json!(null),
            json!(""),
        ] {
            assert_eq!(size_bytes(Some(&odd)), None, "{odd}");
        }
        assert_eq!(size_bytes(None), None);
    }

    fn object(v: Value) -> Map<String, Value> {
        v.as_object().cloned().expect("object")
    }

    #[test]
    fn dimensions_need_both_sides_and_one_bad_side_spoils_both() {
        let dims = |v| dimensions(&object(v));
        assert_eq!(dims(json!({"width": 3, "height": 4})), (Some(3), Some(4)));
        assert_eq!(
            dims(json!({"width": "3", "height": 4.5})),
            (Some(3), Some(4))
        );
        assert_eq!(dims(json!({"width": 3})), (None, None));
        assert_eq!(dims(json!({"width": 3, "height": 0})), (None, None));
        assert_eq!(dims(json!({"width": "0", "height": 4})), (None, None));
        assert_eq!(dims(json!({"width": "wide", "height": 4})), (None, None));
        assert_eq!(dims(json!({})), (None, None));
    }

    #[test]
    fn only_an_absolute_http_url_counts() {
        assert_eq!(
            absolute_url(Some(&json!("https://a/b.png"))).as_deref(),
            Some("https://a/b.png")
        );
        assert_eq!(
            absolute_url(Some(&json!("http://a/b"))).as_deref(),
            Some("http://a/b")
        );
        for odd in [
            json!("596771-original.jpeg"),
            json!("//a/b"),
            json!(5),
            json!(null),
        ] {
            assert_eq!(absolute_url(Some(&odd)), None, "{odd}");
        }
        assert_eq!(absolute_url(None), None);
    }
}
