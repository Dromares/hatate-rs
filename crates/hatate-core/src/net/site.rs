//! Site access: the headers, cookies and credentials a site needs before it will
//! answer as it answers a browser. A port of `core/site_access.py` (docs/net.md section 6).

use http::HeaderMap;
use http::header::{
    ACCEPT, AUTHORIZATION, HeaderName, HeaderValue, REFERER, USER_AGENT as USER_AGENT_HEADER,
};
use url::Url;

use super::config::USER_AGENT;
use super::secret::{Cookies, Secret};

/// The browser string Pixiv, Sankaku, DeviantArt, ascii2d and Google Images need
/// (`core/site_access.py:31`, `core/ascii2d.py:37-40`).
pub const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Reddit asks API clients to name themselves this way (`core/reddit.py:31`).
pub const REDDIT_USER_AGENT: &str = "linux:hatate-linux:1.0 (reverse image tagger)";

/// The cookie carrying an Anime-Pictures login (`core/site_access.py:168-172`).
pub const ANIMEPICTURES_SESSION_COOKIE: &str = "anime_pictures_jwt";

/// The cookies DeviantArt renews with a sliding expiry (`core/site_access.py:297`).
pub const ROTATABLE_DEVIANTART_COOKIES: [&str; 3] = ["auth", "auth_secure", "userinfo"];

/// What the user configured. Every field is a secret; `Debug` prints none of them.
///
/// The field names are the config keys (docs/net.md section 9).
#[derive(Debug, Clone, Default)]
pub struct SiteCredentials {
    pub pixiv_session_cookie: Secret,
    pub sankaku_cookies: Secret,
    pub animepictures_cookies: Secret,
    pub deviantart_cookies: Secret,
    pub e621_username: Secret,
    pub e621_api_key: Secret,
}

/// Whether `host` is `domain` or a subdomain of it. Both lower-cased by the caller
/// or not: the comparison ignores ASCII case.
pub(crate) fn host_matches(host: &str, domain: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let domain = domain.to_ascii_lowercase();
    host == domain || host.ends_with(&format!(".{domain}"))
}

/// True when the URL's **host** is `domain` or a subdomain of it
/// (`core/site_access.py:368-381`).
///
/// These checks decide who is sent the user's session cookies, and the URLs come
/// from search results, which are not trusted. A substring test once handed a
/// login to `anime-pictures.net.evil.example`. A URL that does not parse matches
/// nothing.
pub fn host_is(url: &str, domain: &str) -> bool {
    let url = url.trim();
    let owned;
    let url = if url.starts_with("//") {
        owned = format!("https:{url}");
        owned.as_str()
    } else {
        url
    };
    match Url::parse(url) {
        Ok(parsed) => parsed.host_str().is_some_and(|h| host_matches(h, domain)),
        Err(_) => false,
    }
}

/// HTTP Basic auth for e621, when both halves are configured
/// (`core/site_access.py:76-89`). A half-filled header would turn every request
/// into a 401, so a missing half means no header.
pub fn e621_auth_header(creds: &SiteCredentials) -> Option<Secret> {
    let user = creds.e621_username.expose().trim();
    let key = creds.e621_api_key.expose().trim();
    if user.is_empty() || key.is_empty() {
        return None;
    }
    Some(Secret::new(format!(
        "Basic {}",
        base64(format!("{user}:{key}").as_bytes())
    )))
}

fn header(map: &mut HeaderMap, name: HeaderName, value: &str) {
    map.insert(
        name,
        HeaderValue::from_str(value).expect("static header values are valid"),
    );
}

/// Site-specific request headers, or `None` to use the default
/// (`core/site_access.py:92-119`). First match wins.
///
/// `creds` is optional because most of these depend only on the host; e621's
/// carries the user's key, so it needs them.
pub fn headers_for_url(url: &str, creds: Option<&SiteCredentials>) -> Option<HeaderMap> {
    let mut h = HeaderMap::new();
    if host_is(url, "pixiv.net") {
        header(&mut h, USER_AGENT_HEADER, BROWSER_USER_AGENT);
        header(&mut h, REFERER, "https://www.pixiv.net/");
        header(&mut h, ACCEPT, "application/json");
        return Some(h);
    }
    if host_is(url, "sankakucomplex.com") {
        header(&mut h, USER_AGENT_HEADER, BROWSER_USER_AGENT);
        header(&mut h, REFERER, "https://chan.sankakucomplex.com/");
        return Some(h);
    }
    if host_is(url, "deviantart.com") || host_is(url, "fav.me") {
        header(&mut h, USER_AGENT_HEADER, BROWSER_USER_AGENT);
        header(&mut h, REFERER, "https://www.deviantart.com/");
        return Some(h);
    }
    if host_is(url, "reddit.com") || host_is(url, "redd.it") {
        header(&mut h, USER_AGENT_HEADER, REDDIT_USER_AGENT);
        return Some(h);
    }
    if let Some(creds) = creds
        && ["e621.net", "e926.net", "e6ai.net"]
            .iter()
            .any(|d| host_is(url, d))
        && let Some(auth) = e621_auth_header(creds)
    {
        // The descriptive User-Agent goes with it: e621 blocks generic ones.
        header(&mut h, USER_AGENT_HEADER, USER_AGENT);
        let mut value = HeaderValue::from_str(auth.expose()).ok()?;
        value.set_sensitive(true);
        h.insert(AUTHORIZATION, value);
        return Some(h);
    }
    None
}

/// Parses a browser `Cookie:` header value or a pasted cookie list
/// (`core/site_access.py:122-152`).
///
/// Tolerant on purpose: extra whitespace, a trailing `;`, newlines as separators,
/// and values containing `=` (base64 and JWTs do) all work. A part without `=`
/// or with an empty name is dropped; a repeated name keeps the last value.
pub fn parse_cookie_string(raw: &str) -> Cookies {
    let mut cookies = Cookies::new();
    let normalized = raw.replace('\r', "\n").replace('\n', ";");
    for part in normalized.split(';') {
        let part = part.trim();
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if !name.is_empty() {
            cookies.set(name, value.trim());
        }
    }
    cookies
}

/// The user's Anime-Pictures cookies, accepting a bare token too
/// (`core/site_access.py:168-200`): a lone run of non-space text with no `;`
/// and no `=` is paired with the session cookie's known name.
pub fn animepictures_cookies(raw: &str) -> Option<Cookies> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let parsed = parse_cookie_string(raw);
    if !parsed.is_empty() {
        return Some(parsed);
    }
    if !raw.contains(';') && raw.split_whitespace().count() == 1 {
        let mut cookies = Cookies::new();
        cookies.set(ANIMEPICTURES_SESSION_COOKIE, raw);
        return Some(cookies);
    }
    None
}

/// Site-specific auth cookies, when the user configured them
/// (`core/site_access.py:250-286`). `None`, not an empty jar, when nothing is set.
///
/// The returned cookies are scoped to the site's own domains, so they follow a
/// redirect within the site (`fav.me` to `deviantart.com`) and nowhere else.
pub fn cookies_for_url(url: &str, creds: &SiteCredentials) -> Option<Cookies> {
    if host_is(url, "pixiv.net") {
        let value = creds.pixiv_session_cookie.expose().trim();
        if !value.is_empty() {
            let mut c = Cookies::new().with_scope(["pixiv.net"]);
            c.set("PHPSESSID", value);
            return Some(c);
        }
    }
    if host_is(url, "sankakucomplex.com") && !creds.sankaku_cookies.expose().trim().is_empty() {
        return non_empty(
            parse_cookie_string(creds.sankaku_cookies.expose()),
            ["sankakucomplex.com"],
        );
    }
    if host_is(url, "anime-pictures.net") {
        return animepictures_cookies(creds.animepictures_cookies.expose())
            .and_then(|c| non_empty(c, ["anime-pictures.net"]));
    }
    if (host_is(url, "deviantart.com") || host_is(url, "fav.me"))
        && !creds.deviantart_cookies.expose().trim().is_empty()
    {
        return non_empty(
            parse_cookie_string(creds.deviantart_cookies.expose()),
            ["deviantart.com", "fav.me"],
        );
    }
    None
}

fn non_empty<const N: usize>(cookies: Cookies, scope: [&str; N]) -> Option<Cookies> {
    (!cookies.is_empty()).then(|| cookies.with_scope(scope))
}

/// The result of folding DeviantArt's renewed session cookies into the configured ones.
#[derive(Debug, Clone)]
pub struct RotatedCookies {
    /// The new `deviantart_cookies` value, `k=v; k=v`.
    pub cookies: Secret,
    /// Which cookies changed, sorted. Safe to log; the values are not.
    pub names: Vec<String>,
}

/// Folds a renewed DeviantArt session cookie into the configured string
/// (`core/site_access.py:289-365`). The caller saves the result (ticket 1.d).
///
/// `response_cookies` are the `name=value` pairs of the response's `Set-Cookie`
/// headers. Returns `None` (change nothing) unless: the URL is `deviantart.com` or
/// `fav.me`; the status is 2xx or 304 (a challenge page's cookie must never
/// overwrite a working session); one of `auth`, `auth_secure`, `userinfo` came back
/// with a value; a session is already configured; and some value actually differs.
pub fn merge_rotated_deviantart_cookies(
    url: &str,
    status: u16,
    response_cookies: &[(String, String)],
    current: &str,
) -> Option<RotatedCookies> {
    if !(host_is(url, "deviantart.com") || host_is(url, "fav.me")) {
        return None;
    }
    if !((200..300).contains(&status) || status == 304) {
        return None;
    }
    let mut rotated: Vec<(&str, &str)> = Vec::new();
    for (name, value) in response_cookies {
        if ROTATABLE_DEVIANTART_COOKIES.contains(&name.as_str()) && !value.is_empty() {
            match rotated.iter_mut().find(|(n, _)| n == name) {
                Some(slot) => slot.1 = value,
                None => rotated.push((name, value)),
            }
        }
    }
    if rotated.is_empty() {
        return None;
    }
    let mut merged = parse_cookie_string(current);
    if merged.is_empty() {
        return None;
    }
    if rotated
        .iter()
        .all(|(n, v)| merged.get(n).is_some_and(|s| s.expose() == *v))
    {
        return None;
    }
    for (name, value) in &rotated {
        merged.set(*name, *value);
    }
    let mut names: Vec<String> = rotated.iter().map(|(n, _)| (*n).to_owned()).collect();
    names.sort();
    Some(RotatedCookies {
        cookies: merged.header_value()?,
        names,
    })
}

/// Standard base64 with padding (RFC 4648), for the e621 Basic header.
fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(n, v)| ((*n).into(), (*v).into()))
            .collect()
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (input, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), want);
        }
    }

    #[test]
    fn host_is_matches_hosts_not_substrings() {
        assert!(host_is("https://www.pixiv.net/artworks/1", "pixiv.net"));
        assert!(host_is("https://PIXIV.NET/x", "pixiv.net"));
        assert!(host_is("//fav.me/dabc", "fav.me"));
        assert!(!host_is(
            "https://anime-pictures.net.evil.example/x",
            "anime-pictures.net"
        ));
        assert!(!host_is(
            "https://evil.example/?u=https://pixiv.net/",
            "pixiv.net"
        ));
        assert!(!host_is("https://notpixiv.net/", "pixiv.net"));
        assert!(!host_is("pixiv.net/artworks/1", "pixiv.net"));
        assert!(!host_is("", "pixiv.net"));
        assert!(!host_is("http://[bad", "pixiv.net"));
    }

    #[test]
    fn parse_cookie_string_is_tolerant() {
        let c = parse_cookie_string(
            "  login=you ; pass_hash=abc=def==;\r\nthird=3\n;;junk; =noname; last = x ;",
        );
        assert_eq!(
            c.names().collect::<Vec<_>>(),
            ["login", "pass_hash", "third", "last"]
        );
        assert_eq!(c.get("pass_hash").unwrap().expose(), "abc=def==");
        assert_eq!(c.get("last").unwrap().expose(), "x");
        let dup = parse_cookie_string("a=1; b=2; a=3");
        assert_eq!(dup.header_value().unwrap().expose(), "a=3; b=2");
        assert!(parse_cookie_string("").is_empty());
        assert!(parse_cookie_string("nothing here").is_empty());
    }

    #[test]
    fn animepictures_accepts_a_bare_token() {
        let c = animepictures_cookies("  eyJhbGciOiJIUzI1NiJ9.payload.sig  ").unwrap();
        assert_eq!(
            c.get("anime_pictures_jwt").unwrap().expose(),
            "eyJhbGciOiJIUzI1NiJ9.payload.sig"
        );
        assert!(animepictures_cookies("two words").is_none());
        assert!(animepictures_cookies("a b; c").is_none());
        assert!(animepictures_cookies("   ").is_none());
        let named = animepictures_cookies("anime_pictures_jwt=tok; other=1").unwrap();
        assert_eq!(named.len(), 2);
    }

    #[test]
    fn headers_follow_the_site_table() {
        let pixiv = headers_for_url("https://www.pixiv.net/ajax/illust/1", None).unwrap();
        assert_eq!(pixiv[USER_AGENT_HEADER], BROWSER_USER_AGENT);
        assert_eq!(pixiv[REFERER], "https://www.pixiv.net/");
        assert_eq!(pixiv[ACCEPT], "application/json");
        let sankaku = headers_for_url("https://chan.sankakucomplex.com/posts/1", None).unwrap();
        assert_eq!(sankaku[REFERER], "https://chan.sankakucomplex.com/");
        assert!(!sankaku.contains_key(ACCEPT));
        let fav = headers_for_url("https://fav.me/dabc", None).unwrap();
        assert_eq!(fav[REFERER], "https://www.deviantart.com/");
        let reddit = headers_for_url("https://www.reddit.com/r/x", None).unwrap();
        assert_eq!(reddit[USER_AGENT_HEADER], REDDIT_USER_AGENT);
        assert_eq!(reddit.len(), 1);
        assert!(headers_for_url("https://danbooru.donmai.us/posts/1", None).is_none());
    }

    #[test]
    fn e621_needs_both_halves_and_settings() {
        let url = "https://e621.net/posts/1.json";
        assert!(headers_for_url(url, None).is_none());
        let mut creds = SiteCredentials::default();
        assert!(headers_for_url(url, Some(&creds)).is_none());
        creds.e621_username = "user".into();
        assert!(
            headers_for_url(url, Some(&creds)).is_none(),
            "a username with no key sends nothing"
        );
        creds.e621_api_key = " key ".into();
        let h = headers_for_url(url, Some(&creds)).unwrap();
        assert_eq!(h[USER_AGENT_HEADER], USER_AGENT);
        assert_eq!(h[AUTHORIZATION], "Basic dXNlcjprZXk=");
        assert!(h[AUTHORIZATION].is_sensitive());
        assert!(headers_for_url("https://e926.net/x", Some(&creds)).is_some());
        assert!(headers_for_url("https://e6ai.net/x", Some(&creds)).is_some());
        assert!(headers_for_url("https://e621.net.evil.example/x", Some(&creds)).is_none());
    }

    #[test]
    fn cookies_follow_the_site_table() {
        let creds = SiteCredentials {
            pixiv_session_cookie: "  12345_abc  ".into(),
            sankaku_cookies: "login=you; _sankakuchannel_session=s".into(),
            animepictures_cookies: "tokenvalue".into(),
            deviantart_cookies: "auth=a; userinfo=u".into(),
            ..SiteCredentials::default()
        };
        let pixiv = cookies_for_url("https://www.pixiv.net/x", &creds).unwrap();
        assert_eq!(pixiv.get("PHPSESSID").unwrap().expose(), "12345_abc");
        assert_eq!(pixiv.scope(), ["pixiv.net"]);
        assert_eq!(
            cookies_for_url("https://chan.sankakucomplex.com/x", &creds)
                .unwrap()
                .len(),
            2
        );
        let ap = cookies_for_url("https://api.anime-pictures.net/x", &creds).unwrap();
        assert!(ap.get("anime_pictures_jwt").is_some());
        let da = cookies_for_url("https://fav.me/x", &creds).unwrap();
        assert_eq!(da.scope(), ["deviantart.com", "fav.me"]);
        assert!(cookies_for_url("https://danbooru.donmai.us/x", &creds).is_none());
        assert!(cookies_for_url("https://www.pixiv.net/x", &SiteCredentials::default()).is_none());
        let unparsable = SiteCredentials {
            sankaku_cookies: "no equals here".into(),
            ..SiteCredentials::default()
        };
        assert!(cookies_for_url("https://chan.sankakucomplex.com/x", &unparsable).is_none());
    }

    #[test]
    fn site_credentials_debug_prints_no_value() {
        let creds = SiteCredentials {
            pixiv_session_cookie: "synthetic-pixiv".into(),
            e621_api_key: "synthetic-e621".into(),
            ..SiteCredentials::default()
        };
        let dump = format!("{creds:?} {creds:#?}");
        assert!(!dump.contains("synthetic"), "{dump}");
    }

    #[test]
    fn rotation_follows_site_access_rules() {
        let url = "https://www.deviantart.com/x";
        let current = "auth=old; auth_secure=oldsecure; extra=keep";
        let got = merge_rotated_deviantart_cookies(
            url,
            200,
            &pairs(&[("auth", "new"), ("userinfo", "u"), ("other", "z")]),
            current,
        )
        .unwrap();
        assert_eq!(
            got.cookies.expose(),
            "auth=new; auth_secure=oldsecure; extra=keep; userinfo=u"
        );
        assert_eq!(got.names, ["auth", "userinfo"]);
        assert!(
            merge_rotated_deviantart_cookies(url, 304, &pairs(&[("auth", "new")]), current)
                .is_some()
        );
        // Not a success: never merged.
        for status in [199, 300, 403, 429, 500] {
            assert!(
                merge_rotated_deviantart_cookies(url, status, &pairs(&[("auth", "new")]), current)
                    .is_none()
            );
        }
        // Another host, nothing rotatable, empty value, no session, same values.
        assert!(
            merge_rotated_deviantart_cookies(
                "https://pixiv.net/x",
                200,
                &pairs(&[("auth", "new")]),
                current
            )
            .is_none()
        );
        assert!(
            merge_rotated_deviantart_cookies(url, 200, &pairs(&[("other", "z")]), current)
                .is_none()
        );
        assert!(
            merge_rotated_deviantart_cookies(url, 200, &pairs(&[("auth", "")]), current).is_none()
        );
        assert!(
            merge_rotated_deviantart_cookies(url, 200, &pairs(&[("auth", "new")]), "").is_none()
        );
        assert!(
            merge_rotated_deviantart_cookies(url, 200, &pairs(&[("auth", "old")]), current)
                .is_none()
        );
        assert!(
            merge_rotated_deviantart_cookies(
                "https://fav.me/x",
                200,
                &pairs(&[("auth", "new")]),
                current
            )
            .is_some()
        );
    }
}
