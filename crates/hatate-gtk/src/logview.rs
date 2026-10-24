//! Help > View Logs (`gui/log_viewer_dialog.py` at `76c85c4`): recent log lines, and `crash.log`
//! behind a toggle.
//!
//! Python reads an in-memory ring of the last 4,000 records; that ring (and `get_recent_logs`) is
//! 6.b's. Until it lands the dialog reads through the [`LogSource`] it is given, and [`FileLogs`]
//! is the plain file tail. The Rust app's own log is `hatate-rs.log`, never `app.log` (the Python
//! app reads that one), so the path label says so.

use crate::startup::{CRASH_LOG_TAIL_CHARS, LOG_TAIL_BYTES, tail_lossy};
use crate::widgets;
use gtk4 as gtk;
use gtk4::prelude::*;
use hatate_core::paths::Paths;
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Where the dialog reads from.
pub trait LogSource {
    /// `get_log_file_path()`.
    fn log_path(&self) -> PathBuf;
    /// `get_recent_logs()`.
    fn recent_log(&self) -> String;
    /// `get_crash_log_path()`.
    fn crash_log_path(&self) -> PathBuf;
    /// `get_recent_crash_log()`: the tail of `crash.log`, empty when there is none.
    fn recent_crash_log(&self) -> String;
}

/// The profile's files, read when asked.
pub struct FileLogs {
    paths: Paths,
}

impl FileLogs {
    pub fn new(paths: &Paths) -> FileLogs {
        FileLogs {
            paths: paths.clone(),
        }
    }
}

impl LogSource for FileLogs {
    fn log_path(&self) -> PathBuf {
        self.paths.rust_log_file()
    }

    fn recent_log(&self) -> String {
        std::fs::read(self.log_path())
            .map(|b| tail_lossy(&b, LOG_TAIL_BYTES))
            .unwrap_or_default()
    }

    fn crash_log_path(&self) -> PathBuf {
        self.paths.crash_log()
    }

    fn recent_crash_log(&self) -> String {
        let Ok(bytes) = std::fs::read(self.crash_log_path()) else {
            return String::new();
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        // `data[-CRASH_LOG_TAIL_CHARS:]` counts characters.
        let n = text.chars().count();
        if n > CRASH_LOG_TAIL_CHARS {
            text.chars().skip(n - CRASH_LOG_TAIL_CHARS).collect()
        } else {
            text
        }
    }
}

/// The dialog's text for a source, without any widget: what [`LogViewer::refresh`] shows.
pub fn view_text(source: &dyn LogSource, crash: bool) -> (String, String) {
    if crash {
        let logs = source.recent_crash_log();
        (
            format!("Crash log: {}", source.crash_log_path().display()),
            if logs.is_empty() {
                "(no crash log yet - nothing has crashed)".to_owned()
            } else {
                logs
            },
        )
    } else {
        let logs = source.recent_log();
        (
            format!("Log file: {}", source.log_path().display()),
            if logs.is_empty() {
                "(no log entries yet)".to_owned()
            } else {
                logs
            },
        )
    }
}

/// `LogViewerDialog`.
pub struct LogViewer {
    pub window: gtk::Window,
    pub path_label: gtk::Label,
    pub text: gtk::TextView,
    pub crash_log_btn: gtk::Button,
    source: Rc<dyn LogSource>,
    showing_crash: Cell<bool>,
}

impl LogViewer {
    pub fn new(
        parent: &impl IsA<gtk::Window>,
        source: Rc<dyn LogSource>,
        open_folder: Rc<dyn Fn(&Path)>,
        show_crash_log: bool,
    ) -> Rc<LogViewer> {
        let window = gtk::Window::builder()
            .title("Logs")
            .transient_for(parent)
            .modal(true)
            .default_width(760)
            .default_height(480)
            .build();
        let column = gtk::Box::new(gtk::Orientation::Vertical, widgets::PAGE_SPACING);
        let (l, t, r, b) = widgets::PAGE_MARGINS;
        column.set_margin_start(l);
        column.set_margin_top(t);
        column.set_margin_end(r);
        column.set_margin_bottom(b);
        let path_label = gtk::Label::new(None);
        path_label.add_css_class("log-path");
        path_label.set_xalign(0.0);
        path_label.set_selectable(true);
        column.append(&path_label);

        let text = gtk::TextView::new();
        text.add_css_class("log");
        text.set_editable(false);
        text.set_cursor_visible(false);
        text.set_monospace(true);
        text.set_wrap_mode(gtk::WrapMode::None);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_child(Some(&text));
        column.append(&scroll);

        let row = gtk::Box::new(gtk::Orientation::Horizontal, widgets::BUTTON_ROW_SPACING);
        let refresh = widgets::pill_button("Refresh", false, false);
        let copy = widgets::pill_button("Copy all", false, false);
        let open = widgets::pill_button("Open log file location", false, false);
        let crash_log_btn = widgets::pill_button(
            if show_crash_log {
                "View App Log"
            } else {
                "View Crash Log"
            },
            false,
            false,
        );
        for b in [&refresh, &copy, &open, &crash_log_btn] {
            row.append(b);
        }
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        row.append(&spacer);
        let close = widgets::pill_button("Close", false, false);
        row.append(&close);
        column.append(&row);
        window.set_child(Some(&column));

        let viewer = Rc::new(LogViewer {
            window: window.clone(),
            path_label,
            text,
            crash_log_btn: crash_log_btn.clone(),
            source,
            showing_crash: Cell::new(show_crash_log),
        });
        {
            let v = viewer.clone();
            refresh.connect_clicked(move |_| v.refresh());
        }
        {
            let v = viewer.clone();
            copy.connect_clicked(move |_| v.copy_all());
        }
        {
            let v = viewer.clone();
            open.connect_clicked(move |_| {
                let path = if v.showing_crash.get() {
                    v.source.crash_log_path()
                } else {
                    v.source.log_path()
                };
                if let Some(dir) = path.parent() {
                    open_folder(dir);
                }
            });
        }
        {
            let v = viewer.clone();
            crash_log_btn.connect_clicked(move |_| v.toggle_crash_log());
        }
        {
            let w = window.clone();
            close.connect_clicked(move |_| w.close());
        }
        viewer.refresh();
        viewer
    }

    /// `refresh()`: re-reads the source and scrolls to the end, so the latest entries show.
    pub fn refresh(&self) {
        let (path, body) = view_text(&*self.source, self.showing_crash.get());
        self.path_label.set_text(&path);
        let buffer = self.text.buffer();
        buffer.set_text(&body);
        let end = buffer.end_iter();
        buffer.place_cursor(&end);
        let mark = buffer.create_mark(None, &end, false);
        self.text.scroll_to_mark(&mark, 0.0, false, 0.0, 0.0);
    }

    /// `_toggle_crash_log()`.
    pub fn toggle_crash_log(&self) {
        let now = !self.showing_crash.get();
        self.showing_crash.set(now);
        self.crash_log_btn.set_label(if now {
            "View App Log"
        } else {
            "View Crash Log"
        });
        self.refresh();
    }

    pub fn showing_crash_log(&self) -> bool {
        self.showing_crash.get()
    }

    pub fn body(&self) -> String {
        let b = self.text.buffer();
        b.text(&b.start_iter(), &b.end_iter(), false).to_string()
    }

    /// `_copy_all()`.
    pub fn copy_all(&self) {
        self.window.clipboard().set_text(&self.body());
    }

    pub fn present(&self) {
        self.window.present();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed {
        log: String,
        crash: String,
    }

    impl LogSource for Fixed {
        fn log_path(&self) -> PathBuf {
            "/p/hatate-rs.log".into()
        }
        fn recent_log(&self) -> String {
            self.log.clone()
        }
        fn crash_log_path(&self) -> PathBuf {
            "/p/crash.log".into()
        }
        fn recent_crash_log(&self) -> String {
            self.crash.clone()
        }
    }

    #[test]
    fn an_empty_log_says_so_in_pythons_words() {
        let s = Fixed {
            log: String::new(),
            crash: String::new(),
        };
        assert_eq!(
            view_text(&s, false),
            (
                "Log file: /p/hatate-rs.log".into(),
                "(no log entries yet)".into()
            )
        );
        assert_eq!(
            view_text(&s, true),
            (
                "Crash log: /p/crash.log".into(),
                "(no crash log yet - nothing has crashed)".into()
            )
        );
    }

    #[test]
    fn a_log_shows_its_text() {
        let s = Fixed {
            log: "a\nb".into(),
            crash: "boom".into(),
        };
        assert_eq!(view_text(&s, false).1, "a\nb");
        assert_eq!(view_text(&s, true).1, "boom");
    }

    #[test]
    fn the_file_source_reads_the_tails() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let paths = Paths::from_lookup(move |k| match k {
            "HOME" => Some(root.clone().into()),
            "XDG_CONFIG_HOME" => Some(root.join("c").into()),
            "XDG_CACHE_HOME" => Some(root.join("k").into()),
            _ => None,
        })
        .unwrap();
        std::fs::create_dir_all(paths.config_dir()).unwrap();
        let src = FileLogs::new(&paths);
        assert_eq!(src.recent_log(), "");
        assert_eq!(src.recent_crash_log(), "");
        std::fs::write(paths.crash_log(), "x".repeat(CRASH_LOG_TAIL_CHARS + 5)).unwrap();
        assert_eq!(src.recent_crash_log().chars().count(), CRASH_LOG_TAIL_CHARS);
        std::fs::write(paths.rust_log_file(), "line\n").unwrap();
        assert_eq!(src.recent_log(), "line\n");
    }
}
