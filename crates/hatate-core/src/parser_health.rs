//! Whether each site's parser is actually working (`core/parser_health.py`).
//!
//! Safebooru, rule34 and Xbooru once extracted zero tags, possibly for as long as they had been
//! supported, and nothing said so. A parser that stops working is silent and stays silent; this
//! records what every fetch produced so the app can say "e621 has returned nothing 14 times this
//! session" instead of leaving it to be spotted by eye.
//!
//! In-memory and per-session on purpose: a health signal, not an audit trail. Python keeps it in
//! a module global; here it is a value the fetching code holds.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// How many fetches in a row a parser may return nothing for before it is called broken. One
/// empty result is ordinary: a post can have no tags, and a hash-keyed lookup like Sankaku's
/// misses far more often than it hits.
pub const CONSECUTIVE_EMPTIES_BEFORE_SUSPECT: u32 = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct SiteHealth {
    pub site: String,
    /// Fetches that produced tags.
    pub ok: u32,
    /// Fetched fine, parsed to nothing.
    pub empty: u32,
    /// The fetch itself failed.
    pub failed: u32,
    pub consecutive_empty: u32,
    pub last_ok_at: Option<f64>,
    pub last_problem: Option<String>,
    pub last_problem_at: Option<f64>,
    /// Already surfaced to the user once.
    pub reported: bool,
}

impl SiteHealth {
    fn new(site: &str) -> Self {
        SiteHealth {
            site: site.to_string(),
            ok: 0,
            empty: 0,
            failed: 0,
            consecutive_empty: 0,
            last_ok_at: None,
            last_problem: None,
            last_problem_at: None,
            reported: false,
        }
    }

    pub fn attempts(&self) -> u32 {
        self.ok + self.empty + self.failed
    }

    /// Looks broken, as opposed to merely having missed: never succeeded this session, and a run
    /// of empties. A parser that works and then hits a run of tagless posts is not broken.
    pub fn suspect(&self) -> bool {
        self.ok == 0 && self.consecutive_empty >= CONSECUTIVE_EMPTIES_BEFORE_SUSPECT
    }

    pub fn summary(&self) -> String {
        if self.suspect() {
            format!("no tags in {} attempts", self.consecutive_empty)
        } else if self.ok > 0 {
            let mut s = format!("{} ok", self.ok);
            if self.empty > 0 {
                s.push_str(&format!(", {} empty", self.empty));
            }
            s
        } else if self.failed > 0 {
            format!("{} failed", self.failed)
        } else {
            "not used yet".to_string()
        }
    }
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

#[derive(Debug, Default)]
pub struct ParserHealth {
    sites: Mutex<HashMap<String, SiteHealth>>,
}

impl ParserHealth {
    pub fn new() -> Self {
        Self::default()
    }

    fn with<R>(&self, site: &str, f: impl FnOnce(&mut SiteHealth) -> R) -> R {
        let mut sites = self.sites.lock().unwrap();
        f(sites
            .entry(site.to_string())
            .or_insert_with(|| SiteHealth::new(site)))
    }

    pub fn record_success(&self, site: &str, _tag_count: usize) {
        self.with(site, |h| {
            h.ok += 1;
            h.consecutive_empty = 0;
            h.last_ok_at = Some(now());
        });
    }

    /// Fetched fine but parsed to nothing: the shape of a broken parser.
    pub fn record_empty(&self, site: &str, _url: &str) {
        self.with(site, |h| {
            h.empty += 1;
            h.consecutive_empty += 1;
            h.last_problem = Some("returned no tags".to_string());
            h.last_problem_at = Some(now());
        });
    }

    /// The fetch itself did not work. Kept apart from an empty parse: a site being down says
    /// nothing about whether its parser still fits.
    pub fn record_failure(&self, site: &str, _url: &str, reason: &str) {
        self.with(site, |h| {
            h.failed += 1;
            h.last_problem = Some(reason.to_string());
            h.last_problem_at = Some(now());
        });
    }

    /// Sites that have just crossed into looking broken, reported once each so a warning does not
    /// repeat on every image of a batch.
    pub fn newly_suspect(&self) -> Vec<SiteHealth> {
        let mut sites = self.sites.lock().unwrap();
        let mut found: Vec<SiteHealth> = Vec::new();
        for h in sites.values_mut() {
            if h.suspect() && !h.reported {
                h.reported = true;
                found.push(h.clone());
            }
        }
        found.sort_by(|a, b| a.site.cmp(&b.site));
        found
    }

    /// Every site touched this session, worst first.
    pub fn snapshot(&self) -> Vec<SiteHealth> {
        let sites = self.sites.lock().unwrap();
        let mut all: Vec<SiteHealth> = sites.values().cloned().collect();
        all.sort_by_key(|h| (!h.suspect(), h.ok > 0, h.site.to_lowercase()));
        all
    }

    pub fn reset(&self) {
        self.sites.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_empties_with_no_success_is_suspect_and_is_reported_once() {
        let h = ParserHealth::new();
        for _ in 0..3 {
            h.record_empty("safebooru", "u");
        }
        let found = h.newly_suspect();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].summary(), "no tags in 3 attempts");
        assert!(h.newly_suspect().is_empty());
    }

    #[test]
    fn a_success_clears_the_run_and_a_failure_is_not_an_empty() {
        let h = ParserHealth::new();
        h.record_empty("e621", "u");
        h.record_empty("e621", "u");
        h.record_success("e621", 4);
        h.record_failure("e621", "u", "down");
        let s = &h.snapshot()[0];
        assert_eq!((s.ok, s.empty, s.failed, s.consecutive_empty), (1, 2, 1, 0));
        assert!(!s.suspect());
        assert_eq!(s.summary(), "1 ok, 2 empty");
        assert_eq!(s.last_problem.as_deref(), Some("down"));
    }

    #[test]
    fn the_worst_site_is_listed_first() {
        let h = ParserHealth::new();
        h.record_success("a", 1);
        for _ in 0..3 {
            h.record_empty("z", "u");
        }
        h.record_failure("m", "u", "x");
        let names: Vec<_> = h.snapshot().into_iter().map(|s| s.site).collect();
        assert_eq!(names, ["z", "m", "a"]);
    }
}
