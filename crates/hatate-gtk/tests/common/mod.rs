//! Shared test scaffolding: a scripted prompter that records what the window asked and answers
//! from a queue, and a fixture that builds the real window over a temp profile.
//!
//! Everything here runs headless (Xvfb) with a temp `HOME` and temp XDG directories. Nothing
//! reads the board's profile, session, Hydrus or browser.

#![allow(dead_code)]

pub mod hydrus;
pub mod mcp;
pub mod search;
pub mod xtest;

use gtk4::glib;
use gtk4::prelude::*;
use hatate_core::config::Settings;
use hatate_core::model::{Entry, MatchStatus};
use hatate_core::paths::Paths;
use hatate_core::session::SessionStore;
use hatate_gtk::dialogs::{
    Asked, Buttons, Done, Kind, Prompter, Response, Shown, default_response,
};
use hatate_gtk::window::{Env, MainWindow, MissingFiles};
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// What the next prompt answers.
#[derive(Clone, Debug)]
pub enum Answer {
    Msg(Response),
    /// A Save dialog's answer: the path and the filter the user chose.
    Save(Option<(PathBuf, String)>),
    Files(Vec<PathBuf>),
    Path(Option<PathBuf>),
    /// The Add tags box: the text typed, or `None` for Cancel.
    Text(Option<String>),
    /// The Edit match conditions box: the values on OK, or `None` for Cancel.
    Conditions(Option<hatate_core::config::MatchConditions>),
}

/// A prompter that answers at once from a script and keeps a record of every question.
#[derive(Default)]
pub struct Script {
    /// Message-box answers and chooser answers are separate queues, as in the Python oracle's
    /// driver: which box comes first inside one action is then not part of the script.
    answers: RefCell<VecDeque<Answer>>,
    picks: RefCell<VecDeque<Answer>>,
    pub shown: RefCell<Vec<Shown>>,
    pub asked: RefCell<Vec<Asked>>,
    /// The button pressed for each entry of `shown`.
    pub results: RefCell<Vec<Response>>,
}

impl Script {
    pub fn push(&self, a: Answer) {
        match a {
            Answer::Msg(_) => self.answers.borrow_mut().push_back(a),
            _ => self.picks.borrow_mut().push_back(a),
        }
    }

    /// Forgets every unused answer and every record.
    pub fn reset(&self) {
        self.answers.borrow_mut().clear();
        self.picks.borrow_mut().clear();
        self.shown.borrow_mut().clear();
        self.asked.borrow_mut().clear();
        self.results.borrow_mut().clear();
    }

    /// The next scripted message-box answer. Taken before it is acted on: the continuation may
    /// ask again.
    fn next(&self) -> Option<Answer> {
        self.answers.borrow_mut().pop_front()
    }

    /// The next scripted chooser answer.
    fn next_pick(&self) -> Option<Answer> {
        self.picks.borrow_mut().pop_front()
    }

    pub fn last_shown(&self) -> Shown {
        self.shown
            .borrow()
            .last()
            .cloned()
            .expect("no dialog was shown")
    }

    pub fn count_shown(&self) -> usize {
        self.shown.borrow().len()
    }
}

impl Prompter for Script {
    fn message(&self, kind: Kind, title: &str, text: &str, buttons: Buttons, done: Done<Response>) {
        self.message_detailed(kind, title, text, "", buttons, done);
    }

    fn message_detailed(
        &self,
        kind: Kind,
        title: &str,
        text: &str,
        detail: &str,
        buttons: Buttons,
        done: Done<Response>,
    ) {
        let default = default_response(kind, buttons);
        self.shown.borrow_mut().push(Shown {
            kind,
            title: title.to_owned(),
            text: text.to_owned(),
            buttons,
            default,
            detail: detail.to_owned(),
        });
        let answer = match self.next() {
            // An answer this box has no button for is the one a click elsewhere would give:
            // the Python driver treats any word that is not "yes" as No, and so on.
            Some(Answer::Msg(r)) => match (buttons, r) {
                (Buttons::YesNo, Response::Yes) => Response::Yes,
                (Buttons::YesNo, _) => Response::No,
                (Buttons::KeepReplaceCancel, Response::Keep | Response::Replace) => r,
                (Buttons::KeepReplaceCancel, _) => Response::Cancel,
                (Buttons::RemoveCancel, Response::Remove) => r,
                (Buttons::RemoveCancel, _) => Response::Cancel,
                (Buttons::DeleteCancel, Response::Delete) => r,
                (Buttons::DeleteCancel, _) => Response::Cancel,
                (Buttons::Ok | Buttons::GotIt, _) => Response::Ok,
            },
            // An unscripted box is answered by its default button, as pressing Enter would.
            _ => default,
        };
        self.results.borrow_mut().push(answer);
        done(answer);
    }

    fn save_file_with_filter(
        &self,
        title: &str,
        default_name: &str,
        filter: &str,
        done: Done<Option<(PathBuf, String)>>,
    ) {
        self.asked.borrow_mut().push(Asked::SaveFile {
            title: title.to_owned(),
            default_name: default_name.to_owned(),
            filter: filter.to_owned(),
        });
        match self.next_pick() {
            Some(Answer::Save(p)) => done(p),
            Some(Answer::Path(p)) => done(p.map(|p| (p, String::new()))),
            _ => done(None),
        }
    }

    fn add_tags(&self, prompt: &str, done: Done<Option<String>>) {
        self.asked.borrow_mut().push(Asked::AddTags {
            prompt: prompt.to_owned(),
        });
        match self.next_pick() {
            Some(Answer::Text(t)) => done(t),
            _ => done(None),
        }
    }

    fn match_conditions(
        &self,
        current: &hatate_core::config::MatchConditions,
        done: Done<Option<hatate_core::config::MatchConditions>>,
    ) {
        self.asked.borrow_mut().push(Asked::MatchConditions {
            opened_on: format!(
                "{} {} {} {}",
                current.min_tags_for_good,
                current.min_width_gain_percent,
                current.min_height_gain_percent,
                current.require_larger_than_local
            ),
        });
        match self.next_pick() {
            Some(Answer::Conditions(c)) => done(c),
            _ => done(None),
        }
    }

    fn open_files(&self, title: &str, filter: &str, done: Done<Vec<PathBuf>>) {
        self.asked.borrow_mut().push(Asked::OpenFiles {
            title: title.to_owned(),
            filter: filter.to_owned(),
        });
        match self.next_pick() {
            Some(Answer::Files(f)) => done(f),
            _ => done(Vec::new()),
        }
    }

    fn open_folder(&self, title: &str, done: Done<Option<PathBuf>>) {
        self.asked.borrow_mut().push(Asked::Folder {
            title: title.to_owned(),
        });
        match self.next_pick() {
            Some(Answer::Path(p)) => done(p),
            _ => done(None),
        }
    }

    fn open_file(&self, title: &str, filter: &str, done: Done<Option<PathBuf>>) {
        self.asked.borrow_mut().push(Asked::OpenFile {
            title: title.to_owned(),
            filter: filter.to_owned(),
        });
        match self.next_pick() {
            Some(Answer::Path(p)) => done(p),
            _ => done(None),
        }
    }

    fn save_file(
        &self,
        title: &str,
        default_name: &str,
        filter: &str,
        done: Done<Option<PathBuf>>,
    ) {
        self.asked.borrow_mut().push(Asked::SaveFile {
            title: title.to_owned(),
            default_name: default_name.to_owned(),
            filter: filter.to_owned(),
        });
        match self.next_pick() {
            Some(Answer::Path(p)) => done(p),
            _ => done(None),
        }
    }
}

/// Changes the settings before the window reads them.
pub type Tweak = Box<dyn Fn(&mut Settings)>;
/// Prepares the profile before the window starts.
pub type Setup = Box<dyn Fn(&Paths)>;

/// How a fixture is built.
#[derive(Default)]
pub struct Opts {
    pub unclean: bool,
    /// Entries saved to `session.db` before the window starts.
    pub seed: Vec<Entry>,
    /// A `session.db` with no rows, so a session "exists but came back empty".
    pub empty_session_file: bool,
    pub tweak: Option<Tweak>,
    pub missing: Option<MissingFiles>,
    /// Runs after the profile directory exists and before the window starts.
    pub setup: Option<Setup>,
}

/// A real window over a temp profile.
pub struct Fx {
    pub dir: tempfile::TempDir,
    pub paths: Paths,
    pub script: Rc<Script>,
    pub clock: Rc<Cell<f64>>,
    pub opened: Rc<RefCell<Vec<PathBuf>>>,
    pub w: MainWindow,
    /// The scripted engines behind Start Search.
    pub search: search::Scripted,
}

pub fn temp_paths(root: &Path) -> Paths {
    let root = root.to_path_buf();
    Paths::from_lookup(move |k| -> Option<OsString> {
        match k {
            "HOME" => Some(root.clone().into()),
            "XDG_CONFIG_HOME" => Some(root.join("config").into()),
            "XDG_CACHE_HOME" => Some(root.join("cache").into()),
            _ => None,
        }
    })
    .expect("paths")
}

/// A fixed "now" the tests control.
pub const NOW: f64 = 1_792_000_000.0;

impl Fx {
    pub fn new(app: &adw::Application, opts: Opts) -> Fx {
        Fx::new_with(app, opts, |_| {})
    }

    /// As [`Fx::new`], with a last say over the environment the window is given.
    pub fn new_with(app: &adw::Application, opts: Opts, edit: impl FnOnce(&mut Env)) -> Fx {
        let dir = tempfile::tempdir().expect("temp profile");
        let paths = temp_paths(dir.path());
        std::fs::create_dir_all(paths.config_dir()).unwrap();
        let mut settings = Settings::with_defaults(&paths);
        if let Some(t) = &opts.tweak {
            t(&mut settings);
        }
        settings.save(&paths).expect("settings");
        if let Some(setup) = &opts.setup {
            setup(&paths);
        }
        if !opts.seed.is_empty() || opts.empty_session_file {
            let writer = SessionStore::new(&paths).open_writer().expect("writer");
            writer.save(&opts.seed, NOW - 60.0).expect("seed session");
        }
        let script = Rc::new(Script::default());
        let clock = Rc::new(Cell::new(NOW));
        let opened = Rc::new(RefCell::new(Vec::new()));
        let mut env = Env::new(paths.clone(), opts.unclean);
        env.prompter = Some(script.clone());
        {
            let clock = clock.clone();
            env.clock = Rc::new(move || clock.get());
        }
        {
            let opened = opened.clone();
            env.open_path = Rc::new(move |p| opened.borrow_mut().push(p.to_path_buf()));
        }
        if let Some(m) = opts.missing {
            env.missing_files = m;
        }
        let (backend, search) = search::Scripted::new(&paths);
        env.search = Some(backend);
        edit(&mut env);
        let w = MainWindow::new(app, env);
        w.win.present();
        Fx {
            dir,
            paths,
            script,
            clock,
            opened,
            w,
            search,
        }
    }

    pub fn say(&self) -> String {
        self.w.s.shell.status_text()
    }

    pub fn len(&self) -> usize {
        self.w.s.queue.borrow().len()
    }

    pub fn paths_in_list(&self) -> Vec<String> {
        self.w
            .s
            .queue
            .borrow()
            .entries()
            .iter()
            .map(|e| e.path.clone())
            .collect()
    }

    pub fn activate(&self, action: &str) {
        self.w.activate_action(action);
    }

    /// The session database's entry count on disk.
    pub fn saved_count(&self) -> usize {
        SessionStore::new(&self.paths).entry_count()
    }

    /// Closes the way the window manager would, and drops the window.
    pub fn close(self) {
        self.w.win.close();
        self.w.win.destroy();
    }
}

/// Runs the default main context until `cond` holds or `ms` pass. Returns whether it held.
pub fn pump_until(ms: u64, mut cond: impl FnMut() -> bool) -> bool {
    let ctx = glib::MainContext::default();
    let t = Instant::now();
    while !cond() {
        if t.elapsed() > Duration::from_millis(ms) {
            return false;
        }
        ctx.iteration(false);
        std::thread::sleep(Duration::from_millis(2));
    }
    true
}

/// Lets the main loop turn for `ms`.
pub fn pump_for(ms: u64) {
    let ctx = glib::MainContext::default();
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(ms) {
        ctx.iteration(false);
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// An entry as a session would hold it.
pub fn entry(path: &str, status: MatchStatus) -> Entry {
    let mut e = Entry::new(path);
    e.status = status;
    e.hydrus_hash = Some(format!("{:064x}", path.len()));
    e
}

/// Writes `n` small distinct PNG-less files named so they count as images.
pub fn make_images(dir: &Path, names: &[&str]) -> Vec<PathBuf> {
    std::fs::create_dir_all(dir).unwrap();
    names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let p = dir.join(n);
            std::fs::write(&p, format!("synthetic image bytes {i} {n}")).unwrap();
            p
        })
        .collect()
}
