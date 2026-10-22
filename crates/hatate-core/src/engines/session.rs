//! A `requests.Session` for the one engine that uses one: ascii2d (`core/ascii2d.py`).
//!
//! Every other engine goes through `core.net`, whose per-host sessions refuse cookies
//! (`DefaultCookiePolicy(allowed_domains=[])`). ascii2d builds its own `requests.Session()`, so
//! the jar is live: a cookie set by the upload's answer, or by any of its redirect hops, is sent
//! with the feature search that follows. The net layer follows redirects inside one call and
//! keeps no cookies, so this follows them here (one hop per `send`, `Redirects::Stop`) and keeps
//! the jar.
//!
//! The jar is `http.cookiejar` with its default policy (Netscape cookies, RFC 2965 off), for the
//! attributes ascii2d's host can send: `Path`, `Domain`, `Secure`, `Max-Age` and `Expires`. A
//! cookie with `Version`, a `Domain` that is a bare top-level name, a date `http2time` reads and
//! this does not, or a value `http.cookiejar` would quote, is outside it (`docs/net.md` 13.3).

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use http::HeaderMap;
use http::header::SET_COOKIE;
use url::Url;

use super::client::{Failure, Upload, failure_of, seconds};
use crate::hard_timeout::HardTimeoutError;
use crate::net::{Method, NetClient, NetRequest, NetResponse, Redirects, RetryMode};
use crate::pyurl;

/// `requests.Session.max_redirects`.
const MAX_REDIRECTS: usize = 30;

#[derive(Debug, Clone)]
struct Cookie {
    name: String,
    /// `None` for a cookie sent as a bare name (`Set-Cookie: flag`).
    value: Option<String>,
    /// As `http.cookiejar` stores it: a host for a cookie without `Domain`, a dotted name for one
    /// with it.
    domain: String,
    path: String,
    secure: bool,
    /// Seconds since the epoch; `None` is a session cookie.
    expires: Option<i64>,
}

/// `http.cookiejar.CookieJar`'s nested `domain -> path -> name` dictionary, iteration order
/// included: a replaced cookie keeps its place, a cleared one leaves its (empty) containers.
#[derive(Debug, Clone, Default)]
pub(crate) struct CookieJar {
    domains: Vec<(String, Paths)>,
}

/// The cookies of one domain, by path, each path holding its cookies by name.
type Paths = Vec<(String, Vec<Cookie>)>;

/// Where `request_host` / `eff_request_host` read from: the URL being asked.
struct Target {
    /// Lower-cased host, no port.
    host: String,
    /// `eff_request_host`: the host, with `.local` added when it has no dot and is not an IPv4.
    effective: String,
    /// `request_path`, `escape_path` applied.
    path: String,
    https: bool,
}

impl Target {
    fn of(url: &Url) -> Target {
        let host = url.host_str().unwrap_or("").to_ascii_lowercase();
        let effective = if !host.contains('.') && !is_ipv4(&host) {
            format!("{host}.local")
        } else {
            host.clone()
        };
        let mut path = escape_path(url.path());
        if !path.starts_with('/') {
            path.insert(0, '/');
        }
        Target {
            host,
            effective,
            path,
            https: matches!(url.scheme(), "https" | "wss"),
        }
    }
}

/// `^\d+\.\d+\.\d+\.\d+$`
fn is_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// `http.cookiejar.escape_path`: `quote(path, "%/;:@&=+$,!~*'()")`, then the hex of every escape
/// in upper case.
fn escape_path(path: &str) -> String {
    const SAFE: &[u8] = b"%/;:@&=+$,!~*'()_.-";
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || SAFE.contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    let bytes = out.clone().into_bytes();
    let mut fixed = String::with_capacity(out.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            fixed.push('%');
            fixed.push(char::from(bytes[i + 1]).to_ascii_uppercase());
            fixed.push(char::from(bytes[i + 2]).to_ascii_uppercase());
            i += 3;
        } else {
            fixed.push(char::from(bytes[i]));
            i += 1;
        }
    }
    fixed
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl CookieJar {
    /// `extract_cookies`: stores the cookies the `Set-Cookie` lines of an answer to `url` set.
    fn extract(&mut self, url: &Url, headers: &HeaderMap) {
        let target = Target::of(url);
        let now = now_epoch();
        let mut sets = Vec::new();
        // `_normalized_cookie_tuples` reads every line first: one that raises drops the lot, and
        // nothing is deleted or stored.
        let mut parsed = Vec::new();
        for line in headers.get_all(SET_COOKIE) {
            let Ok(line) = line.to_str() else { continue };
            match parse_set_cookie(line) {
                Parse::Cookie(cookie) => parsed.push(cookie),
                Parse::Bad => {}
                Parse::DropResponse => return,
            }
        }
        // `make_cookies` builds every cookie of the answer before `extract_cookies` stores any, and
        // a cookie whose expiry is already past deletes as it is built: so a deletion in an answer
        // applies to what the jar held before that answer, never to a cookie the same answer sets.
        for cookie in parsed {
            match change_from_header(cookie, &target, now) {
                Some(Change::Set(cookie)) => sets.push(cookie),
                Some(Change::Delete(domain, path, name)) => self.clear(&domain, &path, &name),
                None => {}
            }
        }
        for cookie in sets {
            self.store(cookie);
        }
    }

    fn store(&mut self, cookie: Cookie) {
        let domain = match self.domains.iter().position(|(d, _)| *d == cookie.domain) {
            Some(i) => i,
            None => {
                self.domains.push((cookie.domain.clone(), Vec::new()));
                self.domains.len() - 1
            }
        };
        let paths = &mut self.domains[domain].1;
        let path = match paths.iter().position(|(p, _)| *p == cookie.path) {
            Some(i) => i,
            None => {
                paths.push((cookie.path.clone(), Vec::new()));
                paths.len() - 1
            }
        };
        let names = &mut paths[path].1;
        match names.iter().position(|c| c.name == cookie.name) {
            Some(i) => names[i] = cookie,
            None => names.push(cookie),
        }
    }

    /// `merge_cookies(self, other)`: every cookie of `other`, in its order, stored here (a cookie
    /// already here is replaced where it stands).
    fn merge_from(&mut self, other: &CookieJar) {
        for (_, paths) in &other.domains {
            for (_, names) in paths {
                for cookie in names {
                    self.store(cookie.clone());
                }
            }
        }
    }

    /// `jar.set(name, value, domain=domain)`: a session cookie for `domain` (a dotted name, as
    /// `requests` stores it), path `/`, not `Secure`.
    fn set(&mut self, name: &str, value: &str, domain: &str) {
        self.store(Cookie {
            name: name.to_owned(),
            value: Some(value.to_owned()),
            domain: domain.to_owned(),
            path: "/".to_owned(),
            secure: false,
            expires: None,
        });
    }

    fn clear(&mut self, domain: &str, path: &str, name: &str) {
        for (d, paths) in &mut self.domains {
            if d != domain {
                continue;
            }
            for (p, names) in paths {
                if p == path {
                    names.retain(|c| c.name != name);
                }
            }
        }
    }

    /// `add_cookie_header`: the `Cookie` header value for a request to `url`, if any.
    fn header_for(&self, url: &Url) -> Option<String> {
        let target = Target::of(url);
        let now = now_epoch();
        let mut chosen: Vec<&Cookie> = Vec::new();
        for (domain, paths) in &self.domains {
            if !domain_return_ok(domain, &target) {
                continue;
            }
            for (path, names) in paths {
                if !path_return_ok(path, &target.path) {
                    continue;
                }
                for cookie in names {
                    if cookie.secure && !target.https {
                        continue;
                    }
                    if cookie.expires.is_some_and(|e| e <= now) {
                        continue;
                    }
                    chosen.push(cookie);
                }
            }
        }
        if chosen.is_empty() {
            return None;
        }
        // `sort(key=len(path), reverse=True)` is stable: the most specific path first.
        chosen.sort_by_key(|c| std::cmp::Reverse(c.path.len()));
        let attrs: Vec<String> = chosen
            .iter()
            .map(|c| match &c.value {
                Some(value) => format!("{}={}", c.name, value),
                None => c.name.clone(),
            })
            .collect();
        Some(attrs.join("; "))
    }
}

/// `DefaultCookiePolicy.path_return_ok`: the request path is the cookie's path, or sits below it
/// (the cookie's path ends in `/`, or the next character of the request path is one).
fn path_return_ok(cookie_path: &str, request_path: &str) -> bool {
    if request_path == cookie_path {
        return true;
    }
    match request_path.strip_prefix(cookie_path) {
        Some(rest) => cookie_path.ends_with('/') || rest.starts_with('/'),
        None => false,
    }
}

/// `DefaultCookiePolicy.return_ok_domain` for a version 0 cookie: the request host, or its
/// effective host, ends with the cookie's domain once both are dotted.
fn domain_return_ok(domain: &str, target: &Target) -> bool {
    format!(".{}", target.host).ends_with(domain)
        || format!(".{}", target.effective).ends_with(domain)
}

/// The attribute list of a `Set-Cookie` line, `parse_ns_headers` + `_normalized_cookie_tuples`.
struct Parsed {
    name: String,
    value: Option<String>,
    domain: Option<String>,
    path: Option<String>,
    secure: bool,
    max_age: Option<i64>,
    /// `Version`: 0 when the line has none.
    version: i64,
    /// `Expires`, when it parsed. One that did not is ignored, a session cookie.
    expires: Option<i64>,
}

fn parse_set_cookie(line: &str) -> Parse {
    let mut parsed = Parsed {
        name: String::new(),
        value: None,
        domain: None,
        path: None,
        secure: false,
        max_age: None,
        version: 0,
        expires: None,
    };
    let mut seen: Vec<&'static str> = Vec::new();
    let mut max_age_set = false;
    let mut unreadable_version = false;
    let params: Vec<&str> = split_params(line);
    for (i, raw) in params.iter().enumerate() {
        let param = raw.trim_end();
        if param.is_empty() {
            continue;
        }
        let (key, value) = match param.split_once('=') {
            Some((k, v)) => (k.trim_start().trim_end(), Some(v.trim_start())),
            None => (param, None),
        };
        if i == 0 {
            parsed.name = key.to_owned();
            parsed.value = value.map(str::to_owned);
            continue;
        }
        let lower = key.to_lowercase();
        let known = match lower.as_str() {
            "expires" => "expires",
            "domain" => "domain",
            "path" => "path",
            "secure" => "secure",
            "max-age" => "max-age",
            "version" => "version",
            _ => continue,
        };
        // Only the first value of an attribute is significant, except that `Max-Age` becomes an
        // `expires` after this test, so a second `Max-Age` replaces the first.
        if known != "max-age" && seen.contains(&known) {
            continue;
        }
        if known == "max-age" && value.is_none() {
            // `int(None)` raises something `http.cookiejar` does not expect, and it drops every
            // cookie of the answer, not only this one.
            return Parse::DropResponse;
        }
        if known == "expires" && value.is_none() {
            continue; // no date: a session cookie
        }
        if known != "secure" && value.is_none() {
            return Parse::Bad; // a value attribute with no value: the whole cookie is bad
        }
        match known {
            "secure" => {
                seen.push(known);
                parsed.secure = true;
            }
            "domain" => {
                seen.push(known);
                parsed.domain = value.map(str::to_lowercase);
            }
            "path" => {
                seen.push(known);
                parsed.path = value.map(str::to_owned);
            }
            "version" => {
                seen.push(known);
                // An unreadable version drops the cookie.
                // Read after the other attributes: a number it is not drops this cookie, but a
                // `Max-Age` further along can still drop the whole answer first.
                match parse_int(strip_quotes(value.unwrap_or(""))) {
                    Some(version) => parsed.version = version,
                    None => unreadable_version = true,
                }
            }
            "max-age" => {
                max_age_set = true;
                match parse_int(value.unwrap_or("")) {
                    Some(age) => parsed.max_age = Some(age),
                    None => return Parse::Bad,
                }
            }
            _ => {
                // A date `http2time` cannot read leaves the cookie a session cookie.
                if max_age_set {
                    continue;
                }
                if let Some(at) = http2time(strip_quotes(value.unwrap_or(""))) {
                    seen.push(known);
                    parsed.expires = Some(at);
                }
            }
        }
    }
    if unreadable_version || (parsed.name.is_empty() && parsed.value.is_none()) {
        return Parse::Bad;
    }
    Parse::Cookie(parsed)
}

/// What one `Set-Cookie` line parses to.
enum Parse {
    Cookie(Parsed),
    /// This cookie is dropped (a value attribute with no value, a `Max-Age` or a `Version` that is
    /// not a number).
    Bad,
    /// Every cookie of the answer is dropped.
    DropResponse,
}

/// `re.split(r";\s*", header)`
fn split_params(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(i) = rest.find(';') {
        out.push(&rest[..i]);
        rest = rest[i + 1..].trim_start();
    }
    out.push(rest);
    out
}

fn strip_quotes(text: &str) -> &str {
    let text = text.strip_prefix('"').unwrap_or(text);
    text.strip_suffix('"').unwrap_or(text)
}

/// `int(text)` for the sort of text `Max-Age` carries.
fn parse_int(text: &str) -> Option<i64> {
    let text = text.trim();
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<i64>().ok()
}

/// `http2time` for the two forms servers send: `Wdy, DD Mon YYYY HH:MM:SS GMT` and the
/// `Wdy, DD-Mon-YYYY HH:MM:SS GMT` form Netscape used. `None` for anything else, which
/// `http.cookiejar` may still read (`docs/net.md` 13.3).
fn http2time(text: &str) -> Option<i64> {
    let (_, rest) = text.trim().split_once(',')?;
    let mut tokens: Vec<&str> = rest.split_whitespace().collect();
    if tokens.len() == 3 {
        // `DD-Mon-YYYY HH:MM:SS GMT`
        let date = tokens.remove(0);
        let mut parts = date.split('-');
        let (d, m, y) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        tokens.splice(0..0, [d, m, y]);
    }
    let [day, month, year, time, zone] = tokens[..] else {
        return None;
    };
    if !zone.eq_ignore_ascii_case("GMT") {
        return None;
    }
    let months = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let month =
        i64::try_from(months.iter().position(|m| m.eq_ignore_ascii_case(month))? + 1).ok()?;
    let day: i64 = day.parse().ok()?;
    let mut year: i64 = year.parse().ok()?;
    if year < 100 {
        year += if year > 68 { 1900 } else { 2000 };
    }
    let mut hms = time.split(':');
    let h: i64 = hms.next()?.parse().ok()?;
    let m: i64 = hms.next()?.parse().ok()?;
    let s: i64 = hms.next()?.parse().ok()?;
    if hms.next().is_some() || !(1..=31).contains(&day) || h > 23 || m > 59 || s > 59 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + h * 3600 + m * 60 + s)
}

/// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// What one `Set-Cookie` line asks for.
enum Change {
    Set(Cookie),
    /// An expiry in the past: delete `(domain, path, name)`.
    Delete(String, String, String),
}

/// `_cookie_from_cookie_tuple` + `DefaultCookiePolicy.set_ok`. `None` when the line is bad or the
/// policy refuses the cookie.
fn change_from_header(parsed: Parsed, target: &Target, now: i64) -> Option<Change> {
    let domain = match &parsed.domain {
        None => target.effective.clone(),
        Some(d) if d.starts_with('.') => d.clone(),
        Some(d) => format!(".{d}"),
    };
    let path = match parsed.path.as_deref() {
        Some(p) if !p.is_empty() => escape_path(p),
        _ => {
            // Netscape's default path drops the last segment and its slash; a cookie that names
            // a version keeps the slash.
            let mut path = target.path.clone();
            if let Some(i) = path.rfind('/') {
                path.truncate(if parsed.version == 0 { i } else { i + 1 });
            }
            if path.is_empty() {
                path = "/".to_owned();
            }
            path
        }
    };
    let expires = match (parsed.max_age, parsed.expires) {
        (Some(age), _) => Some(now.saturating_add(age)),
        (None, Some(at)) => Some(at),
        (None, None) => None,
    };
    if expires.is_some_and(|e| e <= now) {
        return Some(Change::Delete(domain, path, parsed.name));
    }
    // Version 1 is read as a Netscape cookie (`rfc2109_as_netscape`); RFC 2965 is off, so
    // anything above it is refused.
    if parsed.version > 1 {
        return None;
    }
    if parsed.domain.is_some() && !set_ok_domain(&domain, target) {
        return None;
    }
    Some(Change::Set(Cookie {
        name: parsed.name,
        value: parsed.value,
        domain,
        path,
        secure: parsed.secure,
        expires,
    }))
}

/// `DefaultCookiePolicy.set_ok_domain` for a version 0 cookie that named a `Domain`: it needs an
/// embedded dot (or be `.local`) and must domain-match the request host.
fn set_ok_domain(domain: &str, target: &Target) -> bool {
    let undotted = domain.strip_prefix('.').unwrap_or(domain);
    if !undotted.contains('.') && domain != ".local" {
        return false;
    }
    format!(".{}", target.effective).ends_with(domain)
}

/// What a session sends: a method, a URL, and for a POST the upload.
pub(crate) struct SessionRequest<'a> {
    pub(crate) method: Method,
    pub(crate) url: &'a str,
    pub(crate) upload: Option<&'a Upload>,
}

/// `requests.Session()` with `headers.update(...)`.
pub(crate) struct Session {
    net: NetClient,
    headers: Vec<(&'static str, String)>,
    jar: CookieJar,
}

impl Session {
    pub(crate) fn new(net: NetClient, headers: Vec<(&'static str, String)>) -> Self {
        Session {
            net,
            headers,
            jar: CookieJar::default(),
        }
    }

    /// `session.cookies.set(name, value, domain=domain)`: a cookie the session starts with.
    pub(crate) fn with_cookie(mut self, name: &str, value: &str, domain: &str) -> Self {
        self.jar.set(name, value, domain);
        self
    }

    /// `session.post/get(url, timeout=timeout, allow_redirects=True)` under a hard deadline of
    /// `timeout` seconds from `started` over the whole chain of hops, as `run_with_hard_timeout`
    /// puts one around the call.
    pub(crate) async fn send(
        &mut self,
        request: SessionRequest<'_>,
        timeout: f64,
        started: Instant,
    ) -> Result<NetResponse, Failure> {
        let total = seconds(timeout);
        let mut method = request.method;
        let mut url = request.url.to_owned();
        let mut body: Option<(&str, Bytes)> = request
            .upload
            .map(|u| (u.content_type.as_str(), u.body.clone()));
        let mut redirects = 0usize;
        // The jar `prepare_request` copied from the session's (`merge_cookies`): the Cookie
        // header of each request is read from it, not from the session's own. A redirect hop
        // copies the previous one, takes the answer's cookies, and merges the session's in.
        let mut hop_jar = self.jar.clone();
        let mut previous: Option<Url> = None;
        let mut previous_headers = http::HeaderMap::new();
        loop {
            let left = total.saturating_sub(started.elapsed());
            if left.is_zero() {
                return Err(Failure::Deadline(HardTimeoutError::trickling(total)));
            }
            let parsed =
                Url::parse(&url).map_err(|e| Failure::Transport(format!("bad URL: {e}")))?;
            let mut net_request = NetRequest::new(method, &url, seconds(timeout))
                .map_err(|e| failure_of(&e, total))?
                .with_redirects(Redirects::Stop)
                .with_retry(RetryMode::Never)
                .without_host_gate()
                .with_deadline(left);
            for (name, value) in &self.headers {
                net_request = net_request
                    .header(name, value)
                    .map_err(|e| failure_of(&e, total))?;
            }
            if let Some(prior) = &previous {
                let mut next = hop_jar.clone();
                next.extract(prior, &previous_headers);
                next.merge_from(&self.jar);
                hop_jar = next;
            }
            if let Some(cookie) = hop_jar.header_for(&parsed) {
                net_request = net_request
                    .header("Cookie", &cookie)
                    .map_err(|e| failure_of(&e, total))?;
            }
            if let Some((content_type, bytes)) = &body {
                net_request = net_request
                    .header("Content-Type", content_type)
                    .map_err(|e| failure_of(&e, total))?
                    .with_body(bytes.clone());
            }
            let response = match self.net.send(net_request).await {
                Ok(response) => response,
                Err(err) => return Err(failure_of(&err, total)),
            };
            // `Session.send` stores an answer's cookies; so does `resolve_redirects`, again, for
            // each answer a hop got. Storing twice matters: a cookie the answer both deletes and
            // sets is put back at the end of its list the second time.
            self.jar.extract(&parsed, &response.headers);
            if redirects > 0 {
                self.jar.extract(&parsed, &response.headers);
            }
            previous = Some(parsed);
            previous_headers = response.headers.clone();

            let status = response.status.as_u16();
            let location = response
                .header("location")
                .filter(|_| matches!(status, 301 | 302 | 303 | 307 | 308));
            let Some(location) = location else {
                return Ok(response);
            };
            redirects += 1;
            if redirects > MAX_REDIRECTS {
                return Err(Failure::Transport(format!(
                    "Exceeded {MAX_REDIRECTS} redirects."
                )));
            }
            url = pyurl::urljoin(&url, location);
            // `Session.rebuild_method`: 303 and 302 turn anything but a HEAD into a GET, and a
            // 301 turns a POST into one; a GET has no body and no content type.
            let to_get = match status {
                303 => method != Method::Head,
                302 => method != Method::Head,
                301 => method == Method::Post,
                _ => false,
            };
            if to_get {
                method = Method::Get;
                body = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(lines: &[&str]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for line in lines {
            map.append(SET_COOKIE, line.parse().unwrap());
        }
        map
    }

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    fn header(jar: &CookieJar, at: &str) -> Option<String> {
        jar.header_for(&url(at))
    }

    #[test]
    fn a_cookie_goes_back_to_its_path_and_below_it_with_the_longest_path_first() {
        let mut jar = CookieJar::default();
        jar.extract(
            &url("https://a.example/search/file"),
            &headers(&["a=1", "b=2; Path=/", "c=3; Path=/search/color"]),
        );
        // `a` took the default path `/search`.
        assert_eq!(
            header(&jar, "https://a.example/search/x"),
            Some("a=1; b=2".into())
        );
        assert_eq!(
            header(&jar, "https://a.example/search/color/aa"),
            Some("c=3; a=1; b=2".into())
        );
        // `/search` is not a prefix of `/searching`.
        assert_eq!(
            header(&jar, "https://a.example/searching"),
            Some("b=2".into())
        );
        assert_eq!(
            header(&jar, "https://a.example/search"),
            Some("a=1; b=2".into())
        );
    }

    #[test]
    fn deletions_apply_before_the_answers_own_cookies_are_stored() {
        let mut jar = CookieJar::default();
        jar.extract(&url("https://a.example/x"), &headers(&["k=old; Path=/"]));
        // Deleting and setting in one answer: the delete reaches the old cookie, the set stays.
        jar.extract(
            &url("https://a.example/x"),
            &headers(&["k=new; Path=/", "k=gone; Path=/; Max-Age=0"]),
        );
        assert_eq!(header(&jar, "https://a.example/y"), Some("k=new".into()));
        // A date in the past deletes too.
        jar.extract(
            &url("https://a.example/x"),
            &headers(&["k=x; Path=/; Expires=Thu, 01 Jan 1970 00:00:01 GMT"]),
        );
        assert_eq!(header(&jar, "https://a.example/y"), None);
    }

    #[test]
    fn a_bad_attribute_drops_one_cookie_and_a_bare_max_age_drops_the_answer() {
        let mut jar = CookieJar::default();
        jar.extract(
            &url("https://a.example/x"),
            &headers(&[
                "a=1; Domain",
                "b=2; Max-Age=abc",
                "c=3; Version=x",
                "d=4; Expires",
                "e=5",
            ]),
        );
        assert_eq!(header(&jar, "https://a.example/y"), Some("d=4; e=5".into()));
        let mut jar = CookieJar::default();
        jar.extract(
            &url("https://a.example/x"),
            &headers(&["a=1", "b=2; Max-Age"]),
        );
        assert_eq!(header(&jar, "https://a.example/y"), None);
    }

    #[test]
    fn domain_secure_and_expiry() {
        let mut jar = CookieJar::default();
        jar.extract(
            &url("https://www.a.example/x"),
            &headers(&[
                "dom=1; Domain=a.example",
                "other=2; Domain=b.example",
                "tld=3; Domain=example",
                "sec=4; Secure",
                "late=5; Max-Age=3600",
                "date=6; Expires=Wed, 01 Jan 2099 00:00:00 GMT",
            ]),
        );
        assert_eq!(
            header(&jar, "https://www.a.example/y"),
            Some("dom=1; sec=4; late=5; date=6".into())
        );
        // A domain cookie goes to the other subdomains; the host-only ones do not.
        assert_eq!(header(&jar, "https://b.a.example/y"), Some("dom=1".into()));
        assert_eq!(header(&jar, "https://c.example/y"), None);
        // `Secure` stays off plain http.
        assert_eq!(
            header(&jar, "http://www.a.example/y"),
            Some("dom=1; late=5; date=6".into())
        );
    }

    #[test]
    fn a_version_keeps_the_slash_of_the_default_path_and_above_one_is_refused() {
        let mut jar = CookieJar::default();
        jar.extract(
            &url("https://a.example/dir/page"),
            &headers(&["v0=1", "v1=2; Version=1", "v2=3; Version=2"]),
        );
        assert_eq!(
            header(&jar, "https://a.example/dir/z"),
            Some("v1=2; v0=1".into())
        );
        assert_eq!(header(&jar, "https://a.example/dir"), Some("v0=1".into()));
    }

    #[test]
    fn dates_are_read_in_the_two_forms_servers_send() {
        assert_eq!(http2time("Thu, 01 Jan 1970 00:00:01 GMT"), Some(1));
        assert_eq!(
            http2time("Fri, 31-Dec-2060 23:59:59 GMT"),
            Some(2_871_763_199)
        );
        assert_eq!(http2time("Sun, 06 Nov 94 08:49:37 GMT"), Some(784_111_777));
        assert_eq!(http2time("garbage"), None);
        assert_eq!(http2time("Thu, 01 Jan 1970 00:00:01 PST"), None);
    }

    #[test]
    fn a_redirect_hop_reads_its_cookies_from_its_own_jar() {
        // `resolve_redirects` copies the previous request's jar, stores the answer's cookies in
        // it and merges the session's. The session stores each hop's answer twice, so a cookie
        // the answer both deletes and sets goes to the end of the session's list the second time,
        // while the hop's own jar (and so the header of the next hop) keeps the answer's order.
        let answer = headers(&[
            "id=v1; Domain=a.example",
            "csrf=1; Max-Age=-5",
            "csrf=1",
            "pair=2",
        ]);
        let target = url("https://a.example/mid");
        let mut session = CookieJar::default();
        session.extract(&target, &answer);
        session.extract(&target, &answer);
        let mut hop = CookieJar::default();
        hop.extract(&target, &answer);
        hop.merge_from(&session);
        let next = "https://a.example/mid/next";
        assert_eq!(header(&hop, next), Some("id=v1; csrf=1; pair=2".into()));
        assert_eq!(header(&session, next), Some("id=v1; pair=2; csrf=1".into()));
    }
}
