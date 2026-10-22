//! The parts of Python's `urllib.parse` that the site parsers call: `urljoin`, `parse_qsl` and
//! `unquote`, with the `urlsplit`/`urlunsplit` they stand on (CPython 3.14, which recorded the
//! corpus).
//!
//! The `url` crate follows the WHATWG URL standard and Python follows RFC 3986 with its own
//! quirks: the two disagree on exactly the inputs a parser meets on a broken page (a `src` with
//! no scheme, a query with a bare `&`, a stray `%`). The corpus is Python's output, so this is
//! Python's algorithm, not the standard's.
//!
//! `urlparse` never fails here: Python raises `ValueError` for a malformed bracketed host or a
//! netloc that NFKC normalisation changes, and the booru hooks that call `urlparse` are then
//! skipped by `fetch_page_info`, so they read such a URL as any other. [`try_urlparse`] is the
//! one that fails where Python does, for the engine parsers (DAN-2009), whose Python code
//! catches that `ValueError` (Yandex's `post_url`, `host_label`) or lets it propagate.

/// Schemes that take a `;params` part on the last path segment (`uses_params`).
const USES_PARAMS: [&str; 16] = [
    "", "ftp", "hdl", "prospero", "http", "imap", "https", "shttp", "rtsp", "rtsps", "rtspu",
    "sip", "sips", "mms", "sftp", "tel",
];

/// Schemes whose relative references resolve against a base (`uses_relative`).
const USES_RELATIVE: [&str; 20] = [
    "", "ftp", "http", "gopher", "nntp", "imap", "wais", "file", "https", "shttp", "mms",
    "prospero", "rtsp", "rtsps", "rtspu", "sftp", "svn", "svn+ssh", "ws", "wss",
];

/// Schemes that have a `//netloc` part (`uses_netloc`).
const USES_NETLOC: [&str; 27] = [
    "",
    "ftp",
    "http",
    "gopher",
    "nntp",
    "telnet",
    "imap",
    "wais",
    "file",
    "mms",
    "https",
    "shttp",
    "snews",
    "prospero",
    "rtsp",
    "rtsps",
    "rtspu",
    "rsync",
    "svn",
    "svn+ssh",
    "sftp",
    "nfs",
    "git",
    "git+ssh",
    "ws",
    "wss",
    "itms-services",
];

/// What `urlparse` returns, as owned strings. `params` is empty for a scheme that has none.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Parsed {
    pub scheme: String,
    pub netloc: String,
    pub path: String,
    pub params: String,
    pub query: String,
    pub fragment: String,
}

/// `_urlsplit`: the five components, each `None` where the URL has no such delimiter. The
/// difference between `None` and `Some("")` (a bare `?` or `#`) is kept, because `urljoin` keeps
/// it.
struct Split {
    scheme: Option<String>,
    netloc: Option<String>,
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

fn split(url: &str, default_scheme: Option<&str>) -> Split {
    // Leading C0 controls and spaces go, then tab, CR and LF anywhere.
    let url: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let mut rest = url.as_str();
    let mut scheme = default_scheme.map(|s| {
        s.trim_matches(|c: char| c <= ' ')
            .chars()
            .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
            .collect::<String>()
    });
    if let Some(i) = rest.find(':')
        && i > 0
        && rest.starts_with(|c: char| c.is_ascii_alphabetic())
        && rest[..i]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        scheme = Some(rest[..i].to_ascii_lowercase());
        rest = &rest[i + 1..];
    }
    let mut netloc = None;
    if let Some(after) = rest.strip_prefix("//") {
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        netloc = Some(after[..end].to_string());
        rest = &after[end..];
    }
    let mut fragment = None;
    if let Some((before, frag)) = rest.split_once('#') {
        rest = before;
        fragment = Some(frag.to_string());
    }
    let mut query = None;
    if let Some((before, q)) = rest.split_once('?') {
        rest = before;
        query = Some(q.to_string());
    }
    Split {
        scheme,
        netloc,
        path: rest.to_string(),
        query,
        fragment,
    }
}

/// `_urlunsplit`.
fn unsplit(
    scheme: Option<&str>,
    netloc: Option<&str>,
    path: &str,
    query: Option<&str>,
    fragment: Option<&str>,
) -> String {
    let mut url = path.to_string();
    if let Some(netloc) = netloc {
        if !url.is_empty() && !url.starts_with('/') {
            url.insert(0, '/');
        }
        url = format!("//{netloc}{url}");
    } else if url.starts_with("//") {
        url = format!("//{url}");
    }
    if let Some(scheme) = scheme.filter(|s| !s.is_empty()) {
        url = format!("{scheme}:{url}");
    }
    if let Some(query) = query {
        url = format!("{url}?{query}");
    }
    if let Some(fragment) = fragment {
        url = format!("{url}#{fragment}");
    }
    url
}

/// What `urlparse` raises as `ValueError`: an unbalanced or invalid bracketed host, or a netloc
/// whose NFKC form carries `/ ? # @ :`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UrlValueError;

impl std::fmt::Display for UrlValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("urlparse raised ValueError")
    }
}

impl std::error::Error for UrlValueError {}

/// `urlparse(url, scheme)`: `scheme` is what a URL with no scheme of its own gets.
pub fn urlparse(url: &str, default_scheme: &str) -> Parsed {
    parsed_from(split(url, Some(default_scheme)))
}

/// `urlparse(url, scheme)` with the `ValueError`s `_urlsplit` raises for the netloc (CPython
/// 3.14): a `[` without a `]` or the reverse, a bracketed host that is not an IPv6 address (or an
/// IPvFuture literal), data before the `[` or between the `]` and the port, and a non-ASCII
/// netloc that NFKC normalisation turns into one with `/ ? # @ :` in it.
pub fn try_urlparse(url: &str, default_scheme: &str) -> Result<Parsed, UrlValueError> {
    let s = split(url, Some(default_scheme));
    if let Some(netloc) = &s.netloc {
        check_netloc(netloc)?;
    }
    Ok(parsed_from(s))
}

/// `urlunparse(parts)` (`ParseResult.geturl()`): empty components are left out, as Python does.
pub fn urlunparse(p: &Parsed) -> String {
    let netloc = if !p.netloc.is_empty() {
        Some(p.netloc.as_str())
    } else if USES_NETLOC.contains(&p.scheme.as_str())
        && !p.scheme.is_empty()
        && (p.path.is_empty() || p.path.starts_with('/'))
    {
        Some("")
    } else {
        None
    };
    let path = if p.params.is_empty() {
        p.path.clone()
    } else {
        format!("{};{}", p.path, p.params)
    };
    fn some(s: &str) -> Option<&str> {
        (!s.is_empty()).then_some(s)
    }
    unsplit(
        some(&p.scheme),
        netloc,
        &path,
        some(&p.query),
        some(&p.fragment),
    )
}

fn check_netloc(netloc: &str) -> Result<(), UrlValueError> {
    let (open, close) = (netloc.contains('['), netloc.contains(']'));
    if open != close {
        return Err(UrlValueError);
    }
    if open {
        check_bracketed_netloc(netloc)?;
    }
    // `_checknetloc`: looking for characters like U+2100 that expand to `a/c`.
    if netloc.is_ascii() {
        return Ok(());
    }
    let n: String = netloc
        .chars()
        .filter(|c| !matches!(c, '@' | ':' | '#' | '?'))
        .collect();
    let normalised = icu_normalizer::ComposingNormalizerBorrowed::new_nfkc().normalize(&n);
    if n == normalised {
        return Ok(());
    }
    if normalised.contains(['/', '?', '#', '@', ':']) {
        return Err(UrlValueError);
    }
    Ok(())
}

/// `_check_bracketed_netloc`, which mirrors how `_hostinfo` splits a netloc.
fn check_bracketed_netloc(netloc: &str) -> Result<(), UrlValueError> {
    let host_port = netloc.rsplit_once('@').map_or(netloc, |(_, rest)| rest);
    let hostname = if let Some((before, bracketed)) = host_port.split_once('[') {
        if !before.is_empty() {
            return Err(UrlValueError);
        }
        let (hostname, port) = bracketed.split_once(']').unwrap_or((bracketed, ""));
        if !port.is_empty() && !port.starts_with(':') {
            return Err(UrlValueError);
        }
        hostname
    } else {
        host_port.split_once(':').map_or(host_port, |(h, _)| h)
    };
    // `_check_bracketed_host`: an IPvFuture literal, or an address that is IPv6 (an IPv4 one
    // "cannot be in brackets").
    if hostname.starts_with(['v', 'V']) {
        let mut chars = hostname.chars();
        chars.next();
        let rest = chars.as_str();
        let digits = rest.chars().take_while(char::is_ascii_hexdigit).count();
        let after = &rest[digits..];
        // `\A[vV][a-fA-F0-9]+\..+\z`; `.` is any character but a newline.
        let ok =
            digits > 0 && after.starts_with('.') && after.len() > 1 && !after[1..].contains('\n');
        return if ok { Ok(()) } else { Err(UrlValueError) };
    }
    if is_ipv6(hostname) {
        Ok(())
    } else {
        Err(UrlValueError)
    }
}

/// `ipaddress.IPv6Address(text)` succeeds (CPython 3.14): up to eight hextets of one to four hex
/// digits with at most one `::`, an IPv4 dotted quad allowed as the last piece, and an optional
/// `%scope` that is non-empty and holds no `%` or `/`.
fn is_ipv6(text: &str) -> bool {
    if text.contains('/') {
        return false;
    }
    let addr = match text.split_once('%') {
        Some((_, scope)) if scope.is_empty() || scope.contains('%') => return false,
        Some((addr, _)) => addr,
        None => text,
    };
    if addr.is_empty() {
        return false;
    }
    let mut parts: Vec<String> = addr.split(':').map(str::to_string).collect();
    if parts.len() < 3 {
        return false;
    }
    if parts[parts.len() - 1].contains('.') {
        let Some(quad) = parts.pop().as_deref().and_then(ipv4_quad) else {
            return false;
        };
        parts.push(format!("{:x}", quad >> 16));
        parts.push(format!("{:x}", quad & 0xFFFF));
    }
    if parts.len() > 9 {
        return false;
    }
    let mut skip_index = None;
    for i in 1..parts.len() - 1 {
        if parts[i].is_empty() {
            if skip_index.is_some() {
                return false;
            }
            skip_index = Some(i);
        }
    }
    let (hi, lo) = match skip_index {
        Some(skip) => {
            let mut hi = skip;
            let mut lo = parts.len() - skip - 1;
            if parts[0].is_empty() {
                hi -= 1;
                if hi != 0 {
                    return false;
                }
            }
            if parts[parts.len() - 1].is_empty() {
                lo -= 1;
                if lo != 0 {
                    return false;
                }
            }
            if 8 < hi + lo + 1 {
                return false;
            }
            (hi, lo)
        }
        None => {
            if parts.len() != 8 || parts[0].is_empty() || parts[parts.len() - 1].is_empty() {
                return false;
            }
            (parts.len(), 0)
        }
    };
    let hextet =
        |s: &String| s.len() <= 4 && !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit());
    parts[..hi].iter().all(hextet) && parts[parts.len() - lo..].iter().all(hextet)
}

/// `IPv4Address(text)`'s integer: four dot-separated decimal octets of at most three ASCII
/// digits, no leading zero, each at most 255.
fn ipv4_quad(text: &str) -> Option<u32> {
    let octets: Vec<&str> = text.split('.').collect();
    if octets.len() != 4 {
        return None;
    }
    let mut value = 0u32;
    for octet in octets {
        if octet.is_empty()
            || octet.len() > 3
            || !octet.bytes().all(|b| b.is_ascii_digit())
            || (octet != "0" && octet.starts_with('0'))
        {
            return None;
        }
        let n: u32 = octet.parse().ok()?;
        if n > 255 {
            return None;
        }
        value = (value << 8) | n;
    }
    Some(value)
}

fn parsed_from(s: Split) -> Parsed {
    let scheme = s.scheme.unwrap_or_default();
    let (path, params) = if USES_PARAMS.contains(&scheme.as_str()) && s.path.contains(';') {
        let (path, params) = split_params(&s.path);
        (path.to_string(), params.to_string())
    } else {
        (s.path, String::new())
    };
    Parsed {
        scheme,
        netloc: s.netloc.unwrap_or_default(),
        path,
        params,
        query: s.query.unwrap_or_default(),
        fragment: s.fragment.unwrap_or_default(),
    }
}

/// `_splitparams`: the `;params` belong to the last path segment.
fn split_params(url: &str) -> (&str, &str) {
    let from = if url.contains('/') {
        url.rfind('/')
    } else {
        None
    };
    let i = match from {
        Some(slash) => url[slash..].find(';').map(|i| i + slash),
        None => url.find(';'),
    };
    match i {
        Some(i) => (&url[..i], &url[i + 1..]),
        None => (url, ""),
    }
}

/// `urljoin(base, url)`. Since CPython 3.14 this works on the five-part split and keeps an empty
/// query or fragment (`urljoin("http://h/p", "?")` is `"http://h/p?"`), and `;params` stay in the
/// path.
pub fn urljoin(base: &str, url: &str) -> String {
    if base.is_empty() {
        return url.to_string();
    }
    if url.is_empty() {
        return base.to_string();
    }
    let b = split(base, None);
    let u = split(url, None);
    let scheme = u.scheme.clone().or_else(|| b.scheme.clone());
    if scheme != b.scheme
        || scheme
            .as_deref()
            .is_some_and(|s| !USES_RELATIVE.contains(&s))
    {
        return url.to_string();
    }
    let scheme_ref = scheme.as_deref();
    let mut netloc = u.netloc.clone();
    if scheme_ref.is_none_or(|s| USES_NETLOC.contains(&s)) {
        if netloc.as_deref().is_some_and(|n| !n.is_empty()) {
            return unsplit(
                scheme_ref,
                netloc.as_deref(),
                &u.path,
                u.query.as_deref(),
                u.fragment.as_deref(),
            );
        }
        netloc = b.netloc.clone();
    }
    if u.path.is_empty() {
        let mut query = u.query;
        let mut fragment = u.fragment;
        if query.is_none() {
            query = b.query;
            if fragment.is_none() {
                fragment = b.fragment;
            }
        }
        return unsplit(
            scheme_ref,
            netloc.as_deref(),
            &b.path,
            query.as_deref(),
            fragment.as_deref(),
        );
    }
    let mut base_parts: Vec<&str> = b.path.split('/').collect();
    if base_parts.last() != Some(&"") {
        base_parts.pop();
    }
    let segments: Vec<&str> = if u.path.starts_with('/') {
        u.path.split('/').collect()
    } else {
        let mut all = base_parts;
        all.extend(u.path.split('/'));
        // `segments[1:-1] = filter(None, segments[1:-1])`: empty middle segments go.
        if all.len() > 2 {
            let last = all.len() - 1;
            let mut kept = vec![all[0]];
            kept.extend(all[1..last].iter().copied().filter(|s| !s.is_empty()));
            kept.push(all[last]);
            all = kept;
        }
        all
    };
    let mut resolved: Vec<&str> = Vec::new();
    for seg in &segments {
        match *seg {
            ".." => {
                resolved.pop();
            }
            "." => {}
            other => resolved.push(other),
        }
    }
    if matches!(segments.last(), Some(&"." | &"..")) {
        resolved.push("");
    }
    let joined = resolved.join("/");
    let path = if joined.is_empty() { "/" } else { &joined };
    unsplit(
        scheme_ref,
        netloc.as_deref(),
        path,
        u.query.as_deref(),
        u.fragment.as_deref(),
    )
}

/// `urljoin(base, url)` with the `ValueError`s the two `urlsplit` calls inside it raise. Nothing
/// is split when either side is empty, as in Python.
pub fn try_urljoin(base: &str, url: &str) -> Result<String, UrlValueError> {
    if !base.is_empty() && !url.is_empty() {
        for part in [base, url] {
            if let Some(netloc) = split(part, None).netloc {
                check_netloc(&netloc)?;
            }
        }
    }
    Ok(urljoin(base, url))
}

/// `quote(s)` with the default `safe="/"`: the UTF-8 bytes of every character other than an ASCII
/// letter, digit, `_.-~/` become `%XX` (upper-case hex).
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b'~' | b'/') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `unquote(s)`: `%XX` runs are decoded as UTF-8 with U+FFFD for what is not; a `%` not followed
/// by two hex digits stays as it is.
pub fn unquote(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let raw = s.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%'
            && let (Some(hi), Some(lo)) = (
                raw.get(i + 1).and_then(|b| (*b as char).to_digit(16)),
                raw.get(i + 2).and_then(|b| (*b as char).to_digit(16)),
            )
        {
            bytes.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            bytes.push(raw[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// `parse_qsl(qs)` with Python's defaults: pairs split on `&`, a pair with no `=` or no value is
/// dropped, `+` is a space, and both halves are unquoted.
pub fn parse_qsl(qs: &str) -> Vec<(String, String)> {
    qs.split('&')
        .filter_map(|pair| {
            let (name, value) = pair.split_once('=')?;
            if value.is_empty() {
                return None;
            }
            Some((
                unquote(&name.replace('+', " ")),
                unquote(&value.replace('+', " ")),
            ))
        })
        .collect()
}

/// The first value of query parameter `key` in `query`, as `parse_qs(query).get(key)[0]`.
pub fn first_query_value(query: &str, key: &str) -> Option<String> {
    parse_qsl(query)
        .into_iter()
        .find_map(|(k, v)| (k == key).then_some(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_reference_resolves_against_the_post_page() {
        let base = "https://rule34.paheal.net/post/view/5674224";
        assert_eq!(urljoin(base, "https://cdn/x.jpg"), "https://cdn/x.jpg");
        assert_eq!(urljoin(base, "//cdn/x.jpg"), "https://cdn/x.jpg");
        assert_eq!(
            urljoin(base, "/_images/x.jpg"),
            "https://rule34.paheal.net/_images/x.jpg"
        );
        assert_eq!(
            urljoin(base, "x.jpg?a=1#f"),
            "https://rule34.paheal.net/post/view/x.jpg?a=1#f"
        );
        assert_eq!(urljoin(base, ""), base);
        assert_eq!(urljoin("", "x"), "x");
    }

    #[test]
    fn dot_segments_and_empty_segments_collapse() {
        let base = "http://a/b/c/d;p?q";
        for (rel, want) in [
            ("g", "http://a/b/c/g"),
            ("./g", "http://a/b/c/g"),
            ("g/", "http://a/b/c/g/"),
            ("/g", "http://a/g"),
            ("?y", "http://a/b/c/d;p?y"),
            ("g?y", "http://a/b/c/g?y"),
            ("#s", "http://a/b/c/d;p?q#s"),
            (";x", "http://a/b/c/;x"),
            ("..", "http://a/b/"),
            ("../..", "http://a/"),
            ("../../../g", "http://a/g"),
            ("g//h", "http://a/b/c/g/h"),
            ("mailto:x@y", "mailto:x@y"),
            ("ftp://h/p", "ftp://h/p"),
        ] {
            assert_eq!(urljoin(base, rel), want, "{rel}");
        }
    }

    #[test]
    fn unquote_decodes_runs_and_leaves_a_stray_percent() {
        assert_eq!(unquote("plain"), "plain");
        assert_eq!(unquote("a%20b%2Fc"), "a b/c");
        assert_eq!(unquote("%E3%81%82"), "\u{3042}");
        assert_eq!(unquote("100%"), "100%");
        assert_eq!(unquote("%zz%4"), "%zz%4");
        assert_eq!(unquote("%C3x"), "\u{fffd}x");
        assert_eq!(unquote("caf\u{e9}%21"), "caf\u{e9}!");
    }

    #[test]
    fn quote_keeps_unreserved_and_slash_and_encodes_the_rest_as_utf8() {
        assert_eq!(quote("a.jpg"), "a.jpg");
        assert_eq!(quote("a b&c.jpg"), "a%20b%26c.jpg");
        assert_eq!(quote("dir/file_name-1~x"), "dir/file_name-1~x");
        assert_eq!(quote("\u{3042}.png"), "%E3%81%82.png");
        assert_eq!(quote("100%+?#"), "100%25%2B%3F%23");
        assert_eq!(quote(""), "");
    }

    #[test]
    fn query_pairs_follow_parse_qsl_defaults() {
        assert_eq!(
            parse_qsl("r=posts/index&q=a+b%21&&x=&y&=z"),
            [
                ("r".to_string(), "posts/index".to_string()),
                ("q".to_string(), "a b!".to_string()),
                (String::new(), "z".to_string()),
            ]
        );
        assert_eq!(first_query_value("q=1&q=2", "q").as_deref(), Some("1"));
        assert_eq!(first_query_value("q=", "q"), None);
        assert_eq!(first_query_value("", "q"), None);
    }

    #[test]
    fn the_query_is_found_the_way_urlsplit_finds_it() {
        let p = urlparse("index.php?r=posts/index&q=a:b#frag?x", "");
        assert_eq!(
            (p.scheme.as_str(), p.query.as_str()),
            ("", "r=posts/index&q=a:b")
        );
        assert_eq!(p.fragment, "frag?x");
        // Everything up to the first colon is a scheme if it can be one.
        let p = urlparse("HTTP://H/p?q=1", "");
        assert_eq!((p.scheme.as_str(), p.netloc.as_str()), ("http", "H"));
        let p = urlparse("  \thttp://h/p;a?q", "");
        assert_eq!((p.path.as_str(), p.params.as_str()), ("/p", "a"));
        // No scheme: the colon is in the path.
        let p = urlparse("a b:c?q=1", "");
        assert_eq!((p.scheme.as_str(), p.query.as_str()), ("", "q=1"));
    }

    #[test]
    fn urlsplit_s_value_errors_are_kept_by_try_urlparse() {
        // Answers read off CPython 3.14.8's `urlparse`.
        for (url, raises) in [
            ("http://[::1]/x", false),
            ("http://[::1]:80/x", false),
            ("http://u@[::1]:80/", false),
            ("http://[::ffff:1.2.3.4]/x", false),
            ("http://[fe80::1%eth0]/x", false),
            ("http://[v1.x]/x", false),
            ("http://[1:2:3:4:5:6:7:8]/", false),
            ("http://[1:2:3:4:5:6:7::]/", false),
            ("http://[::]/", false),
            ("http://[1:2:3:4:5:6:1.2.3.4]/", false),
            ("http://\u{e9}.example/", false),
            ("pixiv.net\u{ff0f}/comments/1", false),
            ("http://[::1/x", true),
            ("http://::1]/x", true),
            ("http://[1.2.3.4]/x", true),
            ("http://[fe80::1%]/x", true),
            ("http://[v1]/x", true),
            ("http://[x]@e621.net/", true),
            ("http://[::1]x/", true),
            ("http://x[::1]/", true),
            ("http://[1:2:3:4:5:6:7:8:9]/", true),
            ("http://[1::2::3]/", true),
            ("http://[:1:2:3:4:5:6:7]/", true),
            ("http://[1:2:3:4:5:6:7:1.2.3.4]/", true),
            ("http://[::01.2.3.4]/", true),
            // A netloc that NFKC turns into one with `/ ? # @ :` in it.
            ("http://a\u{2100}/x", true),
            ("http://\u{ff0f}.example/", true),
            ("http://a\u{ff1a}80/", true),
            ("//host\u{fe56}x/", true),
        ] {
            assert_eq!(try_urlparse(url, "").is_err(), raises, "{url}");
            // `urlparse` itself still reads all of them.
            let _ = urlparse(url, "");
        }
    }

    #[test]
    fn urljoin_raises_only_when_both_sides_are_split() {
        assert!(try_urljoin("https://www.google.com/", "https://x]/y").is_err());
        assert!(try_urljoin("https://[bad/", "rel").is_err());
        assert_eq!(
            try_urljoin("", "https://x]/y").as_deref(),
            Ok("https://x]/y")
        );
        assert_eq!(
            try_urljoin("https://www.google.com/", "").as_deref(),
            Ok("https://www.google.com/")
        );
    }

    #[test]
    fn urlunparse_drops_empty_parts_as_python_does() {
        let swap = |url: &str| {
            let mut parts = urlparse(url, "");
            parts.netloc = "x.y".to_string();
            urlunparse(&parts)
        };
        assert_eq!(
            swap("http://Danbooru.Donmai.US/posts/1;p?q#f"),
            "http://x.y/posts/1;p?q#f"
        );
        assert_eq!(swap("/posts/1?q"), "//x.y/posts/1?q");
        assert_eq!(swap("https://h?"), "https://x.y");
        let bare = Parsed {
            scheme: "https".to_string(),
            ..Parsed::default()
        };
        assert_eq!(urlunparse(&bare), "https://");
    }
}
