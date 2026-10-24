//! Dialogs and file choosers (`gui/message.py` at `76c85c4`, and the `QFileDialog` calls of
//! `MainWindow`).
//!
//! Qt's `exec()` blocks in a nested loop and returns the button; GTK dialogs never block, so a
//! dialog here takes the continuation that runs with the answer. Everything the window shows or
//! asks goes through the [`Prompter`] it is given, which is how a test (and the action
//! differential) scripts the answers and records what was asked: the text, the buttons, the
//! default button and the result. The production prompter ([`AdwPrompter`]) is libadwaita's
//! `AlertDialog` and GTK's `FileDialog`.
//!
//! **Destructive confirmations default to Cancel** (design gate 2.6, a product call on DAN-2025):
//! on a [`Kind::Question`] with Yes/No the default and the Escape response are No, and a
//! destructive Yes is never the default. Python leaves the default to Qt, which makes it Yes; this
//! is the one place the dialog differs on purpose (`docs/gtk-shell.md`).

use gtk4 as gtk;
use gtk4::gio;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::path::PathBuf;

/// Which of Qt's four icons the box carried; it picks the dialog's heading weight and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Information,
    Warning,
    Critical,
    Question,
}

/// The standard buttons `message.py` uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Buttons {
    Ok,
    YesNo,
}

/// What the user pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    Ok,
    Yes,
    No,
}

/// One dialog as it was shown, for a test to read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub kind: Kind,
    pub title: String,
    pub text: String,
    pub buttons: Buttons,
    /// The default button (and, for a question, the one Escape means).
    pub default: Response,
}

/// What a file chooser was asked, for a test to read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    OpenFiles {
        title: String,
        filter: String,
    },
    Folder {
        title: String,
    },
    OpenFile {
        title: String,
        filter: String,
    },
    SaveFile {
        title: String,
        default_name: String,
        filter: String,
    },
}

pub type Done<T> = Box<dyn FnOnce(T)>;

/// Everything the window asks the user.
pub trait Prompter {
    /// A message box. `done` runs with the pressed button.
    fn message(&self, kind: Kind, title: &str, text: &str, buttons: Buttons, done: Done<Response>);
    /// `QFileDialog.getOpenFileNames`: none picked is an empty list.
    fn open_files(&self, title: &str, filter: &str, done: Done<Vec<PathBuf>>);
    /// `QFileDialog.getExistingDirectory`.
    fn open_folder(&self, title: &str, done: Done<Option<PathBuf>>);
    /// `QFileDialog.getOpenFileName`.
    fn open_file(&self, title: &str, filter: &str, done: Done<Option<PathBuf>>);
    /// `QFileDialog.getSaveFileName`.
    fn save_file(&self, title: &str, default_name: &str, filter: &str, done: Done<Option<PathBuf>>);
}

/// The default button of a message box: Cancel (No) on a question, the only button otherwise.
pub fn default_response(kind: Kind, buttons: Buttons) -> Response {
    match (kind, buttons) {
        (Kind::Question, Buttons::YesNo) => Response::No,
        (_, Buttons::YesNo) => Response::No,
        (_, Buttons::Ok) => Response::Ok,
    }
}

/// `message.information(parent, title, text)`.
pub fn information(p: &dyn Prompter, title: &str, text: &str) {
    p.message(
        Kind::Information,
        title,
        text,
        Buttons::Ok,
        Box::new(|_| {}),
    );
}

/// `message.warning(parent, title, text)`.
pub fn warning(p: &dyn Prompter, title: &str, text: &str) {
    p.message(Kind::Warning, title, text, Buttons::Ok, Box::new(|_| {}));
}

/// `message.critical(parent, title, text)`.
pub fn critical(p: &dyn Prompter, title: &str, text: &str) {
    p.message(Kind::Critical, title, text, Buttons::Ok, Box::new(|_| {}));
}

/// `message.question(parent, title, text)` with Yes/No; `done` runs with whether Yes was pressed.
pub fn question(p: &dyn Prompter, title: &str, text: &str, done: Done<bool>) {
    p.message(
        Kind::Question,
        title,
        text,
        Buttons::YesNo,
        Box::new(move |r| done(r == Response::Yes)),
    );
}

/// The production prompter: libadwaita alert dialogs and GTK file dialogs over `parent`.
pub struct AdwPrompter {
    parent: gtk::Window,
}

impl AdwPrompter {
    pub fn new(parent: &impl IsA<gtk::Window>) -> AdwPrompter {
        AdwPrompter {
            parent: parent.clone().upcast(),
        }
    }

    fn filters(spec: &str) -> gio::ListStore {
        // `FILE_DIALOG_FILTER` style: "Name (*.a *.b);;All files (*)".
        let store = gio::ListStore::new::<gtk::FileFilter>();
        for part in spec.split(";;").filter(|p| !p.is_empty()) {
            let filter = gtk::FileFilter::new();
            let name = part.split('(').next().unwrap_or(part).trim();
            filter.set_name(Some(if name.is_empty() { part } else { name }));
            let patterns = part
                .split_once('(')
                .and_then(|(_, rest)| rest.split_once(')'))
                .map_or("", |(inner, _)| inner);
            for pat in patterns.split_whitespace() {
                filter.add_pattern(pat);
            }
            store.append(&filter);
        }
        store
    }
}

impl Prompter for AdwPrompter {
    fn message(&self, kind: Kind, title: &str, text: &str, buttons: Buttons, done: Done<Response>) {
        let dialog = adw::AlertDialog::new(Some(title), Some(text));
        match buttons {
            Buttons::Ok => {
                dialog.add_responses(&[("ok", "OK")]);
                dialog.set_default_response(Some("ok"));
                dialog.set_close_response("ok");
            }
            Buttons::YesNo => {
                dialog.add_responses(&[("no", "No"), ("yes", "Yes")]);
                dialog.set_default_response(Some(match default_response(kind, buttons) {
                    Response::Yes => "yes",
                    _ => "no",
                }));
                dialog.set_close_response("no");
            }
        }
        // A keyboard-first app: the default button shows its focus ring the moment the dialog
        // opens (GTK would draw it only after a key press), so the answer Enter gives is visible
        // (design gate 2.6). The ring belongs to the root window, which an AdwDialog borrows.
        self.parent.set_focus_visible(true);
        dialog.choose(Some(&self.parent), gio::Cancellable::NONE, move |answer| {
            done(match answer.as_str() {
                "yes" => Response::Yes,
                "no" => Response::No,
                _ => Response::Ok,
            });
        });
    }

    fn open_files(&self, title: &str, filter: &str, done: Done<Vec<PathBuf>>) {
        let dlg = gtk::FileDialog::builder().title(title).build();
        dlg.set_filters(Some(&Self::filters(filter)));
        dlg.open_multiple(Some(&self.parent), gio::Cancellable::NONE, move |r| {
            let files = r
                .ok()
                .map(|list| {
                    (0..list.n_items())
                        .filter_map(|i| list.item(i).and_downcast::<gio::File>())
                        .filter_map(|f| f.path())
                        .collect()
                })
                .unwrap_or_default();
            done(files);
        });
    }

    fn open_folder(&self, title: &str, done: Done<Option<PathBuf>>) {
        let dlg = gtk::FileDialog::builder().title(title).build();
        dlg.select_folder(Some(&self.parent), gio::Cancellable::NONE, move |r| {
            done(r.ok().and_then(|f| f.path()));
        });
    }

    fn open_file(&self, title: &str, filter: &str, done: Done<Option<PathBuf>>) {
        let dlg = gtk::FileDialog::builder().title(title).build();
        dlg.set_filters(Some(&Self::filters(filter)));
        dlg.open(Some(&self.parent), gio::Cancellable::NONE, move |r| {
            done(r.ok().and_then(|f| f.path()));
        });
    }

    fn save_file(
        &self,
        title: &str,
        default_name: &str,
        filter: &str,
        done: Done<Option<PathBuf>>,
    ) {
        let dlg = gtk::FileDialog::builder()
            .title(title)
            .initial_name(default_name)
            .build();
        dlg.set_filters(Some(&Self::filters(filter)));
        dlg.save(Some(&self.parent), gio::Cancellable::NONE, move |r| {
            done(r.ok().and_then(|f| f.path()));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_destructive_question_defaults_to_no() {
        assert_eq!(
            default_response(Kind::Question, Buttons::YesNo),
            Response::No
        );
        assert_eq!(
            default_response(Kind::Warning, Buttons::YesNo),
            Response::No
        );
    }

    #[test]
    fn a_one_button_box_defaults_to_ok() {
        for kind in [Kind::Information, Kind::Warning, Kind::Critical] {
            assert_eq!(default_response(kind, Buttons::Ok), Response::Ok);
        }
    }
}
