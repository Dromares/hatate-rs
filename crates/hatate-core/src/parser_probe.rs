//! Parser Health's live check: one known post per site, fetched for real, and the parser held to
//! what it must still read out of it (`core/parser_probe.py` at `76c85c4`; DAN-2501, which ports
//! it because `gui/parser_health_dialog.py:53` calls `parser_probe.run` from the dialog's
//! "Check every site now" button).
//!
//! The Python module is not part of any test run: it is slow, needs the network and reports a site
//! being down as a failure, which is right for something a person chooses to run. The same holds
//! here: nothing in the test suite reaches a real site. [`run`] takes the fetch as a
//! [`PageFetch`], the seam `patch("core.boorus.fetch_page_info")` is in Python's tests; the shipped
//! fetch is [`LiveFetch`], and `tests/parser_probe_https.rs` runs it over https to a local server.

use crate::boorus::fetch::{BooruSession, FetchOptions, fetch_page_info};
use crate::boorus::{BooruPageInfo, find_parser};
use crate::net::NetClient;
use std::time::Instant;
use tracing::info;

/// `PROBES`: one known post per site, long-standing and unremarkable. A deleted one is reported
/// as a failing site, which is right: the reference needs replacing.
pub const PROBES: [(&str, &str); 13] = [
    ("danbooru", "https://danbooru.donmai.us/posts/1000000"),
    (
        "gelbooru",
        "https://gelbooru.com/index.php?page=post&s=view&id=5053544",
    ),
    (
        "safebooru",
        "https://safebooru.org/index.php?page=post&s=view&id=7088660",
    ),
    (
        "rule34",
        "https://rule34.xxx/index.php?page=post&s=view&id=5000000",
    ),
    (
        "xbooru",
        "https://xbooru.com/index.php?page=post&s=view&id=1272540",
    ),
    ("moebooru", "https://yande.re/post/show/933082"),
    ("e621", "https://e621.net/post/show/219935"),
    ("zerochan", "https://www.zerochan.net/3793685"),
    (
        "animepictures",
        "https://anime-pictures.net/pictures/view_post/596771",
    ),
    ("eshuushuu", "https://e-shuushuu.net/images/1118149"),
    (
        "deviantart",
        "https://www.deviantart.com/billjersey/art/Mad-Moxxi-160109252",
    ),
    (
        "pawchive",
        "https://pawchive.pw/patreon/user/50093102/post/83421639",
    ),
    (
        "twitter",
        "https://twitter.com/i/web/status/960455946269945856",
    ),
];

/// `NO_TAGS_EXPECTED`: sites that legitimately produce no tags, so none is not a fault.
pub const NO_TAGS_EXPECTED: [&str; 3] = ["deviantart", "twitter", "pawchive"];

/// `NO_DIMENSIONS_EXPECTED`: sites that do not publish the original's dimensions.
pub const NO_DIMENSIONS_EXPECTED: [&str; 1] = ["pawchive"];

/// `ProbeResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeResult {
    pub site: String,
    pub url: String,
    pub ok: bool,
    pub tags: usize,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub has_preview: bool,
    pub has_file_url: bool,
    pub problem: Option<String>,
    pub seconds: f64,
    pub restricted: Option<String>,
}

impl ProbeResult {
    /// A result that has not read anything yet (the dataclass defaults).
    pub fn new(site: &str, url: &str, ok: bool) -> ProbeResult {
        ProbeResult {
            site: site.to_owned(),
            url: url.to_owned(),
            ok,
            tags: 0,
            width: None,
            height: None,
            has_preview: false,
            has_file_url: false,
            problem: None,
            seconds: 0.0,
            restricted: None,
        }
    }

    /// `ProbeResult.summary`. Python tests truthiness: an empty `problem`, an empty `restricted`
    /// and a `0` dimension all count as absent.
    pub fn summary(&self) -> String {
        if !self.ok {
            return self
                .problem
                .clone()
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| "failed".to_owned());
        }
        if self.restricted.as_deref().is_some_and(|r| !r.is_empty()) {
            return format!(
                "{} tags, held back by the site (not a parser fault)",
                self.tags
            );
        }
        let mut parts = vec![format!("{} tags", self.tags)];
        if let (Some(w), Some(h)) = (
            self.width.filter(|v| *v != 0),
            self.height.filter(|v| *v != 0),
        ) {
            parts.push(format!("{w}x{h}"));
        }
        parts.push(
            if self.has_preview {
                "preview"
            } else {
                "NO preview"
            }
            .to_owned(),
        );
        if self.has_file_url {
            parts.push("file".to_owned());
        }
        parts.join(", ")
    }
}

/// `fetch_page_info(url, timeout=timeout)`: the fetch the probe makes. An `Err` is the text of the
/// exception Python would have raised.
pub trait PageFetch {
    fn fetch(&self, url: &str, timeout: f64) -> Result<BooruPageInfo, String>;
}

/// The shipped fetch: `fetch_page_info` over the app's own net layer and booru session, so the
/// check also fills the session column of Parser Health, as Python's module-level health does.
/// It blocks the calling thread (a worker thread, never the window's).
pub struct LiveFetch<S> {
    net: NetClient,
    /// Whatever owns the booru session (`BooruSession` itself, or the engine that holds it).
    session: S,
    runtime: tokio::runtime::Runtime,
}

impl<S: AsRef<BooruSession>> LiveFetch<S> {
    pub fn new(net: NetClient, session: S) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        Ok(LiveFetch {
            net,
            session,
            runtime,
        })
    }
}

impl<S: AsRef<BooruSession>> PageFetch for LiveFetch<S> {
    fn fetch(&self, url: &str, timeout: f64) -> Result<BooruPageInfo, String> {
        let opts = FetchOptions {
            timeout,
            ..FetchOptions::default()
        };
        self.runtime
            .block_on(fetch_page_info(&self.net, self.session.as_ref(), url, opts))
            .map_err(|e| e.to_string())
    }
}

/// `run(timeout, on_progress, only)` over [`PROBES`].
pub fn run(
    fetcher: &dyn PageFetch,
    timeout: f64,
    on_progress: &dyn Fn(usize, usize, &str),
    only: &[&str],
) -> Vec<ProbeResult> {
    run_over(fetcher, &PROBES, timeout, on_progress, only)
}

/// [`run`] over another list of probes (a test points the sites at a local server).
/// `on_progress(done, total, site)` fires before each fetch, and once more with `total, total, ""`
/// when the last one is done. An empty `only` is every site.
pub fn run_over(
    fetcher: &dyn PageFetch,
    probes: &[(&str, &str)],
    timeout: f64,
    on_progress: &dyn Fn(usize, usize, &str),
    only: &[&str],
) -> Vec<ProbeResult> {
    let targets: Vec<(&str, &str)> = probes
        .iter()
        .copied()
        .filter(|(site, _)| only.is_empty() || only.contains(site))
        .collect();
    let mut results = Vec::with_capacity(targets.len());
    for (index, &(site, url)) in targets.iter().enumerate() {
        on_progress(index, targets.len(), site);
        let started = Instant::now();
        let page = match fetcher.fetch(url, timeout) {
            Ok(page) => page,
            Err(text) => {
                let mut result = ProbeResult::new(site, url, false);
                result.problem = Some(text.chars().take(160).collect());
                result.seconds = started.elapsed().as_secs_f64();
                results.push(result);
                continue;
            }
        };
        let elapsed = started.elapsed().as_secs_f64();
        let tags = page.tags.len();
        let restricted = page.restricted.as_deref().is_some_and(|r| !r.is_empty());
        let problem = if !page.fetched {
            Some("no parser matched this URL")
        } else if tags == 0 && !NO_TAGS_EXPECTED.contains(&site) {
            Some("fetched, but produced no tags")
        } else if restricted {
            // The site is holding this post back rather than the parser failing to read it.
            // Calling that a fault would train the reader to ignore this report.
            None
        } else if page.preview_url.as_deref().is_none_or(str::is_empty)
            && page.file_url.as_deref().is_none_or(str::is_empty)
        {
            Some("no picture of any kind")
        } else if (page.width.is_none_or(|w| w == 0) || page.height.is_none_or(|h| h == 0))
            && !NO_DIMENSIONS_EXPECTED.contains(&site)
        {
            Some("no dimensions - the upgrade comparison needs these")
        } else {
            None
        };
        let mut result = ProbeResult::new(site, url, problem.is_none());
        result.tags = tags;
        result.width = page.width;
        result.height = page.height;
        result.has_preview = page.preview_url.as_deref().is_some_and(|p| !p.is_empty());
        result.has_file_url = page.file_url.as_deref().is_some_and(|p| !p.is_empty());
        result.restricted = page.restricted.clone();
        result.problem = problem.map(str::to_owned);
        result.seconds = elapsed;
        info!("Parser probe {site}: {}", result.summary());
        results.push(result);
    }
    on_progress(targets.len(), targets.len(), "");
    results
}

/// Whether the parser the probe URL claims is the one that matches it: a reference URL that
/// drifted would test the wrong parser, or none (`test_every_probe_url_matches_the_parser_it_claims`).
pub fn parser_of(url: &str) -> Option<&'static str> {
    find_parser(url).map(|p| p.name)
}

#[cfg(test)]
mod tests {
    //! `tests/test_core_utils.py::TestParserProbe`, case by case, over a scripted fetch.

    use super::*;
    use crate::model::{Tag, TagSource};
    use std::cell::RefCell;

    struct Scripted(Result<BooruPageInfo, String>);

    impl PageFetch for Scripted {
        fn fetch(&self, _: &str, _: f64) -> Result<BooruPageInfo, String> {
            self.0.clone()
        }
    }

    /// `_info`: one tag, 800x600, a preview and a file, fetched.
    fn info(edit: impl FnOnce(&mut BooruPageInfo)) -> BooruPageInfo {
        let mut page = BooruPageInfo::default();
        page.tags = vec![Tag {
            name: "1girl".into(),
            namespace: None,
            source: TagSource::Booru,
        }];
        page.width = Some(800);
        page.height = Some(600);
        page.preview_url = Some("https://x/p.jpg".into());
        page.file_url = Some("https://x/f.jpg".into());
        page.fetched = true;
        edit(&mut page);
        page
    }

    /// `_run`: the one result for `site`.
    fn run_one(fetch: Result<BooruPageInfo, String>, site: &str) -> ProbeResult {
        run(&Scripted(fetch), 30.0, &|_, _, _| {}, &[site]).remove(0)
    }

    #[test]
    fn a_healthy_site_passes() {
        assert!(run_one(Ok(info(|_| {})), "danbooru").ok);
    }

    #[test]
    fn no_tags_is_a_fault_for_a_booru() {
        let r = run_one(Ok(info(|p| p.tags.clear())), "danbooru");
        assert!(!r.ok);
        assert!(r.problem.unwrap().contains("no tags"));
    }

    #[test]
    fn no_tags_is_expected_for_sites_without_them() {
        assert!(run_one(Ok(info(|p| p.tags.clear())), "twitter").ok);
    }

    #[test]
    fn missing_dimensions_are_a_fault() {
        let r = run_one(
            Ok(info(|p| {
                p.width = None;
                p.height = None;
            })),
            "danbooru",
        );
        assert!(!r.ok);
        assert!(r.problem.unwrap().contains("dimensions"));
        // pawchive states neither, and that is not a fault.
        assert!(
            run_one(
                Ok(info(|p| {
                    p.width = None;
                    p.height = None;
                })),
                "pawchive"
            )
            .ok
        );
    }

    #[test]
    fn a_post_the_site_withholds_is_not_a_parser_fault() {
        let r = run_one(
            Ok(info(|p| {
                p.preview_url = None;
                p.file_url = None;
                p.restricted = Some("needs an account".into());
            })),
            "danbooru",
        );
        assert!(r.ok);
        assert!(r.summary().contains("held back"));
    }

    #[test]
    fn a_fetch_failure_is_reported_not_raised() {
        let r = run_one(Err("site is down".into()), "danbooru");
        assert!(!r.ok);
        assert!(r.problem.unwrap().contains("site is down"));
    }

    #[test]
    fn a_failure_text_is_cut_at_160_characters() {
        let r = run_one(Err("é".repeat(300)), "danbooru");
        assert_eq!(r.problem.unwrap().chars().count(), 160);
    }

    #[test]
    fn a_page_no_parser_matched_and_a_page_with_no_picture_are_faults() {
        let r = run_one(Ok(info(|p| p.fetched = false)), "danbooru");
        assert_eq!(r.problem.as_deref(), Some("no parser matched this URL"));
        let r = run_one(
            Ok(info(|p| {
                p.preview_url = None;
                p.file_url = None;
            })),
            "danbooru",
        );
        assert_eq!(r.problem.as_deref(), Some("no picture of any kind"));
        assert_eq!(r.summary(), "no picture of any kind");
    }

    #[test]
    fn progress_is_reported_for_each_site() {
        let seen = RefCell::new(Vec::new());
        let fetch = Scripted(Ok(info(|_| {})));
        run(
            &fetch,
            30.0,
            &|d, t, s| seen.borrow_mut().push((d, t, s.to_owned())),
            &["danbooru", "e621"],
        );
        let seen = seen.into_inner();
        let sites: Vec<&str> = seen.iter().take(2).map(|(_, _, s)| s.as_str()).collect();
        assert_eq!(sites, ["danbooru", "e621"]);
        let last = seen.last().unwrap();
        assert_eq!(last.0, last.1, "the last call should report completion");
        assert_eq!(last.2, "");
    }

    #[test]
    fn no_filter_probes_every_site_in_order() {
        let results = run(&Scripted(Ok(info(|_| {}))), 30.0, &|_, _, _| {}, &[]);
        let sites: Vec<&str> = results.iter().map(|r| r.site.as_str()).collect();
        let want: Vec<&str> = PROBES.iter().map(|p| p.0).collect();
        assert_eq!(sites, want);
    }

    #[test]
    fn the_summary_reads_as_pythons_does() {
        let mut r = ProbeResult::new("e621", "u", true);
        r.tags = 34;
        r.width = Some(8);
        r.height = Some(9);
        r.has_preview = true;
        r.has_file_url = true;
        assert_eq!(r.summary(), "34 tags, 8x9, preview, file");
        r.has_preview = false;
        r.width = Some(0);
        assert_eq!(r.summary(), "34 tags, NO preview, file");
        let mut bad = ProbeResult::new("xbooru", "u", false);
        assert_eq!(bad.summary(), "failed");
        bad.problem = Some(String::new());
        assert_eq!(bad.summary(), "failed");
    }

    /// `test_every_probe_url_matches_the_parser_it_claims`.
    #[test]
    fn every_probe_url_matches_the_parser_it_claims() {
        for (site, url) in PROBES {
            let name = parser_of(url).unwrap_or_else(|| panic!("no parser matches the {site} URL"));
            assert_eq!(name, site, "{url}");
        }
    }
}
