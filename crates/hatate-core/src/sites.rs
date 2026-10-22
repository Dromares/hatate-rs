//! Which site a URL belongs to (`core/sites.py`): the host map and `guess_site_name`.
//!
//! Only the part the engine result parsers read is here. IQDB labels each match with the site its
//! URL names; `classify_site`, `canonicalize_url` and the site lists come with the code that
//! filters candidates.

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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            guess_site_name("https://img.e621.net/x"),
            Some("e621")
        );
        assert_eq!(guess_site_name("https://rule34.paheal.net/post/1"), Some("Paheal"));
    }

    #[test]
    fn the_host_is_compared_whole_not_as_a_substring() {
        // `t.co` is a substring of `anilist.co`; the map has both.
        assert_eq!(guess_site_name("https://anilist.co/anime/1"), Some("AniList"));
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
}
