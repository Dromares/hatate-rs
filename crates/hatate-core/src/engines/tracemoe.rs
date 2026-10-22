//! trace.moe anime-scene search (`core/tracemoe.py`).
//!
//! trace.moe identifies which anime a screenshot is from, which episode and where. Its official
//! JSON API answers a POST of the image; a match's URL is the AniList page, so the value is the
//! series tag and not a source image. Results below `min_similarity` (default 85%) are dropped;
//! false positives are common under that.
//!
//! This is the half that reads the answer: [`search_from_response`] is what `search` does once
//! the response is in hand and [`parse_results`] is `parse_results`. The upload and the network
//! errors are the engine client's (Phase 3; see the module docs of [`super`]).
//!
//! Where Python reads a field unchecked and a shape the API never sends would make it raise (a
//! `title` that is a string, a `romaji` that is a number, a list `result` of a non-dict, an
//! `anilist` that is a boolean, a payload that is not an object), this reads the field as the type
//! the API sends and takes the rest as absent, and a payload that is not an object has no
//! matches.

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use tracing::debug;

use super::client::{
    Failure, OnTick, failure_of, read_upload, response_text, seconds, ticking, upload_part,
};
use crate::hard_timeout::HardTimeoutError;
use crate::model::{Tag, TagSource};
use crate::net::{NetClient, NetError, NetRequest};
use crate::pycompat;
use crate::pyrepr::py_str;

pub const API_URL: &str = "https://api.trace.moe/search";
pub const DEFAULT_MIN_SIMILARITY: f64 = 85.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraceMoeMatch {
    /// The AniList page of the series.
    pub url: String,
    pub similarity: f64,
    pub thumb_url: Option<String>,
    pub title: Option<String>,
    pub episode: Option<String>,
    pub tags: Vec<Tag>,
}

/// `TraceMoeError`: the message is user-facing text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceMoeError(pub String);

impl std::fmt::Display for TraceMoeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TraceMoeError {}

/// The request half of `core/tracemoe.py`'s `search`.
///
/// A multipart POST of the (prepared) image to `<api>?cutBorders=1&anilistInfo=1`, with a
/// deadline over the whole exchange. What the answer means is [`search_from_response`].
#[derive(Debug, Clone)]
pub struct TraceMoeClient {
    net: NetClient,
    api_url: String,
}

impl TraceMoeClient {
    /// The real api.trace.moe.
    pub fn new(net: NetClient) -> Self {
        Self::with_host(net, API_URL)
    }

    /// The same client against another address (a local https server in tests). Not reachable
    /// from `config.json`, the environment or a flag.
    pub fn with_host(net: NetClient, api_url: &str) -> Self {
        TraceMoeClient {
            net,
            api_url: api_url.to_owned(),
        }
    }

    /// `search`: upload an image to trace.moe and return the parsed matches at or above
    /// `min_similarity`. `on_tick` is the UI countdown, see [`OnTick`].
    pub async fn search(
        &self,
        image_path: &Path,
        timeout: f64,
        on_tick: Option<OnTick>,
        min_similarity: f64,
    ) -> Result<Vec<TraceMoeMatch>, TraceMoeError> {
        debug!("Uploading {} to trace.moe", image_path.display());
        let (data, filename) = read_upload(image_path)
            .await
            .map_err(|e| TraceMoeError(format!("Could not read image: {}", e.0)))?;
        let request = self
            .request(data, filename, timeout)
            .map_err(|e| TraceMoeError(format!("trace.moe request failed: {e}")))?;
        let sent = ticking(
            "Waiting on trace.moe",
            timeout,
            on_tick,
            self.net.send(request),
        );
        let resp = match sent.await {
            Ok(resp) => resp,
            Err(err) => {
                return Err(match failure_of(&err, seconds(timeout)) {
                    Failure::Deadline(HardTimeoutError(why)) => {
                        TraceMoeError(format!("trace.moe request timed out: {why}"))
                    }
                    Failure::Transport(why) => {
                        TraceMoeError(format!("trace.moe request failed: {why}"))
                    }
                });
            }
        };
        search_from_response(resp.status.as_u16(), &response_text(&resp), min_similarity)
    }

    /// The POST `search` makes: `net.post(API_URL, params=..., files=..., timeout=timeout,
    /// deadline=timeout)`.
    fn request(
        &self,
        data: bytes::Bytes,
        filename: String,
        timeout: f64,
    ) -> Result<NetRequest, NetError> {
        let limit = seconds(timeout);
        Ok(NetRequest::post(&self.api_url, limit)?
            .query("cutBorders", "1")
            .query("anilistInfo", "1")
            .with_multipart(vec![upload_part("image", data, filename)])
            .with_deadline(limit))
    }
}

/// What `search` makes of the answer to its POST: the status mapping, the JSON check, the
/// `error` field, then [`parse_results`]. `body` is the response text.
pub fn search_from_response(
    status: u16,
    body: &str,
    min_similarity: f64,
) -> Result<Vec<TraceMoeMatch>, TraceMoeError> {
    match status {
        429 => {
            return Err(TraceMoeError(
                "trace.moe rate limit reached (HTTP 429)".to_owned(),
            ));
        }
        402 => {
            return Err(TraceMoeError(
                "trace.moe monthly quota exhausted (HTTP 402)".to_owned(),
            ));
        }
        200 => {}
        other => return Err(TraceMoeError(format!("trace.moe returned HTTP {other}"))),
    }
    let payload: Value = serde_json::from_str(body)
        .map_err(|_| TraceMoeError("trace.moe returned non-JSON".to_owned()))?;
    if let Some(error) = payload.get("error").filter(|e| pycompat::truthy(e)) {
        return Err(TraceMoeError(format!("trace.moe: {}", py_str(error))));
    }
    let matches = parse_results(&payload, min_similarity);
    debug!(
        "trace.moe parsed into {} match(es) at or above {min_similarity:.0}%",
        matches.len()
    );
    Ok(matches)
}

/// `parse_results`: a trace.moe JSON body into matches, best first, one per series.
pub fn parse_results(payload: &Value, min_similarity: f64) -> Vec<TraceMoeMatch> {
    let mut matches: Vec<TraceMoeMatch> = Vec::new();
    let mut seen_ids: HashSet<i64> = HashSet::new();
    let results = payload.get("result").and_then(Value::as_array);
    for raw in results.into_iter().flatten() {
        let Some(raw) = raw.as_object() else { continue };
        let Some(similarity) = similarity_percent(raw.get("similarity")) else {
            continue;
        };
        if similarity < min_similarity {
            continue;
        }
        let Some((anilist_id, title, mut tags)) = anilist_fields(raw.get("anilist")) else {
            continue;
        };
        if !seen_ids.insert(anilist_id) {
            continue;
        }

        let episode = episode_label(raw.get("episode"));
        if let Some(episode) = &episode {
            tags.push(Tag {
                name: format!("episode {episode}"),
                namespace: Some("meta".to_owned()),
                source: TagSource::SearchEngine,
            });
        }

        matches.push(TraceMoeMatch {
            url: format!("https://anilist.co/anime/{anilist_id}"),
            similarity,
            // `raw.get("image") or None`
            thumb_url: raw
                .get("image")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            title,
            episode,
            tags,
        });
    }
    // `-similarity`, stable.
    matches.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
    matches
}

/// `_similarity_percent`: trace.moe reports 0 to 1; a 0 to 100 value (a proxy, an older answer)
/// is taken as it is. Anything else, or not a number, is `None`.
fn similarity_percent(value: Option<&Value>) -> Option<f64> {
    let number = pycompat::float(value?)?;
    if (0.0..=1.0).contains(&number) {
        Some(number * 100.0)
    } else if number > 1.0 && number <= 100.0 {
        Some(number)
    } else {
        None
    }
}

/// `_anilist_fields`: `(id, display title, series tags)`, or `None` when there is no usable id.
/// `anilist` is an integer id (`anilistInfo` off) or an object (on).
fn anilist_fields(anilist: Option<&Value>) -> Option<(i64, Option<String>, Vec<Tag>)> {
    let anilist = anilist?;
    if let Some(id) = anilist.as_i64() {
        return Some((id, None, Vec::new()));
    }
    let anilist = anilist.as_object()?;
    let id = pycompat::int(anilist.get("id")?)?;

    let name_of = |key: &str| -> String {
        anilist
            .get("title")
            .and_then(Value::as_object)
            .and_then(|titles| titles.get(key))
            .and_then(Value::as_str)
            .map(|s| pycompat::strip(s).to_owned())
            .unwrap_or_default()
    };
    let (romaji, english, native) = (name_of("romaji"), name_of("english"), name_of("native"));
    let display = [&romaji, &english, &native]
        .into_iter()
        .find(|name| !name.is_empty())
        .cloned();

    let mut tags: Vec<Tag> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for name in [&romaji, &english] {
        if name.is_empty() || !seen.insert(name.to_lowercase()) {
            continue;
        }
        tags.push(Tag {
            name: name.replace(' ', "_"),
            namespace: Some("series".to_owned()),
            source: TagSource::SearchEngine,
        });
    }
    Some((id, display, tags))
}

/// `_episode_label`: `str(value).strip()` of the episode, the first of a list, or `None` when
/// that is empty.
fn episode_label(value: Option<&Value>) -> Option<String> {
    let value = value?;
    let value = match value {
        Value::Null => return None,
        Value::String(s) if s.is_empty() => return None,
        Value::Array(items) => items.first().filter(|v| !v.is_null())?,
        other => other,
    };
    let label = pycompat::strip(&py_str(value)).to_owned();
    (!label.is_empty()).then_some(label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn names(m: &TraceMoeMatch) -> Vec<String> {
        m.tags.iter().map(Tag::display).collect()
    }

    #[test]
    fn a_full_answer_gives_the_series_tags_title_and_episode() {
        let payload = json!({"error": "", "result": [{
            "anilist": {"id": 21, "title": {"romaji": " One Piece ", "english": "One piece", "native": "x"}},
            "episode": 15, "image": "https://media.trace.moe/i.jpg", "similarity": 0.94}]});
        let got = parse_results(&payload, 85.0);
        assert_eq!(got.len(), 1);
        let m = &got[0];
        assert_eq!(m.url, "https://anilist.co/anime/21");
        assert_eq!(m.similarity, 0.94 * 100.0);
        assert_eq!(m.title.as_deref(), Some("One Piece"));
        assert_eq!(m.episode.as_deref(), Some("15"));
        assert_eq!(
            m.thumb_url.as_deref(),
            Some("https://media.trace.moe/i.jpg")
        );
        // The English title is the same name in another case: one tag. Then the episode.
        assert_eq!(names(m), ["series:One_Piece", "meta:episode 15"]);
    }

    #[test]
    fn low_scores_and_unscalable_ones_are_dropped_and_a_percent_is_taken_as_it_is() {
        let payload = json!({"result": [
            {"anilist": 1, "similarity": 0.84},
            {"anilist": 2, "similarity": 90},
            {"anilist": 3, "similarity": "0.95"},
            {"anilist": 4, "similarity": 101},
            {"anilist": 5, "similarity": -1},
            {"anilist": 6, "similarity": null},
            {"anilist": 7, "similarity": [0.99]},
            {"anilist": 8}]});
        let got: Vec<_> = parse_results(&payload, 85.0)
            .into_iter()
            .map(|m| (m.url, m.similarity))
            .collect();
        assert_eq!(
            got,
            [
                ("https://anilist.co/anime/3".to_owned(), 0.95 * 100.0),
                ("https://anilist.co/anime/2".to_owned(), 90.0)
            ]
        );
    }

    #[test]
    fn one_match_per_series_the_first_seen_and_equal_scores_keep_answer_order() {
        let payload = json!({"result": [
            {"anilist": 2, "similarity": 0.9},
            {"anilist": 1, "similarity": 0.9},
            {"anilist": {"id": "2"}, "similarity": 0.99},
            {"anilist": {"title": {"romaji": "no id"}}, "similarity": 0.99},
            {"anilist": {"id": "x"}, "similarity": 0.99},
            {"anilist": "7", "similarity": 0.99},
            "junk"]});
        let ids: Vec<_> = parse_results(&payload, 0.0)
            .into_iter()
            .map(|m| m.url)
            .collect();
        assert_eq!(
            ids,
            ["https://anilist.co/anime/2", "https://anilist.co/anime/1"]
        );
    }

    #[test]
    fn the_episode_is_the_text_of_the_value_or_of_a_lists_first_item() {
        let label = |v: Value| episode_label(Some(&v));
        assert_eq!(label(json!(15)), Some("15".into()));
        assert_eq!(label(json!(" 12.5 ")), Some("12.5".into()));
        assert_eq!(label(json!(15.0)), Some("15.0".into()));
        assert_eq!(label(json!([3, 4])), Some("3".into()));
        assert_eq!(label(json!(["OVA "])), Some("OVA".into()));
        assert_eq!(label(json!([])), None);
        assert_eq!(label(json!([null])), None);
        assert_eq!(label(json!("")), None);
        assert_eq!(label(json!("  ")), None);
        assert_eq!(label(json!(null)), None);
        assert_eq!(episode_label(None), None);
    }

    #[test]
    fn a_thumbnail_that_is_empty_or_not_text_is_absent() {
        let payload = json!({"result": [
            {"anilist": 1, "similarity": 0.9, "image": ""},
            {"anilist": 2, "similarity": 0.9}]});
        assert!(
            parse_results(&payload, 0.0)
                .iter()
                .all(|m| m.thumb_url.is_none())
        );
    }

    #[test]
    fn a_payload_with_no_result_list_has_no_matches() {
        assert!(parse_results(&json!({}), 0.0).is_empty());
        assert!(parse_results(&json!({"result": null}), 0.0).is_empty());
        assert!(parse_results(&json!([1]), 0.0).is_empty());
    }

    #[test]
    fn the_status_the_json_and_the_error_field_decide_before_the_parse() {
        let ok = r#"{"error":"","result":[{"anilist":1,"similarity":0.9}]}"#;
        assert_eq!(search_from_response(200, ok, 85.0).unwrap().len(), 1);
        for (status, message) in [
            (429, "trace.moe rate limit reached (HTTP 429)"),
            (402, "trace.moe monthly quota exhausted (HTTP 402)"),
            (500, "trace.moe returned HTTP 500"),
        ] {
            assert_eq!(
                search_from_response(status, ok, 85.0).unwrap_err().0,
                message
            );
        }
        assert_eq!(
            search_from_response(200, "<html>", 85.0).unwrap_err().0,
            "trace.moe returned non-JSON"
        );
        assert_eq!(
            search_from_response(200, r#"{"error":"Invalid image","result":[]}"#, 85.0)
                .unwrap_err()
                .0,
            "trace.moe: Invalid image"
        );
    }
}

#[cfg(test)]
mod client_tests {
    use bytes::Bytes;

    use super::*;
    use crate::net::{Body, Method, NetConfig};

    #[test]
    fn the_request_is_the_multipart_post_python_makes() {
        let c = TraceMoeClient::new(NetClient::new(NetConfig::default()).unwrap());
        let req = c
            .request(Bytes::from_static(b"x"), "x.png".to_owned(), 30.0)
            .unwrap();
        assert_eq!(req.method, Method::Post);
        // `params={"cutBorders": "1", "anilistInfo": "1"}`, in that order.
        assert_eq!(
            req.url.as_str(),
            "https://api.trace.moe/search?cutBorders=1&anilistInfo=1"
        );
        let Body::Multipart(parts) = &req.body else {
            panic!("{:?}", req.body)
        };
        assert_eq!((parts[0].name.as_str(), parts.len()), ("image", 1));
        // The caller's timeout is used as given: no floor.
        assert_eq!(req.timeout, std::time::Duration::from_secs(30));
        assert_eq!(req.deadline, Some(std::time::Duration::from_secs(30)));
    }
}
