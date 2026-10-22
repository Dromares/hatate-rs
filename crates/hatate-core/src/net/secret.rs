//! Secret-carrying types. Their `Debug` never prints a value (docs/net.md section 9).

use std::fmt;

use url::Url;

/// A value that must not reach a log, a `Debug` dump or an error message.
///
/// No `Display`, no `Serialize`: the only way out is [`Secret::expose`], which
/// the call site has to spell.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Secret(value.into())
    }

    /// The secret itself. Use it only to put it on the wire.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Secret(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Secret(value.to_owned())
    }
}

/// Cookies a caller passes for one request, in insertion order.
///
/// `Debug` prints the cookie **names** only, as the Python log does
/// (`core/site_access.py:364-365`). The optional scope lists the domains the
/// cookies may follow a redirect to; without one they stay on the request's own
/// host (docs/net.md section 8).
#[derive(Clone, Default)]
pub struct Cookies {
    scope: Vec<String>,
    items: Vec<(String, Secret)>,
}

impl Cookies {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lets these cookies follow a redirect to `domain` or any subdomain of it.
    pub fn with_scope<I, S>(mut self, domains: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.scope = domains.into_iter().map(Into::into).collect();
        self
    }

    /// Sets a cookie. A later value for the same name replaces the earlier one
    /// and keeps its position, like a Python dict.
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<Secret>) {
        let name = name.into();
        let value = value.into();
        match self.items.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = value,
            None => self.items.push((name, value)),
        }
    }

    pub fn get(&self, name: &str) -> Option<&Secret> {
        self.items.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(|(n, _)| n.as_str())
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn scope(&self) -> &[String] {
        &self.scope
    }

    /// `name=value; name=value`, as a `Cookie` header value.
    pub fn header_value(&self) -> Option<Secret> {
        if self.items.is_empty() {
            return None;
        }
        let joined = self
            .items
            .iter()
            .map(|(n, v)| format!("{n}={}", v.expose()))
            .collect::<Vec<_>>()
            .join("; ");
        Some(Secret(joined))
    }

    /// Whether these cookies may be sent to `host`, given the host of the
    /// request they were first attached to.
    pub(crate) fn allows(&self, host: &str, origin_host: &str) -> bool {
        if self.scope.is_empty() {
            host.eq_ignore_ascii_case(origin_host)
        } else {
            self.scope
                .iter()
                .any(|domain| crate::net::site::host_matches(host, domain))
        }
    }
}

impl fmt::Debug for Cookies {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cookies")
            .field("names", &self.names().collect::<Vec<_>>())
            .field("scope", &self.scope)
            .finish()
    }
}

/// `scheme://host[:port]/path`, with the userinfo, query and fragment dropped.
///
/// This is the only form of a URL that goes into an error or a log line: the
/// SauceNAO key travels in the query string (`core/saucenao.py:376-380`).
pub fn redact_url(url: &Url) -> String {
    let mut out = format!("{}://", url.scheme());
    if let Some(host) = url.host_str() {
        out.push_str(host);
    }
    if let Some(port) = url.port() {
        out.push(':');
        out.push_str(&port.to_string());
    }
    out.push_str(url.path());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_is_redacted() {
        let s = Secret::new("synthetic-secret-value");
        assert_eq!(format!("{s:?}"), "[redacted]");
        assert_eq!(format!("{s:#?}"), "[redacted]");
        assert_eq!(s.expose(), "synthetic-secret-value");
    }

    #[test]
    fn cookies_debug_shows_names_only() {
        let mut c = Cookies::new();
        c.set("sid", "synthetic-cookie-value");
        c.set("other", "second-synthetic-value");
        let dump = format!("{c:?}");
        assert!(dump.contains("sid") && dump.contains("other"));
        assert!(!dump.contains("synthetic"));
    }

    #[test]
    fn cookie_set_replaces_in_place() {
        let mut c = Cookies::new();
        c.set("a", "1");
        c.set("b", "2");
        c.set("a", "3");
        assert_eq!(c.header_value().unwrap().expose(), "a=3; b=2");
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn redact_url_drops_query_userinfo_fragment() {
        let url = Url::parse(
            "https://user:pw@saucenao.com:8443/search.php?api_key=SECRETKEY&db=999#frag",
        )
        .unwrap();
        let out = redact_url(&url);
        assert_eq!(out, "https://saucenao.com:8443/search.php");
    }

    #[test]
    fn cookie_scope_defaults_to_origin_host() {
        let c = Cookies::new();
        assert!(c.allows("a.example", "a.example"));
        assert!(!c.allows("b.example", "a.example"));
        let scoped = Cookies::new().with_scope(["deviantart.com", "fav.me"]);
        assert!(scoped.allows("www.deviantart.com", "fav.me"));
        assert!(scoped.allows("fav.me", "fav.me"));
        assert!(!scoped.allows("deviantart.com.evil.example", "fav.me"));
    }
}
