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
    use super::*;

    #[test]
    fn sankakus_legacy_route_becomes_the_posts_route() {
        assert_eq!(
            canonicalize_url("https://chan.sankakucomplex.com/post/show/123?x=1"),
            "https://chan.sankakucomplex.com/en/posts/123"
        );
        assert_eq!(
            canonicalize_url("HTTPS://X.SankakuComplex.com/Post/Show/9"),
            "HTTPS://X.SankakuComplex.com/en/posts/9"
        );
        assert_eq!(
            canonicalize_url("https://sankakucomplex.com/en/posts/123"),
            "https://sankakucomplex.com/en/posts/123"
        );
    }

    #[test]
    fn a_reactor_picture_gets_its_unwatermarked_twin() {
        for host in [
            "img10.reactor.cc",
            "joyreactor.com",
            "www.joyreactor.cc",
            "IMG.Reactor.CC",
        ] {
            assert_eq!(
                canonicalize_url(&format!("https://{host}/pics/post/a-1.jpeg")),
                format!("https://{host}/pics/post/full/a-1.jpeg"),
                "{host}"
            );
        }
        // Already the full copy, in any case: left alone.
        assert_eq!(
            canonicalize_url("https://img10.reactor.cc/pics/post/FULL/a-1.jpeg"),
            "https://img10.reactor.cc/pics/post/FULL/a-1.jpeg"
        );
        // Not a reactor host, or not under /pics/post/: left alone.
        assert_eq!(
            canonicalize_url("https://example.org/pics/post/a-1.jpeg"),
            "https://example.org/pics/post/a-1.jpeg"
        );
        assert_eq!(
            canonicalize_url("https://notreactor.cc/pics/post/a-1.jpeg"),
            "https://notreactor.cc/pics/post/a-1.jpeg"
        );
        assert_eq!(canonicalize_url(""), "");
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
