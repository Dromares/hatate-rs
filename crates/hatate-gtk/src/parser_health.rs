//! Help > Parser Health (`gui/parser_health_dialog.py` at `76c85c4`, `action_parser_health`,
//! `_warn_about_broken_parsers`).
//!
//! The dialog answers whether each site's parser still works, two ways: what this session has seen
//! (`hatate_core::parser_health`, gathered as searches ran) and a live check that fetches one known
//! post per site. **The live check is `core/parser_probe.py`, which is not ported** (it is a
//! maintenance tool that needs the real sites, and `docs/phase5-coverage.md` leaves it to
//! Phase 7). The dialog takes the check as a [`ProbeRunner`]: the shipped window gives none, and
//! the button says so and stays off; a test gives a scripted one, so the column, the progress and
//! the summary line are all exercised without a network.

use crate::widgets;
use crate::window::MainWindow;
use gtk4 as gtk;
use gtk4::prelude::*;
use hatate_core::parser_health::SiteHealth;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

/// `parser_probe.PROBES`, the sites by name (the URLs are the live check's).
pub const PROBE_SITES: [&str; 13] = [
    "danbooru",
    "gelbooru",
    "safebooru",
    "rule34",
    "xbooru",
    "moebooru",
    "e621",
    "zerochan",
    "animepictures",
    "eshuushuu",
    "deviantart",
    "pawchive",
    "twitter",
];

/// What the disabled button says, as DAN-2290 words it.
pub const NO_LIVE_CHECK: &str = "The live check is not part of this build yet.";

/// The dialog's columns.
pub const COLUMNS: [&str; 4] = ["Site", "This session", "Live check", "Last problem"];

/// `parser_probe.ProbeResult`, the part the dialog reads.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeResult {
    pub site: String,
    pub ok: bool,
    pub tags: usize,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub has_preview: bool,
    pub has_file_url: bool,
    pub problem: Option<String>,
    pub restricted: Option<String>,
}

impl ProbeResult {
    /// `ProbeResult.summary`.
    pub fn summary(&self) -> String {
        if !self.ok {
            return self
                .problem
                .clone()
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| "failed".to_owned());
        }
        if self.restricted.is_some() {
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

/// `parser_probe.run(timeout, on_progress)`, blocking, off the main thread. `on_progress(done,
/// total, site)` fires before each fetch.
pub trait ProbeRunner: Send + Sync {
    fn run(&self, timeout: f64, on_progress: &dyn Fn(usize, usize, &str)) -> Vec<ProbeResult>;
}

/// What `_populate` puts in one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub site: String,
    pub session: String,
    /// `ink_100` (broken) / `ink_65` (confirmed working) / none.
    pub session_tone: Tone,
    pub live: String,
    pub live_tone: Tone,
    pub problem: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Ok,
    Bad,
}

/// `_populate`'s rows: every site seen this session, every site probed, and every site the live
/// check knows, sorted as Python's `sorted(set(...))` (by code point).
pub fn rows(health: &[SiteHealth], results: &BTreeMap<String, ProbeResult>) -> Vec<Row> {
    let by_site: BTreeMap<&str, &SiteHealth> =
        health.iter().map(|h| (h.site.as_str(), h)).collect();
    let mut sites: Vec<String> = by_site
        .keys()
        .map(|s| (*s).to_owned())
        .chain(results.keys().cloned())
        .chain(PROBE_SITES.iter().map(|s| (*s).to_owned()))
        .collect();
    sites.sort();
    sites.dedup();
    sites
        .into_iter()
        .map(|site| {
            let entry = by_site.get(site.as_str()).copied();
            let session = entry.map_or_else(|| "not used yet".to_owned(), SiteHealth::summary);
            let session_tone = match entry {
                Some(e) if e.suspect() => Tone::Bad,
                Some(e) if e.ok > 0 => Tone::Ok,
                _ => Tone::Plain,
            };
            let result = results.get(&site);
            let (live, live_tone) = match result {
                None => ("\u{2014}".to_owned(), Tone::Plain),
                Some(r) => (r.summary(), if r.ok { Tone::Ok } else { Tone::Bad }),
            };
            let problem = match (result, entry) {
                (Some(r), _) if !r.ok => r.problem.clone().unwrap_or_default(),
                (_, Some(e)) => e.last_problem.clone().unwrap_or_default(),
                _ => String::new(),
            };
            Row {
                site,
                session,
                session_tone,
                live,
                live_tone,
                problem,
            }
        })
        .collect()
}

/// The status line after a check: `(text, tone)`.
pub fn summary_line(results: &[ProbeResult]) -> (String, Tone) {
    let broken: Vec<&ProbeResult> = results.iter().filter(|r| !r.ok).collect();
    if results.is_empty() {
        (
            "The check could not run - see Help > View Logs.".to_owned(),
            Tone::Plain,
        )
    } else if !broken.is_empty() {
        let names: Vec<&str> = broken.iter().map(|r| r.site.as_str()).collect();
        (
            format!(
                "{} of {} sites have a problem: {}. A site being down looks the same as a broken \
                 parser here, so it is worth opening one of those posts in a browser before \
                 concluding anything.",
                broken.len(),
                results.len(),
                names.join(", ")
            ),
            Tone::Bad,
        )
    } else {
        (
            format!("All {} sites read correctly.", results.len()),
            Tone::Ok,
        )
    }
}

fn tone_class(t: Tone) -> Option<&'static str> {
    match t {
        Tone::Plain => None,
        Tone::Ok => Some("health-ok"),
        Tone::Bad => Some("health-bad"),
    }
}

/// The dialog.
pub struct ParserHealthDialog {
    pub window: gtk::Window,
    pub grid: gtk::Grid,
    pub progress: gtk::ProgressBar,
    pub status: gtk::Label,
    pub check_button: gtk::Button,
    results: RefCell<BTreeMap<String, ProbeResult>>,
    probe: Option<Arc<dyn ProbeRunner>>,
    running: Cell<bool>,
    main: MainWindow,
}

impl ParserHealthDialog {
    fn build(main: &MainWindow, probe: Option<Arc<dyn ProbeRunner>>) -> Rc<ParserHealthDialog> {
        let window = gtk::Window::builder()
            .title("Parser Health")
            .transient_for(&main.win)
            .modal(true)
            .default_width(760)
            .default_height(460)
            .build();
        let column = gtk::Box::new(gtk::Orientation::Vertical, widgets::PAGE_SPACING);
        let (l, t, r, b) = widgets::PAGE_MARGINS;
        column.set_margin_start(l);
        column.set_margin_top(t);
        column.set_margin_end(r);
        column.set_margin_bottom(b);
        column.append(&widgets::heading("Parser Health"));
        let intro = gtk::Label::new(Some(
            "Whether each site's tag parsing still works.\nA parser that breaks does so silently - \
             matches keep appearing, just without tags - so this is the place to check when one \
             site's results look thin.",
        ));
        intro.set_xalign(0.0);
        intro.set_wrap(true);
        column.append(&intro);

        let grid = gtk::Grid::new();
        grid.set_column_spacing(24);
        grid.set_row_spacing(6);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_child(Some(&grid));
        column.append(&scroll);

        let progress = gtk::ProgressBar::new();
        progress.set_visible(false);
        column.append(&progress);
        let status = gtk::Label::new(Some(""));
        status.set_xalign(0.0);
        status.set_wrap(true);
        column.append(&status);

        let row = gtk::Box::new(gtk::Orientation::Horizontal, widgets::BUTTON_ROW_SPACING);
        let check_button = widgets::pill_button("Check every site now", false, false);
        check_button.set_tooltip_text(Some(
            "Fetches one known post from each site and checks the parser still reads it.\nThis \
             makes real requests to a dozen sites, so it takes a few seconds - and a site being \
             down will show here as a failure, which is worth knowing too.",
        ));
        if probe.is_none() {
            // DAN-2290 (Cloud): disabled, with exactly this text; and the same text as a caption
            // under the button, because GTK may not show a tooltip on an insensitive widget.
            check_button.set_sensitive(false);
            check_button.set_tooltip_text(Some(NO_LIVE_CHECK));
            let caption = gtk::Label::new(Some(NO_LIVE_CHECK));
            caption.add_css_class("muted");
            caption.set_xalign(0.0);
            column.append(&caption);
        }
        row.append(&check_button);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        row.append(&spacer);
        let close = widgets::pill_button("Close", false, false);
        row.append(&close);
        column.append(&row);
        window.set_child(Some(&column));

        let dialog = Rc::new(ParserHealthDialog {
            window: window.clone(),
            grid,
            progress,
            status,
            check_button: check_button.clone(),
            results: RefCell::new(BTreeMap::new()),
            probe,
            running: Cell::new(false),
            main: main.clone(),
        });
        {
            let w = window.clone();
            close.connect_clicked(move |_| w.close());
        }
        {
            let d = Rc::downgrade(&dialog);
            check_button.connect_clicked(move |_| {
                if let Some(d) = d.upgrade() {
                    d.start_probe();
                }
            });
        }
        dialog.populate();
        dialog
    }

    /// `_populate`.
    pub fn populate(&self) {
        let health = self.main.parser_health_snapshot();
        let rows = rows(&health, &self.results.borrow());
        while let Some(child) = self.grid.first_child() {
            self.grid.remove(&child);
        }
        // Without a live check there is no "Live check" column: a column of dashes would imply
        // one runs (DAN-2290 condition 1).
        let shown = self.shown_columns();
        for (c, &col) in shown.iter().enumerate() {
            let h = widgets::section_label(COLUMNS[col]);
            h.set_xalign(0.0);
            self.grid.attach(&h, c as i32, 0, 1, 1);
        }
        for (i, r) in rows.iter().enumerate() {
            let y = i as i32 + 1;
            let cells = [
                (&r.site, Tone::Plain),
                (&r.session, r.session_tone),
                (&r.live, r.live_tone),
                (&r.problem, Tone::Plain),
            ];
            for (c, &col) in shown.iter().enumerate() {
                let (text, tone) = cells[col];
                let l = gtk::Label::new(Some(text));
                l.set_xalign(0.0);
                l.set_selectable(true);
                if let Some(class) = tone_class(tone) {
                    l.add_css_class(class);
                }
                self.grid.attach(&l, c as i32, y, 1, 1);
            }
        }
    }

    /// The indexes into [`COLUMNS`] the dialog shows.
    pub fn shown_columns(&self) -> Vec<usize> {
        if self.probe.is_some() {
            vec![0, 1, 2, 3]
        } else {
            vec![0, 1, 3]
        }
    }

    /// Every cell's text, row by row (header first).
    pub fn texts(&self) -> Vec<Vec<String>> {
        let mut out: Vec<Vec<String>> = Vec::new();
        let mut y = 0;
        loop {
            let mut row = Vec::new();
            let width = self.shown_columns().len();
            for x in 0..width as i32 {
                match self.grid.child_at(x, y).and_downcast::<gtk::Label>() {
                    Some(l) => row.push(l.text().to_string()),
                    None => break,
                }
            }
            if row.len() != width {
                break;
            }
            out.push(row);
            y += 1;
        }
        out
    }

    /// `_start_probe`.
    fn start_probe(self: &Rc<Self>) {
        let Some(probe) = self.probe.clone() else {
            return;
        };
        if self.running.replace(true) {
            return;
        }
        self.check_button.set_sensitive(false);
        self.progress.set_visible(true);
        self.progress.set_fraction(0.0);
        self.status.set_text("Checking\u{2026}");
        let timeout = self.main.s.settings.lock().unwrap().search_timeout;
        let me = Rc::downgrade(self);
        let job = self.main.s.workers.run(
            move |ctx: &crate::worker::Ctx<(usize, usize, String)>| {
                // A diagnostic window must not be able to take the app down.
                let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    probe.run(timeout, &|d, t, s| ctx.report((d, t, s.to_owned())))
                }));
                run.unwrap_or_default()
            },
            move |event| {
                let Some(d) = me.upgrade() else { return };
                match event {
                    crate::worker::Event::Progress((done, total, site)) => {
                        d.on_progress(done, total, &site)
                    }
                    crate::worker::Event::Done(results) => d.on_done(results),
                }
            },
        );
        self.main.s.parser_probe_worker.replace(job);
    }

    fn on_progress(&self, done: usize, total: usize, site: &str) {
        self.progress.set_fraction(if total == 0 {
            0.0
        } else {
            done as f64 / total as f64
        });
        if !site.is_empty() {
            self.status.set_text(&format!("Checking {site}\u{2026}"));
        }
    }

    fn on_done(&self, results: Vec<ProbeResult>) {
        self.running.set(false);
        self.check_button.set_sensitive(true);
        self.progress.set_visible(false);
        *self.results.borrow_mut() = results
            .iter()
            .map(|r| (r.site.clone(), r.clone()))
            .collect();
        self.populate();
        let (text, tone) = summary_line(&results);
        self.status.set_text(&text);
        for c in ["health-ok", "health-bad"] {
            self.status.remove_css_class(c);
        }
        if let Some(c) = tone_class(tone) {
            self.status.add_css_class(c);
        }
    }
}

impl MainWindow {
    /// `hatate_core::parser_health` as the engines fill it, worst first.
    pub fn parser_health_snapshot(&self) -> Vec<SiteHealth> {
        self.s
            .search
            .backend_if_built()
            .map(|b| b.engine.booru.health.snapshot())
            .unwrap_or_default()
    }

    /// `action_parser_health`: the dialog, modal over the window.
    pub fn action_parser_health(&self) {
        let dialog = ParserHealthDialog::build(self, self.s.probe.clone());
        dialog.window.present();
        *self.s.parser_health_dialog.borrow_mut() = Some(dialog);
    }

    /// `_warn_about_broken_parsers`: says something the first time a site looks broken, once per
    /// site, and only for a site that has never succeeded this session.
    pub fn warn_about_broken_parsers(&self) {
        let Some(backend) = self.s.search.backend_if_built() else {
            return;
        };
        for health in backend.engine.booru.health.newly_suspect() {
            tracing::warn!(
                "Parser for {} looks broken: {}",
                health.site,
                health.summary()
            );
            self.say(&format!(
                "{}: no tags in {} attempts - see Help > Parser Health",
                health.site, health.consecutive_empty
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hatate_core::parser_health::ParserHealth;

    fn result(site: &str, ok: bool, problem: Option<&str>) -> ProbeResult {
        ProbeResult {
            site: site.to_owned(),
            ok,
            tags: 7,
            width: Some(800),
            height: Some(600),
            has_preview: true,
            has_file_url: true,
            problem: problem.map(str::to_owned),
            restricted: None,
        }
    }

    #[test]
    fn every_site_is_listed_before_anything_is_known() {
        let rows = rows(&[], &BTreeMap::new());
        assert_eq!(rows.len(), PROBE_SITES.len());
        assert!(
            rows.iter()
                .all(|r| r.session == "not used yet" && r.live == "\u{2014}")
        );
        let sites: Vec<&str> = rows.iter().map(|r| r.site.as_str()).collect();
        let mut sorted = sites.clone();
        sorted.sort();
        assert_eq!(sites, sorted);
    }

    #[test]
    fn a_broken_parser_shows_in_the_session_column_with_its_problem() {
        let h = ParserHealth::new();
        for _ in 0..3 {
            h.record_empty("e621", "https://e621.net/posts/1");
        }
        h.record_success("danbooru", 5);
        let rows = rows(&h.snapshot(), &BTreeMap::new());
        let e621 = rows.iter().find(|r| r.site == "e621").unwrap();
        assert_eq!(e621.session, "no tags in 3 attempts");
        assert_eq!(e621.session_tone, Tone::Bad);
        assert_eq!(e621.problem, "returned no tags");
        let dan = rows.iter().find(|r| r.site == "danbooru").unwrap();
        assert_eq!((dan.session.as_str(), dan.session_tone), ("1 ok", Tone::Ok));
    }

    #[test]
    fn live_results_fill_the_check_column_and_a_failure_names_its_problem() {
        let mut results = BTreeMap::new();
        results.insert("gelbooru".to_owned(), result("gelbooru", true, None));
        results.insert(
            "twitter".to_owned(),
            result("twitter", false, Some("no parser matched this URL")),
        );
        let rows = rows(&[], &results);
        let gel = rows.iter().find(|r| r.site == "gelbooru").unwrap();
        assert_eq!(gel.live, "7 tags, 800x600, preview, file");
        assert_eq!(gel.live_tone, Tone::Ok);
        let tw = rows.iter().find(|r| r.site == "twitter").unwrap();
        assert_eq!(tw.live, "no parser matched this URL");
        assert_eq!(tw.problem, "no parser matched this URL");
        assert_eq!(tw.live_tone, Tone::Bad);
    }

    #[test]
    fn the_summary_line_says_all_clear_names_the_broken_and_never_clears_a_check_that_did_not_run()
    {
        assert_eq!(
            summary_line(&[result("a", true, None), result("b", true, None)]),
            ("All 2 sites read correctly.".to_owned(), Tone::Ok)
        );
        let (text, tone) = summary_line(&[result("a", true, None), result("b", false, Some("x"))]);
        assert!(text.starts_with("1 of 2 sites have a problem: b. A site being down"));
        assert_eq!(tone, Tone::Bad);
        let (text, tone) = summary_line(&[]);
        assert_eq!(text, "The check could not run - see Help > View Logs.");
        assert_eq!(tone, Tone::Plain);
    }

    #[test]
    fn a_restricted_post_is_not_a_parser_fault() {
        let mut r = result("pawchive", true, None);
        r.restricted = Some("login".to_owned());
        assert_eq!(
            r.summary(),
            "7 tags, held back by the site (not a parser fault)"
        );
        assert_eq!(result("x", false, None).summary(), "failed");
        let mut r = result("x", true, None);
        r.has_preview = false;
        r.has_file_url = false;
        r.width = None;
        assert_eq!(r.summary(), "7 tags, NO preview");
    }
}
