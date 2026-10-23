//! Which site a URL belongs to and which URL form it serves (`core/sites.py`): the host map,
//! `guess_site_name` and `canonicalize_url`.
//!
//! Only the part the engine result parsers read is here. IQDB labels each match with the site its
//! URL names; Yandex runs every post link through `canonicalize_url`. `classify_site` is here for
//! the engine runner (DAN-2063); the site option lists come with the code that filters candidates.

use std::sync::LazyLock;

use regex::Regex;

use crate::pycase;
use crate::pyurl;

/// Host -> friendly display name, in `HOST_MAP`'s order. The order is behaviour: the first entry
/// a host matches wins.
pub const HOST_MAP: [(&str, &str); 40] = [
    ("danbooru.donmai.us", "Danbooru"),
    ("gelbooru.com", "Gelbooru"),
    ("safebooru.org", "Safebooru"),
    ("rule34.xxx", "Rule34"),
    ("rule34.paheal.net", "Paheal"),
    ("paheal.net", "Paheal"),
    ("rule34.us", "Rule34.us"),
    ("reddit.com", "Reddit"),
    ("redd.it", "Reddit"),
    ("xbooru.com", "Xbooru"),
    ("yande.re", "Yande.re"),
    ("konachan.com", "Konachan"),
    ("konachan.net", "Konachan"),
    ("sankakucomplex.com", "Sankaku Complex"),
    ("chan.sankakucomplex.com", "Sankaku Complex"),
    ("e-hentai.org", "e-Hentai"),
    ("exhentai.org", "e-Hentai"),
    ("e621.net", "e621"),
    ("e926.net", "e621"),
    ("pixiv.net", "Pixiv"),
    ("anime-pictures.net", "Anime-Pictures"),
    ("e-shuushuu.net", "e-shuushuu"),
    ("zerochan.net", "Zerochan"),
    ("pawchive.pw", "Pawchive"),
    ("pawchive.org", "Pawchive"),
    ("mangadex.org", "MangaDex"),
    ("deviantart.com", "DeviantArt"),
    ("fav.me", "DeviantArt"),
    ("twitter.com", "Twitter"),
    ("x.com", "Twitter"),
    ("t.co", "Twitter"),
    ("pbs.twimg.com", "Twitter"),
    ("fxtwitter.com", "Twitter"),
    ("vxtwitter.com", "Twitter"),
    ("fixupx.com", "Twitter"),
    ("fixvx.com", "Twitter"),
    ("mangaupdates.com", "MangaUpdates"),
    ("myanimelist.net", "MyAnimeList"),
    ("anilist.co", "AniList"),
    ("toon34.com", "Toon34"),
];

/// Best-effort site name for a match URL, from its host and not from a substring of the URL
/// (`t.co` as a substring would call `anilist.co` Twitter). `None` for an empty URL, one with no
/// host, and a host the map does not know.
pub fn guess_site_name(url: &str) -> Option<&'static str> {
    if url.is_empty() {
        return None;
    }
    let netloc = pyurl::urlparse(url, "").netloc.to_lowercase();
    if netloc.is_empty() {
        return None;
    }
    let host = netloc.strip_prefix("www.").unwrap_or(&netloc);
    HOST_MAP
        .iter()
        .find(|(known, _)| {
            host == *known
                || host
                    .strip_suffix(known)
                    .is_some_and(|before| before.ends_with('.'))
        })
        .map(|(_, name)| *name)
}

/// `OTHER_SITE`: the catch-all in `ALL_SITE_OPTIONS`.
pub const OTHER_SITE: &str = "Other";

/// `classify_site`: resolves a candidate match to one of `ALL_SITE_OPTIONS`. The URL's host first,
/// then `source_name` if it is already one of the known names (`KNOWN_SITES` is the set of
/// `HOST_MAP`'s values), else `"Other"`.
pub fn classify_site(source_name: Option<&str>, url: &str) -> String {
    if let Some(guessed) = guess_site_name(url) {
        return guessed.to_owned();
    }
    if let Some(name) = source_name {
        if HOST_MAP.iter().any(|(_, known)| *known == name) {
            return name.to_owned();
        }
    }
    OTHER_SITE.to_owned()
}

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
    fn classify_prefers_the_host_then_a_known_name_then_other() {
        assert_eq!(
            classify_site(Some("Whatever"), "https://danbooru.donmai.us/posts/1"),
            "Danbooru"
        );
        assert_eq!(
            classify_site(Some("Gelbooru"), "https://example.invalid/x"),
            "Gelbooru"
        );
        assert_eq!(
            classify_site(Some("nope"), "https://example.invalid/x"),
            "Other"
        );
        assert_eq!(classify_site(None, ""), "Other");
    }

    #[test]
    fn a_host_and_its_subdomains_match() {
        assert_eq!(
            guess_site_name("https://danbooru.donmai.us/posts/1"),
            Some("Danbooru")
        );
        assert_eq!(
            guess_site_name("https://WWW.Pixiv.net/artworks/1"),
            Some("Pixiv")
        );
        assert_eq!(guess_site_name("https://img.e621.net/x"), Some("e621"));
        assert_eq!(
            guess_site_name("https://rule34.paheal.net/post/1"),
            Some("Paheal")
        );
    }

    #[test]
    fn the_host_is_compared_whole_not_as_a_substring() {
        // `t.co` is a substring of `anilist.co`; the map has both.
        assert_eq!(
            guess_site_name("https://anilist.co/anime/1"),
            Some("AniList")
        );
        assert_eq!(guess_site_name("https://t.co/abc"), Some("Twitter"));
        assert_eq!(guess_site_name("https://nott.co/abc"), None);
        assert_eq!(guess_site_name("https://fakex.com/abc"), None);
        assert_eq!(guess_site_name("https://x.com.evil.example/abc"), None);
    }

    #[test]
    fn no_host_or_no_url_is_none() {
        assert_eq!(guess_site_name(""), None);
        assert_eq!(guess_site_name("/posts/1"), None);
        assert_eq!(guess_site_name("not a url"), None);
        // The port is part of Python's netloc, so it does not match.
        assert_eq!(guess_site_name("https://pixiv.net:8080/x"), None);
    }

    // `tests/test_urls_and_availability.py::TestSankakuLegacyUrlRewrite` and
    // `TestReactorFullResUrlRewrite` at `76c85c4`, test for test, then what those tests do not
    // say.

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
