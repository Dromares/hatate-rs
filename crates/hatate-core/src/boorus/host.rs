//! Which site a URL is on, by its hostname (`core/boorus/_host.py`).
//!
//! Every parser used to ask whether its host appeared ANYWHERE in the URL, so "x.com" claimed
//! "hentaivox.com" and "rule34.xxx" claimed the path of r34.app's mirror pages. This asks about
//! the hostname only.
//!
//! The hostname is read the way `urllib.parse.urlparse(url).hostname` reads it (CPython 3.14,
//! which recorded the corpus) and not the way the `url` crate does, so that the two agree on the
//! odd inputs: a missing scheme, a `user@` prefix, backslashes (not a separator here).
//!
//! Two inputs make Python raise `ValueError` out of `matches`: a malformed bracketed (IPv6)
//! host, and a non-ASCII host whose NFKC form contains one of `/?#@:`. Here the first is "not on
//! the host" and the second is not detected at all (it would need a Unicode-normalisation
//! dependency for a hostile-input case); neither occurs in the corpus.

use crate::pycompat;

/// `urllib.parse` strips these from the front of a URL, and removes tab, CR and LF anywhere.
fn is_c0_or_space(c: char) -> bool {
    c <= ' '
}

/// True when `s` is a URL scheme as `urlsplit` reads one: ASCII letter first, then letters,
/// digits and `+-.`.
fn is_scheme(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// `urlparse(url).hostname`, lower-cased; `None` where Python gives `None`.
fn hostname(url: &str) -> Option<String> {
    let cleaned: String = url
        .trim_start_matches(is_c0_or_space)
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let mut rest = cleaned.as_str();
    if let Some(i) = rest.find(':')
        && i > 0
        && is_scheme(&rest[..i])
    {
        rest = &rest[i + 1..];
    }
    let after_slashes = rest.strip_prefix("//")?;
    let end = after_slashes
        .find(['/', '?', '#'])
        .unwrap_or(after_slashes.len());
    let netloc = &after_slashes[..end];
    let hostinfo = netloc.rsplit_once('@').map_or(netloc, |(_, host)| host);
    let host = match hostinfo.split_once('[') {
        Some((before, bracketed)) => {
            if !before.is_empty() || !bracketed.contains(']') {
                return None;
            }
            bracketed.split_once(']').map_or("", |(host, _)| host)
        }
        None => {
            if hostinfo.contains(']') {
                return None;
            }
            hostinfo.split_once(':').map_or(hostinfo, |(host, _)| host)
        }
    };
    if host.is_empty() {
        return None;
    }
    // `hostname` lower-cases what comes before an IPv6 zone id (`%`) and leaves the rest.
    Some(match host.split_once('%') {
        Some((name, zone)) => format!("{}%{zone}", name.to_lowercase()),
        None => host.to_lowercase(),
    })
}

/// True if `url` is on one of `hosts` or a subdomain of one.
pub fn on_host(url: &str, hosts: &[&str]) -> bool {
    let url = pycompat::strip(url);
    if url.is_empty() {
        return false;
    }
    let url = if url.contains("://") {
        url.to_string()
    } else {
        format!("http://{}", url.trim_start_matches('/'))
    };
    let host = hostname(&url).unwrap_or_default();
    hosts
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const X: &[&str] = &["x.com", "twitter.com"];

    #[test]
    fn claims_the_host_and_its_subdomains_only() {
        assert!(on_host("https://x.com/a/status/1", X));
        assert!(on_host("https://mobile.twitter.com/a", X));
        assert!(on_host("HTTPS://X.COM/a", X));
        // The bug this module exists for: the host name is inside another host's name.
        assert!(!on_host("https://hentaivox.com/x.com", X));
        assert!(!on_host("https://freeadultcomix.com/", X));
        assert!(!on_host("https://r34.app/?u=https://x.com/a", X));
    }

    #[test]
    fn a_missing_scheme_is_supplied() {
        assert!(on_host("x.com/a", X));
        assert!(on_host("//x.com/a", X));
        assert!(on_host("  x.com:8080/a  ", X));
        assert!(!on_host("", X));
        assert!(!on_host("   ", X));
    }

    #[test]
    fn userinfo_ports_and_separators_read_like_urlparse() {
        assert!(on_host("https://user:pw@x.com:443/a", X));
        // The LAST `@` ends the userinfo, so this is evil.com.
        assert!(!on_host("https://x.com@evil.com/a", X));
        assert!(on_host("https://evil.com@x.com/a", X));
        // A backslash is not a separator to urlparse, so the host runs on.
        assert!(!on_host("https://x.com\\@evil.com/", X));
        // A tab or newline inside the URL is dropped before parsing.
        assert!(on_host("https://x.c\tom/a", X));
        assert!(!on_host("foo?u=http://x.com/a", X));
        assert!(!on_host("https://[::1]/x.com", X));
        assert!(!on_host("https://[x.com/a", X));
    }
}
