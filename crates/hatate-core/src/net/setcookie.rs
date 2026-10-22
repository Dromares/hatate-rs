//! Which `Set-Cookie` lines of a response Python's cookie jar keeps (`http.cookiejar` in CPython
//! 3.14, as `requests` uses it for `Response.cookies`).
//!
//! `requests` hands every response to `extract_cookies_to_jar`, and the jar applies its default
//! policy: a cookie for another domain is refused, an `Expires` in the past (or `Max-Age` of zero)
//! deletes instead of setting, an unreadable attribute spoils the whole cookie. Reading `name=value`
//! off the line (as a first cut would) keeps cookies Python drops. The shared session keeps no
//! cookies (`docs/net.md` section 1); this is only what a caller that reads `response.cookies`
//! (the DeviantArt session renewal) sees.

use std::sync::LazyLock;

use regex::Regex;

use crate::pyurl;

/// A cookie as the jar holds it: the parts that decide which one wins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JarCookie {
    pub name: String,
    pub value: Option<String>,
    pub domain: String,
    pub path: String,
}

/// The cookies `requests` would find in `Response.cookies` for a response to `request_url` that
/// carried these `Set-Cookie` lines (decoded as Latin-1, as `http.client` reads them), at epoch
/// second `now`, in the order the jar iterates them (domain, then path, then name, each in the
/// order it was first set).
pub fn response_cookies(request_url: &str, set_cookie: &[String], now: i64) -> Vec<JarCookie> {
    let Ok(cookies) = make_cookies(request_url, set_cookie, now) else {
        // An exception inside `make_cookies` is caught, and every Netscape cookie is dropped.
        return Vec::new();
    };
    let mut jar: Vec<(String, PathJar)> = Vec::new();
    for cookie in cookies {
        if !set_ok(&cookie, request_url) {
            continue;
        }
        let by_domain = match jar.iter().position(|(d, _)| *d == cookie.jar.domain) {
            Some(at) => &mut jar[at].1,
            None => {
                jar.push((cookie.jar.domain.clone(), Vec::new()));
                &mut jar.last_mut().expect("just pushed").1
            }
        };
        let by_path = match by_domain.iter().position(|(p, _)| *p == cookie.jar.path) {
            Some(at) => &mut by_domain[at].1,
            None => {
                by_domain.push((cookie.jar.path.clone(), Vec::new()));
                &mut by_domain.last_mut().expect("just pushed").1
            }
        };
        match by_path.iter().position(|c| c.name == cookie.jar.name) {
            Some(at) => by_path[at] = cookie.jar,
            None => by_path.push(cookie.jar),
        }
    }
    jar.into_iter()
        .flat_map(|(_, paths)| paths.into_iter().flat_map(|(_, names)| names))
        .collect()
}

/// A parsed cookie and the attributes `set_ok` still has to look at.
struct Parsed {
    jar: JarCookie,
    version: i64,
    domain_specified: bool,
    port: Option<String>,
    port_specified: bool,
}

/// `str.strip` on the characters `str.isspace` knows.
fn strip(s: &str) -> &str {
    crate::pycompat::strip(s)
}

fn strip_quotes(text: &str) -> &str {
    let text = text.strip_prefix('"').unwrap_or(text);
    text.strip_suffix('"').unwrap_or(text)
}

type Attrs = Vec<(String, Option<String>)>;
/// One domain's cookies, grouped by path, then listed in the order set.
type PathJar = Vec<(String, Vec<JarCookie>)>;

/// `parse_ns_headers`: one list of (key, value) pairs per header. `Err` is an exception inside the
/// parser (a date `http2time` cannot take in).
fn parse_ns_headers(lines: &[String]) -> Result<Vec<Attrs>, ()> {
    const KNOWN: [&str; 7] = [
        "expires", "domain", "path", "secure", "version", "port", "max-age",
    ];
    let mut result = Vec::new();
    for line in lines {
        let mut pairs: Attrs = Vec::new();
        let mut version_set = false;
        for (ii, param) in line.split(';').enumerate() {
            let param = strip(param);
            let (key, val) = match param.split_once('=') {
                Some((k, v)) => (strip(k), Some(strip(v))),
                None => (param, None),
            };
            if key.is_empty() {
                if ii == 0 {
                    break;
                }
                continue;
            }
            let mut key = key.to_string();
            let mut value = val.map(str::to_string);
            if ii != 0 {
                let lc = key.to_lowercase();
                if KNOWN.contains(&lc.as_str()) {
                    key = lc;
                }
                if key == "version" {
                    value = value.map(|v| strip_quotes(&v).to_string());
                    version_set = true;
                } else if key == "expires" {
                    // `None` for an invalid date; a number is carried as its text.
                    value = match value {
                        Some(v) => http2time(strip_quotes(&v))?.map(|t| t.to_string()),
                        None => None,
                    };
                }
            }
            pairs.push((key, value));
        }
        if !pairs.is_empty() {
            if !version_set {
                pairs.push(("version".to_string(), Some("0".to_string())));
            }
            result.push(pairs);
        }
    }
    Ok(result)
}

fn make_cookies(request_url: &str, lines: &[String], now: i64) -> Result<Vec<Parsed>, ()> {
    if lines.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for attrs in parse_ns_headers(lines)? {
        if let Some(cookie) = cookie_from_attrs(&attrs, request_url, now) {
            out.push(cookie);
        }
    }
    Ok(out)
}

/// `_normalized_cookie_tuples` and `_cookie_from_cookie_tuple` for one header, then
/// `_process_rfc2109_cookies`.
fn cookie_from_attrs(attrs: &Attrs, request_url: &str, now: i64) -> Option<Parsed> {
    let (name, value) = attrs[0].clone();
    let mut standard: Vec<(String, Option<String>)> = Vec::new();
    let mut max_age_set = false;
    let value_attrs = [
        "version",
        "expires",
        "max-age",
        "domain",
        "path",
        "port",
        "comment",
        "commenturl",
    ];
    for (k, v) in &attrs[1..] {
        let lc = k.to_lowercase();
        let mut key = if value_attrs.contains(&lc.as_str()) || lc == "secure" || lc == "discard" {
            lc
        } else {
            k.clone()
        };
        let mut v = v.clone();
        let boolean = key == "secure" || key == "discard";
        if boolean && v.is_none() {
            v = Some("True".to_string());
        }
        if standard.iter().any(|(sk, _)| *sk == key) {
            continue; // only the first value is significant
        }
        if key == "domain" {
            v = Some(v?.to_lowercase());
        }
        if key == "expires" {
            if max_age_set {
                continue; // Max-Age is preferred
            }
            if v.is_none() {
                continue; // an unreadable date: a session cookie
            }
        }
        if key == "max-age" {
            max_age_set = true;
            let n = py_int(v.as_deref()?)?;
            key = "expires".to_string();
            v = Some((now + n).to_string());
        }
        if value_attrs.contains(&key.as_str()) || boolean {
            if v.is_none() && !matches!(key.as_str(), "port" | "comment" | "commenturl") {
                return None; // a missing value spoils the cookie
            }
            match standard.iter_mut().find(|(sk, _)| *sk == key) {
                Some(slot) => slot.1 = v,
                None => standard.push((key, v)),
            }
        }
    }
    let get = |key: &str| {
        standard
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };

    let version = match get("version") {
        Some(Some(v)) => Some(py_int(&v)?),
        _ => None,
    };
    let domain = get("domain").flatten();
    let path = get("path").flatten();
    let port = get("port");
    let expires = get("expires").flatten().and_then(|e| e.parse::<i64>().ok());

    let parsed_url = pyurl::urlparse(request_url, "");
    let req_path = {
        let escaped = escape_path(&split_path(&parsed_url));
        if escaped.starts_with('/') {
            escaped
        } else {
            format!("/{escaped}")
        }
    };
    let path = match path.filter(|p| !p.is_empty()) {
        Some(p) => escape_path(&p),
        None => {
            let mut p = req_path.clone();
            if let Some(i) = p.rfind('/') {
                p.truncate(if version == Some(0) { i } else { i + 1 });
            }
            if p.is_empty() { "/".to_string() } else { p }
        }
    };
    let (_, erhn) = eff_request_host(&parsed_url.netloc);
    let domain_specified = domain.is_some();
    let domain = match domain {
        None => erhn,
        Some(d) if d.starts_with('.') => d,
        Some(d) => format!(".{d}"),
    };
    let (port, port_specified) = match port {
        Some(None) => (Some(request_port(&parsed_url.netloc)), false),
        Some(Some(p)) => (Some(p.split_whitespace().collect::<String>()), true),
        None => (None, false),
    };
    // An expiry in the past is a request to delete the cookie.
    if expires.is_some_and(|e| e <= now) {
        return None;
    }
    // `_process_rfc2109_cookies`: a version 1 cookie is read as a Netscape one.
    let version = match version {
        Some(1) => 0,
        Some(v) => v,
        None => return None,
    };
    Some(Parsed {
        jar: JarCookie {
            name,
            value,
            domain,
            path,
        },
        version,
        domain_specified,
        port,
        port_specified,
    })
}

/// `set_ok`: version, name, path (strict off), domain and port.
fn set_ok(cookie: &Parsed, request_url: &str) -> bool {
    if cookie.version > 0 {
        return false; // RFC 2965 cookies are switched off
    }
    let netloc = pyurl::urlparse(request_url, "").netloc;
    if cookie.domain_specified {
        let (_, erhn) = eff_request_host(&netloc);
        let domain = &cookie.jar.domain;
        let undotted = domain.strip_prefix('.').unwrap_or(domain);
        if !undotted.contains('.') && !erhn.ends_with(".local") {
            return false;
        }
        let dotted = format!(".{erhn}");
        if !(erhn.ends_with(domain.as_str()) || erhn.ends_with(&format!("{undotted}.local")))
            && (!erhn.starts_with('.') && !dotted.ends_with(domain.as_str()))
        {
            return false;
        }
    }
    if cookie.port_specified {
        let req_port = request_port_number(&netloc);
        let Some(ports) = &cookie.port else {
            return false;
        };
        let mut found = false;
        for p in ports.split(',') {
            if py_int(p).is_none() {
                return false;
            }
            if p == req_port {
                found = true;
                break;
            }
        }
        if !found {
            return false;
        }
    }
    true
}

/// `(request-host, effective request-host name)`: the netloc without a trailing `:port`,
/// lower-cased, and the same with `.local` added when it has no dot.
fn eff_request_host(netloc: &str) -> (String, String) {
    static PORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?-u::\d+$)").expect("port"));
    let host = PORT.replace(netloc, "").to_lowercase();
    let erhn = if host.contains('.') {
        host.clone()
    } else {
        format!("{host}.local")
    };
    (host, erhn)
}

/// `request_port`: what follows the colon of the netloc, or 80; `None` when it is not a number.
fn request_port(netloc: &str) -> String {
    match netloc.find(':') {
        Some(i) => netloc[i + 1..].to_string(),
        None => "80".to_string(),
    }
}

/// The port as `set_ok_port` compares it: `"80"` when the request has none or an unreadable one.
fn request_port_number(netloc: &str) -> String {
    let port = request_port(netloc);
    if py_int(&port).is_some() {
        port
    } else {
        "80".to_string()
    }
}

/// `int(text)` for the text Python accepts: optional whitespace and sign, ASCII digits (and single
/// underscores between them).
fn py_int(text: &str) -> Option<i64> {
    let t = strip(text);
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
        || !digits.chars().all(|c| c.is_ascii_digit() || c == '_')
    {
        return None;
    }
    let n: i64 = digits.replace('_', "").parse().ok()?;
    Some(if t.starts_with('-') { -n } else { n })
}

/// `urlsplit(url).path`: `urlparse` takes `;params` off the last segment, `urlsplit` does not.
fn split_path(p: &pyurl::Parsed) -> String {
    if p.params.is_empty() {
        p.path.clone()
    } else {
        format!("{};{}", p.path, p.params)
    }
}

/// `escape_path`: `quote(path, "!#$%&'()*+,/:;=@[]~")` with the escapes upper-cased.
fn escape_path(path: &str) -> String {
    let quoted = pyurl::quote_safe(path, "!#$%&'()*+,/:;=@[]~");
    static ESCAPE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"%([0-9a-fA-F][0-9a-fA-F])").expect("escape"));
    ESCAPE
        .replace_all(&quoted, |c: &regex::Captures| {
            format!("%{}", c[1].to_uppercase())
        })
        .into_owned()
}

const MONTHS_LOWER: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// Days from 1970-01-01 to the first of `month` of `year` (proleptic Gregorian).
fn days_to_first(year: i64, month: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `_timegm`: seconds since the epoch for a UTC time tuple, or `None` when it is out of range.
/// `Err` is the `ValueError` of `datetime.date(year, ...)` for a year past 9999.
fn timegm(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    min: i64,
    sec: i64,
) -> Result<Option<i64>, ()> {
    if !((1970..).contains(&year)
        && (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && (0..=24).contains(&hour)
        && (0..=59).contains(&min)
        && (0..=61).contains(&sec))
    {
        return Ok(None);
    }
    if year > 9999 {
        return Err(());
    }
    let days = days_to_first(year, month) + day - 1;
    Ok(Some(((days * 24 + hour) * 60 + min) * 60 + sec))
}

/// `http2time`: an HTTP date as seconds since the epoch, `None` for a date it cannot read.
/// `Err` is an exception Python raises on the way (an unknown month name in the strict form).
fn http2time(text: &str) -> Result<Option<i64>, ()> {
    static STRICT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?-u)^[SMTWF][a-z][a-z], (\d\d) ([JFMASOND][a-z][a-z]) (\d\d\d\d) (\d\d):(\d\d):(\d\d) GMT$",
        )
        .expect("strict date")
    });
    static WEEKDAY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i-u)^(?:Sun|Mon|Tue|Wed|Thu|Fri|Sat)[a-z]*,?\s*").expect("weekday")
    });
    static LOOSE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?x-u)^
            (\d\d?)
               (?:\s+|[-/])
            (\w+)
                (?:\s+|[-/])
            (\d+)
            (?:
                  (?:\s+|:)
               (\d\d?):(\d\d)
               (?::(\d\d))?
            )?
               \s*
            (?:
               ([-+]?\d{2,4}|[A-Za-z]+)
               \s*
            )?
            (?:
               \(\w+\)
               \s*
            )?$",
        )
        .expect("loose date")
    });
    if let Some(g) = STRICT.captures(text) {
        let Some(mon) = MONTHS_LOWER.iter().position(|m| *m == g[2].to_lowercase()) else {
            return Err(());
        };
        return timegm(
            g[3].parse().map_err(|_| ())?,
            mon as i64 + 1,
            g[1].parse().map_err(|_| ())?,
            g[4].parse().map_err(|_| ())?,
            g[5].parse().map_err(|_| ())?,
            g[6].parse().map_err(|_| ())?,
        );
    }
    let text = text.trim_start_matches(crate::pycompat::is_space);
    let text = WEEKDAY.replace(text, "");
    let Some(g) = LOOSE.captures(&text) else {
        return Ok(None);
    };
    let tz = g.get(7).map(|m| m.as_str());
    // `(?![APap][Mm]\b)[A-Za-z]+`: a bare "am" or "pm" is not a time zone.
    if tz.is_some_and(|t| t.eq_ignore_ascii_case("am") || t.eq_ignore_ascii_case("pm")) {
        return Ok(None);
    }
    str2time(
        &g[1],
        &g[2],
        &g[3],
        g.get(4).map(|m| m.as_str()),
        g.get(5).map(|m| m.as_str()),
        g.get(6).map(|m| m.as_str()),
        tz,
    )
}

fn str2time(
    day: &str,
    mon: &str,
    yr: &str,
    hr: Option<&str>,
    min: Option<&str>,
    sec: Option<&str>,
    tz: Option<&str>,
) -> Result<Option<i64>, ()> {
    let mon = match MONTHS_LOWER.iter().position(|m| *m == mon.to_lowercase()) {
        Some(i) => i as i64 + 1,
        None => match mon.parse::<i64>() {
            Ok(n) if (1..=12).contains(&n) => n,
            _ => return Ok(None),
        },
    };
    let num =
        |s: Option<&str>| -> Result<i64, ()> { s.map_or(Ok(0), |s| s.parse().map_err(|_| ())) };
    let mut yr: i64 = yr.parse().map_err(|_| ())?;
    let day: i64 = day.parse().map_err(|_| ())?;
    let (hr, min, sec) = (num(hr)?, num(min)?, num(sec)?);
    if yr < 1000 {
        // The "obvious" year for a two- or three-digit one.
        let cur_yr = 1970 + (crate::timeutil::now_epoch() / 31_556_952.0) as i64;
        let mut m = cur_yr % 100;
        let tmp = yr;
        yr = yr + cur_yr - m;
        m -= tmp;
        if m.abs() > 50 {
            yr += if m > 0 { 100 } else { -100 };
        }
    }
    let Some(t) = timegm(yr, mon, day, hr, min, sec)? else {
        return Ok(None);
    };
    let tz = tz.unwrap_or("UTC").to_uppercase();
    let offset = if matches!(tz.as_str(), "GMT" | "UTC" | "UT" | "Z") {
        Some(0)
    } else {
        static TZ: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?-u)^([-+])?(\d\d?):?(\d\d)?$").expect("tz"));
        TZ.captures(&tz).map(|m| {
            let mut offset = 3600 * m[2].parse::<i64>().unwrap_or(0);
            if let Some(minutes) = m.get(3) {
                offset += 60 * minutes.as_str().parse::<i64>().unwrap_or(0);
            }
            if m.get(1).is_some_and(|s| s.as_str() == "-") {
                -offset
            } else {
                offset
            }
        })
    };
    Ok(offset.map(|o| t - o))
}

/// The `name=value` pairs of the DeviantArt cookies a response renewed, as `Response.cookies`
/// holds them: only the names in `rotatable`, only with a value, a later cookie of the same name
/// (another domain or path) over an earlier one.
pub fn renewed_pairs(
    request_url: &str,
    headers: &http::HeaderMap,
    rotatable: &[&str],
    now: i64,
) -> Vec<(String, String)> {
    let lines: Vec<String> = headers
        .get_all(http::header::SET_COOKIE)
        .iter()
        // `http.client` reads header bytes as Latin-1.
        .map(|v| v.as_bytes().iter().map(|b| char::from(*b)).collect())
        .collect();
    let mut out: Vec<(String, String)> = Vec::new();
    for cookie in response_cookies(request_url, &lines, now) {
        let Some(value) = cookie.value.filter(|v| !v.is_empty()) else {
            continue;
        };
        if !rotatable.contains(&cookie.name.as_str()) {
            continue;
        }
        match out.iter_mut().find(|(n, _)| *n == cookie.name) {
            Some(slot) => slot.1 = value,
            None => out.push((cookie.name, value)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000; // 2027-01-15
    const URL: &str = "https://www.deviantart.com/someone/art/x-1";

    fn kept(url: &str, lines: &[&str]) -> Vec<(String, Option<String>)> {
        let lines: Vec<String> = lines.iter().map(|l| (*l).to_string()).collect();
        response_cookies(url, &lines, NOW)
            .into_iter()
            .map(|c| (c.name, c.value))
            .collect()
    }

    fn pair(name: &str, value: &str) -> (String, Option<String>) {
        (name.to_string(), Some(value.to_string()))
    }

    #[test]
    fn a_plain_cookie_and_one_for_the_requests_own_domain_are_kept() {
        assert_eq!(
            kept(
                URL,
                &["auth=a1; Path=/; Domain=.deviantart.com; Secure; HttpOnly"]
            ),
            vec![pair("auth", "a1")]
        );
        assert_eq!(kept(URL, &["auth=a1"]), vec![pair("auth", "a1")]);
    }

    #[test]
    fn a_cookie_for_another_domain_is_refused() {
        assert!(kept(URL, &["auth=a1; Domain=.example.com"]).is_empty());
        // a bare "com" has no embedded dot
        assert!(kept(URL, &["auth=a1; Domain=com"]).is_empty());
        assert!(kept("https://fav.me/d1", &["auth=a1; Domain=.deviantart.com"]).is_empty());
        assert_eq!(
            kept("https://fav.me/d1", &["auth=a1; Domain=fav.me"]),
            vec![pair("auth", "a1")]
        );
    }

    #[test]
    fn an_expiry_in_the_past_deletes_and_one_in_the_future_sets() {
        assert!(kept(URL, &["auth=a1; Expires=Thu, 01 Jan 1970 00:00:00 GMT"]).is_empty());
        assert!(kept(URL, &["auth=a1; Max-Age=0"]).is_empty());
        assert_eq!(
            kept(URL, &["auth=a1; Expires=Fri, 01 Jan 2100 00:00:00 GMT"]),
            vec![pair("auth", "a1")]
        );
        assert_eq!(
            kept(URL, &["auth=a1; Expires=Fri, 01-Jan-2100 00:00:00 GMT"]),
            vec![pair("auth", "a1")]
        );
        assert_eq!(
            kept(URL, &["auth=a1; Max-Age=3600"]),
            vec![pair("auth", "a1")]
        );
        // An unreadable date makes it a session cookie; Max-Age wins over Expires.
        assert_eq!(
            kept(URL, &["auth=a1; Expires=garbage"]),
            vec![pair("auth", "a1")]
        );
        assert!(
            kept(
                URL,
                &["auth=a1; Max-Age=0; Expires=Fri, 01 Jan 2100 00:00:00 GMT"]
            )
            .is_empty()
        );
        assert!(kept(URL, &["auth=a1; Max-Age=soon"]).is_empty());
    }

    #[test]
    fn a_later_cookie_of_the_same_name_replaces_the_earlier_and_other_paths_stay_apart() {
        assert_eq!(
            kept(URL, &["auth=a1; Path=/", "auth=a2; Path=/"]),
            vec![pair("auth", "a2")]
        );
        assert_eq!(
            kept(URL, &["auth=a1; Path=/x", "auth=a2; Path=/"]),
            vec![pair("auth", "a1"), pair("auth", "a2")]
        );
    }

    #[test]
    fn scratch() {
        let r = kept(
            URL,
            &["auth=n22; Max-Age=0; Expires=Wed, 21 Oct 2099 07:28:00 GMT"],
        );
        assert!(r.is_empty(), "{r:?}");
    }

    #[test]
    fn version_and_port_attributes() {
        assert_eq!(kept(URL, &["auth=a1; Version=1"]), vec![pair("auth", "a1")]);
        assert!(kept(URL, &["auth=a1; Version=2"]).is_empty());
        assert!(kept(URL, &["auth=a1; Version=x"]).is_empty());
        assert!(kept(URL, &["auth=a1; Port=8080"]).is_empty());
        assert_eq!(
            kept(URL, &["auth=a1; Port=80,8080"]),
            vec![pair("auth", "a1")]
        );
        assert!(kept(URL, &["auth=a1; Domain"]).is_empty());
    }

    #[test]
    fn dates_read_the_way_http_cookiejar_reads_them() {
        assert_eq!(http2time("Thu, 01 Jan 1970 00:00:00 GMT"), Ok(Some(0)));
        assert_eq!(
            http2time("Fri, 01-Jan-2100 00:00:00 GMT"),
            Ok(Some(4_102_444_800))
        );
        assert_eq!(
            http2time("01 Jan 2100 00:00:00 +0100"),
            Ok(Some(4_102_441_200))
        );
        assert_eq!(http2time("Sun Nov  6 08:49:37 1994"), Ok(None));
        assert_eq!(http2time("nonsense"), Ok(None));
        assert_eq!(http2time("01 Jan 2100 00:00:00 pm"), Ok(None));
        assert_eq!(http2time("Thu, 01 Jxx 2100 00:00:00 GMT"), Err(()));
        assert_eq!(http2time("Thu, 01 Xyz 2100 00:00:00 GMT"), Ok(None));
    }
}
