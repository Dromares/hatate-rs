//! Which URL form a site really serves (`core/sites.py`): `canonicalize_url`.
//!
//! Only the part the engine result parsers read is here (Yandex runs every post link through it).
//! The host map, `guess_site_name` and `classify_site` come with the code that filters
//! candidates.

use std::sync::LazyLock;

use regex::Regex;

use crate::pycase;
use crate::pyurl;

/// `SANKAKU_LEGACY_POST_RE`. Python compiles it with `re.IGNORECASE`; no letter in it has a case
/// that Python adds and `(?i)` lacks (those are `i` and `ı`/`İ`), so `(?i)` is the same.
static SANKAKU_LEGACY_POST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(https?://[^/]*sankakucomplex\.com)/post/show/(\d+)")
        .expect("sankaku legacy regex")
});

/// The prefix of `REACTOR_PICS_POST_RE`,
/// `^(https?://[^/]+/pics/post/)(?!full/)(.+)$` with `re.IGNORECASE`. The crate has no
/// lookahead, so [`canonicalize_url`] reads the rest by hand. Python's `i` also matches `ı` and
/// `İ`, which `(?i)` does not, so the one `i` is spelled out.
static REACTOR_PICS_POST_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(https?://[^/]+/p[i\u{131}\u{130}]cs/post/)").expect("reactor prefix regex")
});

const REACTOR_HOSTS: [&str; 3] = ["reactor.cc", "joyreactor.com", "joyreactor.cc"];

fn is_reactor_host(netloc: &str) -> bool {
    let netloc = netloc.strip_prefix("www.").unwrap_or(netloc);
    REACTOR_HOSTS.iter().any(|host| {
        netloc == *host
            || netloc
                .strip_suffix(host)
                .is_some_and(|before| before.ends_with('.'))
    })
}

/// Rewrites known-stale or known-degraded URL forms to the one the site serves: Sankaku's
/// deprecated `/post/show/{id}` becomes `/en/posts/{id}`, and a JoyReactor/reactor.cc
/// `/pics/post/<file>` (the watermarked copy) becomes `/pics/post/full/<file>`. Anything else is
/// returned untouched.
pub fn canonicalize_url(url: &str) -> String {
    if url.is_empty() {
        return String::new();
    }
    if let Some(m) = SANKAKU_LEGACY_POST_RE.captures(url) {
        return format!("{}/en/posts/{}", &m[1], &m[2]);
    }
    if let Some(m) = REACTOR_PICS_POST_PREFIX.captures(url) {
        let rest = &url[m[0].len()..];
        // `(?!full/)(.+)$`: not "full/" in any case, then one or more characters other than a
        // newline, up to the end or to one final newline (which the group leaves out).
        let starts_full = rest
            .chars()
            .zip("full/".chars())
            .take_while(|(t, p)| pycase::ignorecase_matches(*p, *t))
            .count()
            == 5;
        let body = rest.strip_suffix('\n').unwrap_or(rest);
        if !starts_full && !body.is_empty() && !body.contains('\n') {
            // A ValueError from `urlparse` is caught: the URL is then left alone.
            let netloc = pyurl::try_urlparse(url, "")
                .map(|parsed| parsed.netloc.to_lowercase())
                .unwrap_or_default();
            if !netloc.is_empty() && is_reactor_host(&netloc) {
                return format!("{}full/{}", &m[1], body);
            }
        }
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    //! `tests/test_urls_and_availability.py::TestSankakuLegacyUrlRewrite` and
    //! `TestReactorFullResUrlRewrite` at `76c85c4`, test for test, then what those tests do not
    //! say.

    use super::*;

    #[test]
    fn legacy_route_is_rewritten() {
        assert_eq!(
            canonicalize_url("https://chan.sankakucomplex.com/post/show/4100422"),
            "https://chan.sankakucomplex.com/en/posts/4100422"
        );
    }

    #[test]
    fn query_string_is_dropped_with_the_old_route() {
        assert_eq!(
            canonicalize_url("https://chan.sankakucomplex.com/post/show/4100422?tags=x"),
            "https://chan.sankakucomplex.com/en/posts/4100422"
        );
    }

    #[test]
    fn modern_route_is_untouched() {
        let url = "https://chan.sankakucomplex.com/en/posts/4100422";
        assert_eq!(canonicalize_url(url), url);
    }

    #[test]
    fn other_sites_are_untouched() {
        // Danbooru also serves /post/show/{id} and handles it fine.
        for url in [
            "https://danbooru.donmai.us/post/show/2577264",
            "https://gelbooru.com/index.php?page=post&s=view&id=1",
            "https://yande.re/post/show/123",
        ] {
            assert_eq!(canonicalize_url(url), url);
        }
    }

    #[test]
    fn empty_and_junk_are_safe() {
        assert_eq!(canonicalize_url(""), "");
        assert_eq!(canonicalize_url("not a url"), "not a url");
    }

    const LOW: &str = "https://img10.reactor.cc/pics/post/egoswans-Rebecca-(Edgerunners)-Cyberpunk-Edgerunners-Cyberpunk-2077-8192103.jpeg";
    const HIGH: &str = "https://img10.reactor.cc/pics/post/full/egoswans-Rebecca-(Edgerunners)-Cyberpunk-Edgerunners-Cyberpunk-2077-8192103.jpeg";

    #[test]
    fn reactor_cc_is_rewritten_to_full() {
        assert_eq!(canonicalize_url(LOW), HIGH);
    }

    #[test]
    fn joyreactor_com_host_is_also_rewritten() {
        assert_eq!(
            canonicalize_url("https://joyreactor.com/pics/post/a-1234.jpg"),
            "https://joyreactor.com/pics/post/full/a-1234.jpg"
        );
    }

    #[test]
    fn joyreactor_cc_host_is_also_rewritten() {
        assert_eq!(
            canonicalize_url("https://joyreactor.cc/pics/post/a-1234.jpg"),
            "https://joyreactor.cc/pics/post/full/a-1234.jpg"
        );
    }

    #[test]
    fn already_full_is_left_alone() {
        assert_eq!(canonicalize_url(HIGH), HIGH);
    }

    #[test]
    fn non_pics_post_path_is_left_alone() {
        let url = "https://reactor.cc/post/1234567";
        assert_eq!(canonicalize_url(url), url);
    }

    #[test]
    fn unrelated_host_with_a_similar_path_is_left_alone() {
        let url = "https://example.com/pics/post/a-1234.jpg";
        assert_eq!(canonicalize_url(url), url);
    }

    #[test]
    fn case_does_not_matter_to_the_pattern_or_the_host() {
        assert_eq!(
            canonicalize_url("HTTPS://X.SankakuComplex.com/Post/Show/9"),
            "HTTPS://X.SankakuComplex.com/en/posts/9"
        );
        assert_eq!(
            canonicalize_url("https://IMG.Reactor.CC/PICS/POST/a-1.jpeg"),
            "https://IMG.Reactor.CC/PICS/POST/full/a-1.jpeg"
        );
        assert_eq!(
            canonicalize_url("https://img10.reactor.cc/pics/post/FULL/a-1.jpeg"),
            "https://img10.reactor.cc/pics/post/FULL/a-1.jpeg"
        );
    }

    #[test]
    fn python_dollar_drops_one_final_newline_and_dot_stops_at_a_newline() {
        assert_eq!(
            canonicalize_url("https://x.reactor.cc/pics/post/a.jpg\n"),
            "https://x.reactor.cc/pics/post/full/a.jpg"
        );
        assert_eq!(
            canonicalize_url("https://x.reactor.cc/pics/post/a\nb.jpg"),
            "https://x.reactor.cc/pics/post/a\nb.jpg"
        );
        assert_eq!(
            canonicalize_url("https://x.reactor.cc/pics/post/"),
            "https://x.reactor.cc/pics/post/"
        );
    }

    #[test]
    fn a_netloc_python_refuses_is_left_alone() {
        // `urlparse` raises ValueError on the unbalanced bracket; Python's `except ValueError`
        // then skips the rewrite.
        assert_eq!(
            canonicalize_url("https://[x.reactor.cc/pics/post/a.jpg"),
            "https://[x.reactor.cc/pics/post/a.jpg"
        );
    }
}
