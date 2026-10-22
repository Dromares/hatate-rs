//! The parts of Python's `urllib.parse` that the site parsers call: `urljoin`, `parse_qsl` and
//! `unquote`, with the `urlsplit`/`urlunsplit` they stand on (CPython 3.14, which recorded the
//! corpus).
//!
//! The `url` crate follows the WHATWG URL standard and Python follows RFC 3986 with its own
//! quirks: the two disagree on exactly the inputs a parser meets on a broken page (a `src` with
//! no scheme, a query with a bare `&`, a stray `%`). The corpus is Python's output, so this is
//! Python's algorithm, not the standard's.
//!
//! Left out: `ValueError` for a malformed bracketed host or a NFKC-unstable netloc. Python
//! raises there, the hook that called it is then skipped by `fetch_page_info`, and the port
//! reads such a URL as it would any other.

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

/// `urlparse(url, scheme)`: `scheme` is what a URL with no scheme of its own gets.
pub fn urlparse(url: &str, default_scheme: &str) -> Parsed {
    // Leading C0 controls and spaces go, then tab, CR and LF anywhere.
    let url: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let mut rest = url.as_str();
    let mut scheme = default_scheme.trim_matches(|c: char| c <= ' ').to_string();
    if let Some(i) = rest.find(':')
        && i > 0
        && rest.starts_with(|c: char| c.is_ascii_alphabetic())
        && rest[..i]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        scheme = rest[..i].to_ascii_lowercase();
        rest = &rest[i + 1..];
    }
    let mut netloc = "";
    if let Some(after) = rest.strip_prefix("//") {
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        netloc = &after[..end];
        rest = &after[end..];
    }
    let mut fragment = "";
    if let Some((before, frag)) = rest.split_once('#') {
        rest = before;
        fragment = frag;
    }
    let mut query = "";
    if let Some((before, q)) = rest.split_once('?') {
        rest = before;
        query = q;
    }
    let (path, params) = if USES_PARAMS.contains(&scheme.as_str()) && rest.contains(';') {
        split_params(rest)
    } else {
        (rest, "")
    };
    Parsed {
        scheme,
        netloc: netloc.to_string(),
        path: path.to_string(),
        params: params.to_string(),
        query: query.to_string(),
        fragment: fragment.to_string(),
    }
}

/// `_splitparams`: the `;params` belong to the last path segment.
fn split_params(url: &str) -> (&str, &str) {
    let from = url.rfind('/').filter(|_| url.contains('/'));
    let i = match from {
        Some(slash) => url[slash..].find(';').map(|i| i + slash),
        None => url.find(';'),
    };
    match i {
        Some(i) => (&url[..i], &url[i + 1..]),
        None => (url, ""),
    }
}

/// `urlunparse`.
pub fn urlunparse(p: &Parsed) -> String {
    let mut url = if p.params.is_empty() {
        p.path.clone()
    } else {
        format!("{};{}", p.path, p.params)
    };
    if !p.netloc.is_empty()
        || (!p.scheme.is_empty()
            && USES_NETLOC.contains(&p.scheme.as_str())
            && !url.starts_with("//"))
    {
        if !url.is_empty() && !url.starts_with('/') {
            url.insert(0, '/');
        }
        url = format!("//{}{url}", p.netloc);
    }
    if !p.scheme.is_empty() {
        url = format!("{}:{url}", p.scheme);
    }
    if !p.query.is_empty() {
        url = format!("{url}?{}", p.query);
    }
    if !p.fragment.is_empty() {
        url = format!("{url}#{}", p.fragment);
    }
    url
}

/// `urljoin(base, url)`.
pub fn urljoin(base: &str, url: &str) -> String {
    if base.is_empty() {
        return url.to_string();
    }
    if url.is_empty() {
        return base.to_string();
    }
    let b = urlparse(base, "");
    let mut u = urlparse(url, &b.scheme);
    if u.scheme != b.scheme || !USES_RELATIVE.contains(&u.scheme.as_str()) {
        return url.to_string();
    }
    if USES_NETLOC.contains(&u.scheme.as_str()) {
        if !u.netloc.is_empty() {
            return urlunparse(&u);
        }
        u.netloc = b.netloc.clone();
    }
    if u.path.is_empty() && u.params.is_empty() {
        u.path = b.path;
        u.params = b.params;
        if u.query.is_empty() {
            u.query = b.query;
        }
        return urlunparse(&u);
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
    u.path = if joined.is_empty() {
        "/".to_string()
    } else {
        joined
    };
    urlunparse(&u)
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
}
