//! Pawchive exact-file lookup, by SHA-256 (`core/pawchive_lookup.py`).
//!
//! Pawchive (pawchive.pw, a Kemono fork archiving Patreon and Fanbox) stores every file under its
//! own SHA-256, and `/api/v1/search_hash/<sha256>` answers anonymously with every post that holds
//! the file (404 for a hash it has never seen). It finds byte-identical copies only: a resized or
//! re-saved copy has another hash.
//!
//! This is the half that reads the answer: [`lookup_from_response`] is what `lookup` does once
//! the response is in hand and [`parse_response`] is `parse_response`. Hashing the file
//! (`file_sha256`) and the GET are the engine client's (Phase 3; see the module docs of
//! [`super`]). This is not the booru parser of the same site, `boorus::pawchive`.
//!
//! Where Python formats a post's `service`, `user` or `id` straight into the URL, any JSON value
//! that holds is printed as Python's `str` would ([`crate::pyrepr::py_str`]). A `title` is read as
//! the text the site sends: where Python would hand a number through, this has no title.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::pycompat;
use crate::pyrepr::py_str;

pub const API_URL: &str = "https://pawchive.pw/api/v1/search_hash/";
pub const THUMBNAIL_URL: &str = "https://img.pawchive.pw/thumbnail/data";

/// The same file can sit in several posts; a few is useful, dozens would bury the rest.
pub const MAX_POSTS: usize = 5;

static SHA256_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9a-f]{64}\n?\z").expect("sha256 regex"));

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PawchiveMatch {
    pub url: String,
    pub title: Option<String>,
    pub thumb_url: Option<String>,
    /// Which image of the post this file is, 0-based.
    pub page_index: usize,
    pub page_count: usize,
}

/// `PawchiveLookupError`: the message is user-facing text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PawchiveLookupError(pub String);

impl std::fmt::Display for PawchiveLookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PawchiveLookupError {}

/// The hash as `lookup` normalises it (stripped, lower-cased), or the error it raises for text
/// that is not a SHA-256.
pub fn normalise_sha256(sha256: &str) -> Result<String, PawchiveLookupError> {
    let sha256 = pycompat::strip(sha256).to_lowercase();
    if SHA256_RE.is_match(&sha256) {
        Ok(sha256)
    } else {
        let head: String = sha256.chars().take(16).collect();
        Err(PawchiveLookupError(format!(
            "not a SHA-256: {}",
            crate::pyrepr::py_repr(&Value::String(head))
        )))
    }
}

/// The address `lookup` asks for a (normalised) hash.
pub fn request_url(sha256: &str) -> String {
    format!("{API_URL}{sha256}")
}

/// What `lookup` makes of the answer: 404 is "never seen it" (no matches), 429 and any other
/// status but 200 are errors, a body that is not JSON is an error, then [`parse_response`].
pub fn lookup_from_response(
    status: u16,
    body: &str,
    sha256: &str,
) -> Result<Vec<PawchiveMatch>, PawchiveLookupError> {
    match status {
        404 => return Ok(Vec::new()),
        429 => {
            return Err(PawchiveLookupError(
                "pawchive.pw is rate limiting - slow the search delay down".to_owned(),
            ));
        }
        200 => {}
        other => {
            return Err(PawchiveLookupError(format!(
                "pawchive.pw answered HTTP {other}"
            )));
        }
    }
    let body: Value = serde_json::from_str(body).map_err(|_| {
        PawchiveLookupError("pawchive.pw sent something that isn't JSON".to_owned())
    })?;
    Ok(parse_response(&body, sha256))
}

/// `files_of`: a post's files in the site's own order, "file" first. "file" is normally
/// `attachments[0]` repeated, so paths are de-duplicated. Only an entry with a path starting `/`
/// counts.
fn files_of(post: &Map<String, Value>) -> Vec<&str> {
    let attachments = post
        .get("attachments")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let mut paths: Vec<&str> = Vec::new();
    for entry in std::iter::once(post.get("file")).chain(attachments.iter().map(Some)) {
        let path = entry
            .and_then(Value::as_object)
            .and_then(|e| e.get("path"))
            .and_then(Value::as_str);
        if let Some(path) = path
            && path.starts_with('/')
            && !paths.contains(&path)
        {
            paths.push(path);
        }
    }
    paths
}

/// A value that is there and truthy in Python's sense (`if not (service and user and post_id)`).
fn truthy(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| pycompat::truthy(v))
}

/// `parse_response`: the posts in a `search_hash` answer, each pointing at the file. A post with
/// no `service`, `user` or `id` is skipped; at most [`MAX_POSTS`] are kept.
pub fn parse_response(body: &Value, sha256: &str) -> Vec<PawchiveMatch> {
    let mut matches = Vec::new();
    let posts = body.get("posts").and_then(Value::as_array);
    for post in posts.into_iter().flatten() {
        let Some(post) = post.as_object() else { continue };
        let (service, user, id) = (post.get("service"), post.get("user"), post.get("id"));
        let (Some(service), Some(user), Some(id)) =
            (truthy(service), truthy(user), truthy(id))
        else {
            continue;
        };
        let files = files_of(post);
        let index = files.iter().position(|path| path.contains(sha256)).unwrap_or(0);
        matches.push(PawchiveMatch {
            url: format!(
                "https://pawchive.pw/{}/user/{}/post/{}",
                py_str(service),
                py_str(user),
                py_str(id)
            ),
            title: post
                .get("title")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
                .map(str::to_owned),
            thumb_url: files.get(index).map(|path| format!("{THUMBNAIL_URL}{path}")),
            page_index: index,
            page_count: files.len().max(1),
        });
        if matches.len() >= MAX_POSTS {
            break;
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const HASH: &str = "3fac89759d85abce1ebcbff0c27d838d9a52c47ffc5a6fdcf31c6986064bbefb";

    fn post(id: &str) -> Value {
        json!({"service": "fanbox", "user": "44", "id": id, "title": "T",
               "file": {"path": "/d4/x.png"},
               "attachments": [{"path": "/d4/x.png"}, {"path": format!("/3f/ac/{HASH}.jpeg")}]})
    }

    #[test]
    fn a_post_points_at_the_page_that_holds_the_hash() {
        let got = parse_response(&json!({"posts": [post("9")]}), HASH);
        assert_eq!(got.len(), 1);
        let m = &got[0];
        assert_eq!(m.url, "https://pawchive.pw/fanbox/user/44/post/9");
        assert_eq!(m.title.as_deref(), Some("T"));
        assert_eq!((m.page_index, m.page_count), (1, 2));
        assert_eq!(
            m.thumb_url.as_deref(),
            Some(format!("https://img.pawchive.pw/thumbnail/data/3f/ac/{HASH}.jpeg").as_str())
        );
    }

    #[test]
    fn a_hash_in_no_file_points_at_the_first_and_no_files_means_one_page_and_no_thumbnail() {
        let got = parse_response(&json!({"posts": [post("9")]}), "ff");
        assert_eq!((got[0].page_index, got[0].page_count), (0, 2));
        let bare = json!({"service": "s", "user": 1, "id": 2});
        let got = parse_response(&json!({"posts": [bare]}), HASH);
        assert_eq!(got[0].url, "https://pawchive.pw/s/user/1/post/2");
        assert_eq!((got[0].thumb_url.clone(), got[0].page_index, got[0].page_count), (None, 0, 1));
        assert_eq!(got[0].title, None);
    }

    #[test]
    fn paths_must_start_with_a_slash_and_are_kept_once() {
        let p = json!({"service": "s", "user": "u", "id": "i",
            "file": {"path": "no-slash.png"},
            "attachments": [{"path": "/a.png"}, {"path": "/a.png"}, {"name": "x"}, 5, {"path": 7}, {"path": "/b.png"}]});
        let got = parse_response(&json!({"posts": [p]}), HASH);
        assert_eq!(got[0].page_count, 2);
    }

    #[test]
    fn posts_missing_a_part_are_skipped_and_five_is_the_most() {
        let mut posts: Vec<Value> = vec![
            json!({"service": "s", "user": "u"}),
            json!({"service": "", "user": "u", "id": "i"}),
            json!({"service": "s", "user": 0, "id": "i"}),
            json!("junk"),
        ];
        posts.extend((0..8).map(|n| post(&n.to_string())));
        let got = parse_response(&json!({"posts": posts}), HASH);
        let ids: Vec<_> = got.iter().map(|m| m.url.rsplit('/').next().unwrap().to_owned()).collect();
        assert_eq!(ids, ["0", "1", "2", "3", "4"]);
    }

    #[test]
    fn no_posts_is_no_matches() {
        for body in [json!({}), json!({"posts": null}), json!({"posts": {}}), json!(null), json!([])] {
            assert!(parse_response(&body, HASH).is_empty(), "{body}");
        }
    }

    #[test]
    fn numbers_in_a_post_print_as_python_prints_them() {
        let p = json!({"service": "s", "user": 1.5, "id": 10_000_000_000_000_000_000u64, "title": 3});
        let got = parse_response(&json!({"posts": [p]}), HASH);
        assert_eq!(got[0].url, "https://pawchive.pw/s/user/1.5/post/10000000000000000000");
        // A title is read as the text the site sends; a number is not one.
        assert_eq!(got[0].title, None);
    }

    #[test]
    fn the_status_and_the_json_decide_before_the_parse() {
        let body = json!({"posts": [post("9")]}).to_string();
        assert_eq!(lookup_from_response(200, &body, HASH).unwrap().len(), 1);
        assert_eq!(lookup_from_response(404, "anything", HASH).unwrap(), vec![]);
        assert_eq!(
            lookup_from_response(429, &body, HASH).unwrap_err().0,
            "pawchive.pw is rate limiting - slow the search delay down"
        );
        assert_eq!(
            lookup_from_response(503, &body, HASH).unwrap_err().0,
            "pawchive.pw answered HTTP 503"
        );
        assert_eq!(
            lookup_from_response(200, "<html>", HASH).unwrap_err().0,
            "pawchive.pw sent something that isn't JSON"
        );
    }

    #[test]
    fn the_hash_is_stripped_and_lower_cased_and_anything_else_is_an_error() {
        assert_eq!(normalise_sha256(&format!(" {}\n", HASH.to_uppercase())).unwrap(), HASH);
        assert_eq!(request_url(HASH), format!("https://pawchive.pw/api/v1/search_hash/{HASH}"));
        assert_eq!(normalise_sha256("abc").unwrap_err().0, "not a SHA-256: 'abc'");
        assert_eq!(
            normalise_sha256(&format!("{HASH}0")).unwrap_err().0,
            "not a SHA-256: '3fac89759d85abce'"
        );
        assert!(normalise_sha256("").is_err());
        assert!(normalise_sha256(&HASH.replace('3', "g")).is_err());
    }
}
