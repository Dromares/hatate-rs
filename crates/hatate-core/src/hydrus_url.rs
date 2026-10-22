//! The URL forms Hydrus's own downloader parses (`core/hydrus_import.py::normalize_url_for_hydrus`).
//!
//! Only this one pure function is here, because the availability sweep (`availability`) checks the
//! post page under the form Hydrus would be given. The import itself is Phase 4.

use std::sync::LazyLock;

use regex::Regex;

use crate::boorus::host::{on_host, raises_value_error};
use crate::boorus::pixiv;
pub use crate::pyurl::UrlValueError;

// `\d` is Unicode in both Python's `re` and here.
static DANBOORU_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(https?://danbooru\.donmai\.us)/post/show/(\d+)").expect("danbooru rewrite")
});
static TWITTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(https?://(?:www\.|mobile\.)?(?:twitter|x)\.com)/i/web/status/(\d+)")
        .expect("twitter rewrite")
});
static ENCODED_SLASH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)%2f").expect("encoded slash"));

/// Forms the search engines hand back that Hydrus's URL classes do not match, each with the form
/// they do. Same URL for the add_url call and the later confirmation poll.
///
/// `Err` is the `ValueError` Python raises when the URL names `rule34.us` and `urlparse` refuses
/// its host.
pub fn normalize_url_for_hydrus(url: &str) -> Result<String, UrlValueError> {
    for (re, rewrite) in [(&*DANBOORU_RE, "/posts/"), (&*TWITTER_RE, "/i/status/")] {
        if let Some(found) = re.captures(url) {
            // `^`-anchored with `count=1`: the match is the start of the URL.
            let whole = found.get(0).expect("whole match").end();
            return Ok(format!(
                "{}{rewrite}{}{}",
                &found[1],
                &found[2],
                &url[whole..]
            ));
        }
    }
    if url.contains("rule34.us") {
        if raises_value_error(url) {
            return Err(UrlValueError);
        }
        if on_host(url, &["rule34.us"]) {
            return Ok(unencode_r_param(url));
        }
    }
    if url.contains("pixiv.net")
        && let Some(id) = pixiv::illust_id(url)
    {
        return Ok(format!("https://www.pixiv.net/artworks/{id}"));
    }
    Ok(url.to_string())
}

/// `(?<=[?&]r=)([^&#]*)` with its `%2f`s turned back into slashes, first match only. The regex
/// crate has no look-behind: the match starts right after the first `?r=` or `&r=`.
fn unencode_r_param(url: &str) -> String {
    let start = ["?r=", "&r="]
        .iter()
        .filter_map(|needle| url.find(needle))
        .min()
        .map(|at| at + 3);
    let Some(start) = start else {
        return url.to_string();
    };
    let end = url[start..]
        .find(['&', '#'])
        .map_or(url.len(), |at| start + at);
    format!(
        "{}{}{}",
        &url[..start],
        ENCODED_SLASH_RE.replace_all(&url[start..end], "/"),
        &url[end..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(url: &str) -> String {
        normalize_url_for_hydrus(url).unwrap()
    }

    #[test]
    fn danbooru_and_twitter_legacy_forms_are_rewritten_once() {
        assert_eq!(
            norm("https://danbooru.donmai.us/post/show/123?q=1"),
            "https://danbooru.donmai.us/posts/123?q=1"
        );
        assert_eq!(
            norm("http://mobile.twitter.com/i/web/status/99"),
            "http://mobile.twitter.com/i/status/99"
        );
        assert_eq!(
            norm("https://x.com/i/web/status/7#x"),
            "https://x.com/i/status/7#x"
        );
        // Not anchored at the start, or not a numeric id: left alone.
        assert_eq!(
            norm("see https://x.com/i/web/status/7"),
            "see https://x.com/i/web/status/7"
        );
        assert_eq!(
            norm("https://danbooru.donmai.us/post/show/abc"),
            "https://danbooru.donmai.us/post/show/abc"
        );
    }

    #[test]
    fn rule34_us_encoded_slash_is_decoded_in_the_r_parameter_only() {
        assert_eq!(
            norm("https://rule34.us/index.php?r=posts%2Fview&id=5%2f6"),
            "https://rule34.us/index.php?r=posts/view&id=5%2f6"
        );
        assert_eq!(
            norm("https://rule34.us/index.php?id=1&r=posts%2FView#a%2fb"),
            "https://rule34.us/index.php?id=1&r=posts/View#a%2fb"
        );
        // Another host with the name in its path is not rule34.us.
        assert_eq!(
            norm("https://example.com/rule34.us?r=a%2Fb"),
            "https://example.com/rule34.us?r=a%2Fb"
        );
    }

    #[test]
    fn pixiv_legacy_urls_become_the_artwork_page() {
        assert_eq!(
            norm("https://www.pixiv.net/member_illust.php?mode=medium&illust_id=4242"),
            "https://www.pixiv.net/artworks/4242"
        );
        assert_eq!(
            norm("https://www.pixiv.net/en/artworks/17"),
            "https://www.pixiv.net/artworks/17"
        );
        assert_eq!(
            norm("https://www.pixiv.net/users/1"),
            "https://www.pixiv.net/users/1"
        );
    }

    #[test]
    fn a_malformed_rule34_us_host_is_the_value_error_urlparse_raises() {
        assert!(normalize_url_for_hydrus("http://[rule34.us/").is_err());
        assert_eq!(norm("http://[example.com/"), "http://[example.com/");
    }
}
