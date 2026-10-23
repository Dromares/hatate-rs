//! A handler's result as MCP content blocks (`core/mcp_content.py`).
//!
//! The convention: any result key ending in `_image` whose value is a dict with a `bytes` entry
//! is image content, pulled out into its own image block and replaced in the text block by its
//! remaining metadata (the URL, whether it was full resolution, any fallback reason), never by
//! the raw bytes.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Map, Value, json};

use crate::hydrus_client::py_dumps;

const IMAGE_KEY_SUFFIX: &str = "_image";

/// A dict value that carries image bytes: Python's `{"bytes": ..., "mime_type": ..., ...}`.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageValue {
    /// `value.get("bytes")`: `None` and empty bytes both count as a failed fetch.
    pub bytes: Option<Vec<u8>>,
    /// Every other entry of the dict, in order (`mime_type` among them).
    pub entries: Vec<(String, Value)>,
}

/// One value of a handler's result dict.
#[derive(Debug, Clone, PartialEq)]
pub enum ResultValue {
    Json(Value),
    /// A dict with a `bytes` entry. It is image content only under a key ending in `_image`;
    /// under any other key Python's `json.dumps(default=str)` prints the bytes object's `repr`,
    /// which [`ImageValue`] cannot do, so a handler puts it under an `_image` key.
    Image(ImageValue),
}

/// A handler's result: Python's `Dict[str, Any]`, in insertion order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolOutput {
    pub fields: Vec<(String, ResultValue)>,
}

impl ToolOutput {
    pub fn new() -> Self {
        Self::default()
    }

    /// A result with only JSON values.
    pub fn from_json(map: Map<String, Value>) -> Self {
        Self {
            fields: map
                .into_iter()
                .map(|(k, v)| (k, ResultValue::Json(v)))
                .collect(),
        }
    }

    pub fn push(mut self, key: &str, value: Value) -> Self {
        self.fields.push((key.to_owned(), ResultValue::Json(value)));
        self
    }

    pub fn push_image(mut self, key: &str, image: ImageValue) -> Self {
        self.fields
            .push((key.to_owned(), ResultValue::Image(image)));
        self
    }

    /// The JSON-only view, for the audit log: image fields show their metadata plus nothing
    /// else, the way `outcome` would show them without the bytes.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        for (key, value) in &self.fields {
            map.insert(
                key.clone(),
                match value {
                    ResultValue::Json(v) => v.clone(),
                    ResultValue::Image(img) => Value::Object(img.entries.iter().cloned().collect()),
                },
            );
        }
        Value::Object(map)
    }
}

/// `to_content_blocks(result)`: the text block first, then one image block per `_image` value
/// that has bytes.
pub fn to_content_blocks(result: &ToolOutput) -> Vec<Value> {
    let mut images: Vec<Value> = Vec::new();
    let mut metadata = Map::new();
    for (key, value) in &result.fields {
        match value {
            ResultValue::Image(img) if key.ends_with(IMAGE_KEY_SUFFIX) => {
                let mime_type = img
                    .entries
                    .iter()
                    .find(|(k, _)| k == "mime_type")
                    .map(|(_, v)| v)
                    .filter(|v| crate::pycompat::truthy(v))
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "image/jpeg".to_owned());
                let mut remaining: Map<String, Value> = img.entries.iter().cloned().collect();
                match img.bytes.as_deref().filter(|b| !b.is_empty()) {
                    Some(data) => images.push(json!({
                        "type": "image",
                        "data": STANDARD.encode(data),
                        "mimeType": mime_type,
                    })),
                    None => {
                        remaining.insert("fetch_failed".to_owned(), Value::Bool(true));
                    }
                }
                metadata.insert(key.clone(), Value::Object(remaining));
            }
            ResultValue::Image(img) => {
                metadata.insert(
                    key.clone(),
                    Value::Object(img.entries.iter().cloned().collect()),
                );
            }
            ResultValue::Json(v) => {
                metadata.insert(key.clone(), v.clone());
            }
        }
    }
    let mut blocks = vec![json!({"type": "text", "text": py_dumps(&Value::Object(metadata))})];
    blocks.extend(images);
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_result_becomes_one_text_block() {
        let out = ToolOutput::new()
            .push("allowed", json!(true))
            .push("rows", json!([1, 2]));
        let blocks = to_content_blocks(&out);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "{\"allowed\": true, \"rows\": [1, 2]}");
    }

    #[test]
    fn an_image_key_becomes_an_image_block_plus_its_metadata() {
        let out = ToolOutput::new().push("row", json!(3)).push_image(
            "match_image",
            ImageValue {
                bytes: Some(b"abc".to_vec()),
                entries: vec![
                    ("mime_type".into(), json!("image/png")),
                    ("url".into(), json!("https://example.invalid/a.png")),
                ],
            },
        );
        let blocks = to_content_blocks(&out);
        assert_eq!(blocks.len(), 2);
        assert_eq!(
            blocks[1],
            json!({"type": "image", "data": "YWJj", "mimeType": "image/png"})
        );
        let text = blocks[0]["text"].as_str().unwrap();
        assert!(
            !text.contains("YWJj"),
            "raw bytes never reach the text block"
        );
        assert!(text.contains("\"match_image\": {\"mime_type\": \"image/png\""));
    }

    #[test]
    fn a_failed_fetch_with_no_bytes_produces_no_image_block() {
        for bytes in [None, Some(Vec::new())] {
            let out = ToolOutput::new().push_image(
                "local_image",
                ImageValue {
                    bytes,
                    entries: vec![("reason".into(), json!("gone"))],
                },
            );
            let blocks = to_content_blocks(&out);
            assert_eq!(blocks.len(), 1);
            assert_eq!(
                blocks[0]["text"],
                "{\"local_image\": {\"reason\": \"gone\", \"fetch_failed\": true}}"
            );
        }
    }

    #[test]
    fn the_mime_type_defaults_to_jpeg() {
        let out = ToolOutput::new().push_image(
            "x_image",
            ImageValue {
                bytes: Some(vec![1]),
                entries: vec![("mime_type".into(), Value::Null)],
            },
        );
        assert_eq!(to_content_blocks(&out)[1]["mimeType"], "image/jpeg");
    }
}
