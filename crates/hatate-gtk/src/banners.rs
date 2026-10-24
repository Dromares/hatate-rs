//! The banners and the empty variants the window shows at start-up (`gui/main_window.py` at
//! `76c85c4`): the Crashed banner (`_show_run_banner`), "Nothing survived." (`_show_nothing_survived`)
//! and the persisted SauceNAO quota pause (`_show_persisted_quota_pause`, `_show_quota_banner`,
//! `_hide_quota_banner`). 6.e owns the live-run banners and wires the buttons that act.
//!
//! Python reads `crash.log` and `app.log` for the meta line; the Rust app reads `crash.log` (a
//! Python run's faulthandler output, if any) and its own `hatate-rs.log` (UTC stamps).

use crate::shell::{QUOTA_PAUSED_NOTE, RUN_FAILED_NOTE};
use crate::startup::{self, LOG_TAIL_BYTES};
use crate::text::{
    describe_quota_pause, format_duration, plural, quota_pause_prose, waiting_review_count,
};
use crate::widgets::{self, DROP_ZONE_BELOW_WEIGHT};
use crate::window::MainWindow;
use gtk4::prelude::*;
use hatate_core::localtime::local_hhmm;
use hatate_core::model::Entry;
use hatate_core::quota_pause::{QuotaPauseFile, StartupPause};
use hatate_core::session::SessionStore;
use hatate_core::timeutil;

/// `f"{n:,}"`.
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        out.insert(0, '-');
    }
    out
}

/// The Crash log meta figure: `1 new entry`, `2 new entries`, or `no new entry`.
fn crash_log_figure(faults: usize) -> String {
    if faults > 0 {
        plural(faults as i64, "new entry", Some("new entries"))
    } else {
        "no new entry".to_owned()
    }
}

impl MainWindow {
    fn faults_in_previous_run(&self) -> usize {
        std::fs::read(self.s.paths.crash_log())
            .map(|b| startup::faults_in_previous_run(&String::from_utf8_lossy(&b)))
            .unwrap_or(0)
    }

    fn previous_run_stopped_at(&self) -> Option<f64> {
        let bytes = std::fs::read(self.s.paths.rust_log_file()).ok()?;
        startup::previous_run_stopped_at(&startup::tail_lossy(&bytes, LOG_TAIL_BYTES))
    }

    fn saved_session_age(&self) -> Option<f64> {
        SessionStore::new(&self.s.paths).saved_session_age_seconds((self.s.clock)())
    }

    /// `_show_run_banner(entries)`: populates and shows the Queue page's interrupted-run banner
    /// for an unclean-shutdown restore that brought something back: the reassuring prose with its
    /// counts, the meta line of what is on disk, and `stopped Xh Ym ago` on the run strip.
    pub fn show_run_banner(&self, entries: &[Entry]) {
        let banner = &self.s.run_banner;
        banner
            .parts
            .body
            .set_text(&crate::text::interrupted_prose(entries));
        let mut items: Vec<(String, String)> = Vec::new();
        let autosave_age = self.saved_session_age();
        if let Some(age) = autosave_age {
            items.push((
                "Last autosave".into(),
                format!("{} ago", format_duration(age, true)),
            ));
        }
        if let Some(age) = self.last_clean_exit_age() {
            items.push((
                "Last clean exit".into(),
                format!("{} ago", format_duration(age, true)),
            ));
        }
        items.push((
            "Crash log".into(),
            crash_log_figure(self.faults_in_previous_run()),
        ));
        clear_children(&banner.parts.meta_slot);
        banner
            .parts
            .meta_slot
            .append(&widgets::meta_line(&items, false, None));
        // When it stopped is when it last wrote to the log; the autosave is the fallback, and
        // either is a lower bound: a hard crash records nothing at the instant.
        let now = (self.s.clock)();
        let stopped_age = match self.previous_run_stopped_at() {
            Some(at) => Some((now - at).max(0.0)),
            None => autosave_age,
        };
        if let Some(age) = stopped_age {
            self.s
                .shell
                .run_note_label
                .set_text(&format!("stopped {} ago", format_duration(age, false)));
        }
        banner.root.set_visible(true);
    }

    /// `_show_nothing_survived()`: replaces the Queue's drop zone with the "Nothing survived."
    /// variant and says so on the run strip. Built here because its meta line reads what is on
    /// disk at this moment.
    pub fn show_nothing_survived(&self) {
        let faults = self.faults_in_previous_run();
        let mut items = vec![("Crash log".to_owned(), crash_log_figure(faults))];
        if let Some(age) = self.saved_session_age() {
            items.push((
                "Last saved session".into(),
                format!("{} ago", format_duration(age, true)),
            ));
        }
        let panel = widgets::nothing_survived(&items, DROP_ZONE_BELOW_WEIGHT);
        let w = self.downgrade();
        panel.new_search.connect_clicked({
            let w = w.clone();
            move |_| {
                if let Some(w) = w.upgrade() {
                    w.dismiss_nothing_survived();
                }
            }
        });
        panel.crash_log.connect_clicked({
            let w = w.clone();
            move |_| {
                if let Some(w) = w.upgrade() {
                    w.open_crash_log();
                }
            }
        });
        panel.add_files.connect_clicked({
            let w = w.clone();
            move |_| {
                if let Some(w) = w.upgrade() {
                    w.action_add_files();
                }
            }
        });
        self.s.queue_stack.add_child(&panel.root);
        *self.s.nothing_survived.borrow_mut() = Some(panel);
        self.s.shell.run_note_label.set_text(RUN_FAILED_NOTE);
        self.refresh_queue_empty_state();
    }

    /// `_dismiss_nothing_survived()`: back to the ordinary drop zone. The notice has been read,
    /// and the strip's "last run failed" belongs to the run it described.
    pub fn dismiss_nothing_survived(&self) {
        let Some(panel) = self.s.nothing_survived.borrow_mut().take() else {
            return;
        };
        self.s.queue_stack.remove(&panel.root);
        self.s.shell.run_note_label.set_text("");
        self.refresh_queue_empty_state();
    }

    /// `_show_persisted_quota_pause()`: surfaces a quota pause from a PREVIOUS visit, not just the
    /// moment it happened. A pause whose allowance has already come back is stale and is cleared.
    pub fn show_persisted_quota_pause(&self) {
        let now = (self.s.clock)();
        let file = QuotaPauseFile::new(&self.s.paths);
        let pause = match file.check_at_startup(now) {
            Ok(StartupPause::Active(p)) => p,
            Ok(_) => return,
            Err(e) => {
                tracing::warn!("could not check the quota pause: {e}");
                return;
            }
        };
        let quota_text = describe_quota_pause(&pause, now);
        let restore = self.s.session_restore_status.borrow().clone();
        match restore {
            Some(status) => self.say(&format!("{status} \u{b7} {quota_text}")),
            None => self.say(&quota_text),
        }
        self.show_quota_banner(pause.paused_at, pause.remaining, pause.reset_epoch());
    }

    /// `_show_quota_banner(paused_at, remaining, reset_at)`: populates and shows the quota-paused
    /// banner and puts the pause on the run strip: `paused - quota exhausted` beside the count.
    /// The counts read the per-row state, so they are right after a relaunch too.
    pub fn show_quota_banner(&self, paused_at: f64, remaining: i64, reset_at: Option<f64>) {
        self.s.quota_paused.set(true);
        let (waiting, prose) = {
            let q = self.s.queue.borrow();
            (
                waiting_review_count(q.entries()),
                quota_pause_prose(q.entries(), remaining),
            )
        };
        let banner = &self.s.quota_banner;
        banner.parts.body.set_text(&prose);
        banner
            .review
            .set_label(&format!("Review the {waiting} waiting \u{2192}"));
        // With nothing decided yet there is nothing to review: the button would open an empty
        // page.
        banner.review.set_visible(waiting > 0);
        let mut items = vec![
            ("Quota exhausted at".to_owned(), local_hhmm(paused_at)),
            ("Images waiting".to_owned(), thousands(remaining)),
        ];
        if let Some(reset) = reset_at {
            let stamp = timeutil::format_stamp(reset);
            items.push(("Resets".to_owned(), format!("{} UTC", &stamp[11..16])));
        }
        clear_children(&banner.parts.meta_slot);
        banner
            .parts
            .meta_slot
            .append(&widgets::meta_line(&items, false, Some(0)));
        self.s.shell.run_note_label.set_text(QUOTA_PAUSED_NOTE);
        // DEFERRED(6.e): `_refresh_run_progress_label()`.
        banner.root.set_visible(true);
    }

    /// `_hide_quota_banner()`: the pause is being acted on (or the list is gone): the banner, the
    /// strip's note and `ETA paused` all describe a state that no longer holds.
    pub fn hide_quota_banner(&self) {
        self.s.quota_paused.set(false);
        self.s.quota_banner.root.set_visible(false);
        if self.s.shell.run_note_label.text() == QUOTA_PAUSED_NOTE {
            self.s.shell.run_note_label.set_text("");
        }
    }
}

fn clear_children(slot: &gtk4::Box) {
    while let Some(child) = slot.first_child() {
        slot.remove(&child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_match_pythons_comma_format() {
        for (n, want) in [
            (0, "0"),
            (7, "7"),
            (999, "999"),
            (1000, "1,000"),
            (12345, "12,345"),
            (1234567, "1,234,567"),
            (-1234, "-1,234"),
        ] {
            assert_eq!(thousands(n), want);
        }
    }

    #[test]
    fn the_crash_log_figure_is_a_count_or_none() {
        assert_eq!(crash_log_figure(0), "no new entry");
        assert_eq!(crash_log_figure(1), "1 new entry");
        assert_eq!(crash_log_figure(3), "3 new entries");
    }
}
