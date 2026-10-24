//! The queue model and the single-owner channel in front of it (DAN-2167).
//!
//! Python's MCP tools run on the server's own thread and reach the list of images through
//! `MainThreadInvoker`: a callable is marshaled onto the Qt thread, which owns `window.entries`,
//! and the calling thread blocks until the result comes back or a timeout passes
//! (`gui/mcp_bridge.py`). Here the window's list becomes [`QueueModel`], owned by exactly one
//! thread through a [`QueueOwner`], and [`QueueChannel`] is the invoker: it sends a closure to the
//! owner and waits for its answer with Python's timeouts and the same timeout text.
//!
//! The model holds what the tools touch and nothing about how it is drawn: the entries in queue
//! order, the settings the window shares with the server, which row is current, and whether a
//! search is running. What the window does after a change (`_refresh_entry_row`,
//! `_refresh_table`, `_on_selection_changed`) is a [`QueueEvent`] the owner drains and renders.
//! Phase 6's GUI is the owner; [`spawn_owner_thread`] is a plain owner for tests and for a
//! headless host.
//!
//! Rows are addressed by position in the full queue, never by what a filter shows (the tools'
//! rule), and tracked across removals by [`EntryId`], the stand-in for Python's object identity.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use tracing::info;

use crate::config::Settings;
use crate::error::ErrorKind;
use crate::model::{Entry, MatchStatus, Tag, TagSource};

/// `DEFAULT_TIMEOUT_SECONDS`: how long a marshaled call may take before the invoker gives up.
pub const DEFAULT_TIMEOUT_SECONDS: f64 = 20.0;
/// `HYDRUS_SEND_TIMEOUT_SECONDS`.
pub const HYDRUS_SEND_TIMEOUT_SECONDS: f64 = 60.0;
/// `RESEARCH_TIMEOUT_PER_IMAGE_SECONDS`.
pub const RESEARCH_TIMEOUT_PER_IMAGE_SECONDS: f64 = 90.0;
/// `RESEARCH_TIMEOUT_FLOOR_SECONDS`.
pub const RESEARCH_TIMEOUT_FLOOR_SECONDS: f64 = 60.0;

/// A row's identity across removals and inserts: Python compares entries with `id(entry)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(pub u64);

/// What the window would redraw after a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueEvent {
    /// `_refresh_entry_row(entry)`.
    RowChanged(EntryId),
    /// `_refresh_table()`.
    TableRefreshed,
    /// Rows left the list (`_remove_rows` / `_remove_entries`).
    Removed(Vec<EntryId>),
    /// `_on_selection_changed()`: the preview follows whichever row is current.
    SelectionChanged,
    /// A search started or ended.
    SearchRunning(bool),
}

/// Why `_remove_entries` is removing (it only reaches the log).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveReason {
    /// `_remove_rows`: the user, or a tool, removed the row.
    Requested,
    /// `reason="sent-to-Hydrus"` and the other post-send reasons, as text.
    After(&'static str),
}

/// The list of images and the little state the tools read and change.
pub struct QueueModel {
    entries: Vec<Entry>,
    ids: Vec<EntryId>,
    next_id: u64,
    current: Option<EntryId>,
    search_running: bool,
    settings: Arc<Mutex<Settings>>,
    events: Vec<QueueEvent>,
}

impl QueueModel {
    /// A model over `entries` (in queue order) and the settings the window shares.
    pub fn new(entries: Vec<Entry>, settings: Arc<Mutex<Settings>>) -> QueueModel {
        let ids = (0..entries.len() as u64).map(EntryId).collect();
        QueueModel {
            next_id: entries.len() as u64,
            entries,
            ids,
            current: None,
            search_running: false,
            settings,
            events: Vec::new(),
        }
    }

    /// `window.entries`.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// `len(window.entries)`.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `window.settings`: the one object the GUI edits and the tools read on every call.
    pub fn settings(&self) -> &Arc<Mutex<Settings>> {
        &self.settings
    }

    /// Adds an entry at the end and returns its id.
    pub fn push(&mut self, entry: Entry) -> EntryId {
        let id = EntryId(self.next_id);
        self.next_id += 1;
        self.entries.push(entry);
        self.ids.push(id);
        self.events.push(QueueEvent::TableRefreshed);
        id
    }

    /// `_entry_at(row)`: a row by its index into the FULL queue, `None` outside it.
    pub fn entry_at(&self, row: i64) -> Option<&Entry> {
        usize::try_from(row).ok().and_then(|r| self.entries.get(r))
    }

    /// The id of the entry at `row`.
    pub fn id_at(&self, row: i64) -> Option<EntryId> {
        usize::try_from(row)
            .ok()
            .and_then(|r| self.ids.get(r).copied())
    }

    /// The position of an entry still in the list.
    pub fn position_of(&self, id: EntryId) -> Option<usize> {
        self.ids.iter().position(|i| *i == id)
    }

    pub fn entry(&self, id: EntryId) -> Option<&Entry> {
        self.position_of(id).map(|p| &self.entries[p])
    }

    pub fn entry_mut(&mut self, id: EntryId) -> Option<&mut Entry> {
        self.position_of(id).map(|p| &mut self.entries[p])
    }

    /// `window._current_entry()`.
    pub fn current(&self) -> Option<EntryId> {
        self.current
    }

    /// The GUI's selection moved.
    pub fn set_current(&mut self, id: Option<EntryId>) {
        self.current = id.filter(|i| self.position_of(*i).is_some());
    }

    /// `window.worker is not None and window.worker.isRunning()`.
    pub fn search_running(&self) -> bool {
        self.search_running
    }

    pub fn set_search_running(&mut self, running: bool) {
        if self.search_running != running {
            self.search_running = running;
            self.events.push(QueueEvent::SearchRunning(running));
        }
    }

    /// What changed since the last call.
    pub fn take_events(&mut self) -> Vec<QueueEvent> {
        std::mem::take(&mut self.events)
    }

    /// `_refresh_entry_row(entry)`.
    pub fn refresh_row(&mut self, id: EntryId) {
        self.events.push(QueueEvent::RowChanged(id));
    }

    /// `_refresh_table()`.
    pub fn refresh_table(&mut self) {
        self.events.push(QueueEvent::TableRefreshed);
    }

    /// `_on_selection_changed()`.
    pub fn selection_changed(&mut self) {
        self.events.push(QueueEvent::SelectionChanged);
    }

    /// The `select_candidate` tool's body after the range check: switch the active match, then
    /// redraw the row, and the preview when the row is the current one.
    pub fn select_candidate(&mut self, id: EntryId, index: usize) {
        if let Some(entry) = self.entry_mut(id) {
            entry.select_candidate(index);
        }
        self.refresh_row(id);
        if self.current == Some(id) {
            self.selection_changed();
        }
    }

    /// `_set_rows_reviewed([entry], reviewed)`: only a row whose flag changes is touched.
    pub fn set_reviewed(&mut self, id: EntryId, reviewed: bool) {
        let remaining;
        match self.entry_mut(id) {
            Some(entry) if entry.reviewed != reviewed => entry.reviewed = reviewed,
            _ => return,
        }
        self.refresh_row(id);
        remaining = self.entries.iter().filter(|e| needs_review(e)).count();
        info!(
            "{} 1 image(s) as reviewed ({remaining} still to review)",
            if reviewed { "Marked" } else { "Unmarked" }
        );
    }

    /// `_remove_rows` / `_remove_entries`: drops the entries by identity. When the current row is
    /// among them, the current row moves to the next survivor down, else the nearest above.
    pub fn remove(&mut self, ids: &[EntryId], reason: RemoveReason) {
        let doomed: Vec<EntryId> = ids
            .iter()
            .copied()
            .filter(|i| self.position_of(*i).is_some())
            .collect();
        if doomed.is_empty() {
            return;
        }
        let successor = self.successor_surviving(&doomed);
        let names: Vec<String> = doomed
            .iter()
            .filter_map(|i| self.entry(*i).map(|e| e.filename().to_owned()))
            .collect();
        let keep: Vec<bool> = self.ids.iter().map(|i| !doomed.contains(i)).collect();
        let mut flags = keep.iter();
        self.entries
            .retain(|_| *flags.next().expect("one flag per entry"));
        self.ids.retain(|i| !doomed.contains(i));
        match reason {
            RemoveReason::Requested => {
                info!("Removed {} file(s) from the list: {names:?}", doomed.len())
            }
            RemoveReason::After(why) => {
                info!("Removed {} {why} file(s) from the list", doomed.len())
            }
        }
        if self.current.is_some_and(|c| doomed.contains(&c)) {
            self.current = successor;
        }
        self.events.push(QueueEvent::Removed(doomed));
        self.events.push(QueueEvent::TableRefreshed);
        self.selection_changed();
    }

    /// `_successor_surviving`: where the selection lands if it is about to be removed.
    fn successor_surviving(&self, doomed: &[EntryId]) -> Option<EntryId> {
        let current = self.current?;
        if !doomed.contains(&current) {
            return None;
        }
        let at = self.position_of(current)?;
        (at + 1..self.ids.len())
            .chain((0..at).rev())
            .map(|p| self.ids[p])
            .find(|i| !doomed.contains(i))
    }

    /// `ImageEntry.reset_result` for the entry: back to never-searched, keeping user and Hydrus
    /// tags, the sent flags, the hash and the local dimensions. Returns the hash the search
    /// cache is keyed on, or `None` when there was nothing to reset.
    pub fn reset_result(&mut self, id: EntryId) -> Option<Option<String>> {
        let entry = self.entry_mut(id)?;
        if !has_result(entry) {
            return None;
        }
        reset_entry(entry);
        let hash = entry.hydrus_hash.clone();
        self.refresh_table();
        self.selection_changed();
        Some(hash)
    }

    /// Takes back a finished search's results: each entry that is still in the list is replaced
    /// by the searched copy; the rows `remove_after_import` dropped leave the list.
    pub fn apply_search(&mut self, results: Vec<(EntryId, Entry)>, removed: &[EntryId]) {
        for (id, entry) in results {
            if let Some(slot) = self.entry_mut(id) {
                *slot = entry;
                self.refresh_row(id);
            }
        }
        if !removed.is_empty() {
            self.remove(
                removed,
                RemoveReason::After("downloaded-and-sent-to-Hydrus"),
            );
        }
        self.set_search_running(false);
    }
}

/// `ImageEntry.needs_review`: not sent and not reviewed.
pub fn needs_review(entry: &Entry) -> bool {
    !entry.sent_to_hydrus && !entry.reviewed
}

/// `ImageEntry.has_result`: whether a search has produced anything.
pub fn has_result(entry: &Entry) -> bool {
    entry.status != MatchStatus::NotSearched
        || !entry.candidates.is_empty()
        || entry.last_searched.is_some()
        || entry.result_source.is_some()
}

/// `ImageEntry.reset_result`.
pub fn reset_entry(entry: &mut Entry) {
    entry.candidates = Vec::new();
    entry.selected_candidate_index = 0;
    entry.stale_matched_url = None;
    entry.stale_similarity = None;
    entry.stale_similarity_measured = false;
    entry.stale_source_name = None;
    entry.error_message = None;
    entry.last_searched = None;
    entry.result_source = None;
    entry.searched_without_saucenao = false;
    entry.upscale_verdict = None;
    entry.upscale_check_detail = None;
    entry.reviewed = false;
    entry.status = MatchStatus::NotSearched;
    let keep = |t: &Tag| matches!(t.source, TagSource::User | TagSource::Hydrus);
    entry.tags.retain(keep);
}

// ---------------------------------------------------------------------------------------------
// The channel
// ---------------------------------------------------------------------------------------------

type Job = Box<dyn FnOnce(&mut QueueModel) + Send>;

/// `MainThreadTimeout`: the owner did not run the marshaled call within its timeout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainThreadTimeout(pub String);

impl std::fmt::Display for MainThreadTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a marshaled call did not return a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvokeError {
    /// The owner did not run the call in time (or is gone).
    Timeout(MainThreadTimeout),
    /// The call ran and failed: Python's `RuntimeError(str(exc))`, or the executor's refusal once
    /// `shutdown` has run.
    Failed(String),
}

impl InvokeError {
    /// `str(exc)`.
    pub fn text(&self) -> String {
        match self {
            InvokeError::Timeout(t) => t.0.clone(),
            InvokeError::Failed(m) => m.clone(),
        }
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            InvokeError::Timeout(_) => ErrorKind::McpQueueTimeout,
            InvokeError::Failed(_) => ErrorKind::McpQueueFailed,
        }
    }

    /// What failed and what to do. No path, URL, token or OS text.
    pub fn user_message(&self) -> String {
        match self {
            InvokeError::Timeout(_) => {
                "Hatate's window did not answer in time, so the request was dropped. Try it again."
                    .to_owned()
            }
            InvokeError::Failed(_) => {
                "Hatate could not complete the request against its list. Try it again.".to_owned()
            }
        }
    }

    /// The raw text for a log: Python's message.
    pub fn detail(&self) -> String {
        self.text()
    }
}

impl std::fmt::Display for InvokeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text())
    }
}

impl std::error::Error for InvokeError {}

/// `MainThreadInvoker`: runs a closure against the model on its owner's thread and returns the
/// result to whichever thread asked. Cloneable; every clone talks to the same owner.
#[derive(Clone)]
pub struct QueueChannel {
    tx: Sender<Job>,
    shut_down: Arc<AtomicBool>,
}

impl QueueChannel {
    /// `invoker.call(fn, timeout)`. The closure keeps its place in the owner's queue after a
    /// timeout and runs when the owner gets to it, as Python's blocked emit does; only the wait
    /// is abandoned. An error returned by the closure is `RuntimeError(str(exc))`.
    ///
    /// Not safe to call from the owner's own thread (it would wait for itself): Python runs the
    /// callable directly there, a path no tool takes and this channel has no model to offer.
    pub fn call<T, F>(&self, f: F, timeout: f64) -> Result<T, InvokeError>
    where
        T: Send + 'static,
        F: FnOnce(&mut QueueModel) -> Result<T, String> + Send + 'static,
    {
        if self.shut_down.load(Ordering::SeqCst) {
            return Err(InvokeError::Failed(
                "cannot schedule new futures after shutdown".to_owned(),
            ));
        }
        let (reply_tx, reply_rx) = mpsc::sync_channel::<Result<T, String>>(1);
        let job: Job = Box::new(move |model| {
            let outcome = catch_unwind(AssertUnwindSafe(|| f(model)))
                .unwrap_or_else(|p| Err(panic_text(&*p)));
            let _ = reply_tx.send(outcome);
        });
        let timed_out = || {
            InvokeError::Timeout(MainThreadTimeout(format!(
                "Timed out after {timeout:.0}s waiting for the GUI thread"
            )))
        };
        if self.tx.send(job).is_err() {
            return Err(timed_out());
        }
        match reply_rx.recv_timeout(crate::remote::secs(timeout)) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(message)) => Err(InvokeError::Failed(message)),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => Err(timed_out()),
        }
    }

    /// Queues a closure and does not wait for it (a worker reporting back).
    pub fn post<F>(&self, f: F)
    where
        F: FnOnce(&mut QueueModel) + Send + 'static,
    {
        let _ = self.tx.send(Box::new(f));
    }

    /// `invoker.shutdown()`: no new calls after this.
    pub fn shutdown(&self) {
        self.shut_down.store(true, Ordering::SeqCst);
    }
}

fn panic_text(p: &(dyn Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".to_owned())
}

/// The owning end: holds the model and runs what the channel sends.
pub struct QueueOwner {
    rx: Receiver<Job>,
    model: QueueModel,
}

impl QueueOwner {
    /// The owner of `model` and the channel to it.
    pub fn new(model: QueueModel) -> (QueueOwner, QueueChannel) {
        let (tx, rx) = mpsc::channel();
        (
            QueueOwner { rx, model },
            QueueChannel {
                tx,
                shut_down: Arc::new(AtomicBool::new(false)),
            },
        )
    }

    pub fn model(&self) -> &QueueModel {
        &self.model
    }

    pub fn model_mut(&mut self) -> &mut QueueModel {
        &mut self.model
    }

    /// Runs every job already queued, in order; returns how many ran.
    pub fn pump(&mut self) -> usize {
        let mut ran = 0;
        while let Ok(job) = self.rx.try_recv() {
            job(&mut self.model);
            ran += 1;
        }
        ran
    }

    /// Waits up to `wait` for one job, runs it and whatever queued behind it.
    pub fn pump_wait(&mut self, wait: Duration) -> usize {
        match self.rx.recv_timeout(wait) {
            Ok(job) => {
                job(&mut self.model);
                1 + self.pump()
            }
            Err(_) => 0,
        }
    }

    pub fn into_model(self) -> QueueModel {
        self.model
    }
}

/// A thread that owns a model and serves its channel until [`OwnerThread::stop`].
pub struct OwnerThread {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<QueueModel>>,
}

/// Starts a plain owner: for tests, and for a host with no window.
pub fn spawn_owner_thread(model: QueueModel) -> (OwnerThread, QueueChannel) {
    let (mut owner, channel) = QueueOwner::new(model);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let handle = std::thread::Builder::new()
        .name("queue-owner".to_owned())
        .spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                owner.pump_wait(Duration::from_millis(5));
            }
            owner.pump();
            owner.into_model()
        })
        .expect("spawn the queue owner thread");
    (
        OwnerThread {
            stop,
            handle: Some(handle),
        },
        channel,
    )
}

impl OwnerThread {
    /// Stops the thread after it has run what was queued, and hands the model back.
    pub fn stop(mut self) -> QueueModel {
        self.stop.store(true, Ordering::SeqCst);
        self.handle
            .take()
            .expect("owner thread handle")
            .join()
            .expect("the queue owner thread panicked")
    }
}

// ---------------------------------------------------------------------------------------------
// Searches started from the queue
// ---------------------------------------------------------------------------------------------

/// Set once a launched search has ended (`finished_all`): the tool waits on it.
#[derive(Default)]
pub struct DoneFlag {
    done: Mutex<bool>,
    cv: std::sync::Condvar,
}

impl DoneFlag {
    pub fn set(&self) {
        *self.done.lock().expect("done flag") = true;
        self.cv.notify_all();
    }

    /// `event.wait(timeout)`: true when the flag was set within the timeout.
    pub fn wait(&self, timeout: f64) -> bool {
        let guard = self.done.lock().expect("done flag");
        let (guard, _) = self
            .cv
            .wait_timeout_while(guard, crate::remote::secs(timeout), |done| !*done)
            .expect("done flag");
        *guard
    }
}

/// What a launcher is given to report a finished search.
pub struct SearchDone {
    channel: QueueChannel,
    flag: Arc<DoneFlag>,
}

impl SearchDone {
    pub(crate) fn new(channel: QueueChannel, flag: Arc<DoneFlag>) -> SearchDone {
        SearchDone { channel, flag }
    }

    /// The search ended: hand the searched copies back to the model (rows `removed` left the
    /// list), then release the tool that is waiting. The model is updated first.
    pub fn finish(self, results: Vec<(EntryId, Entry)>, removed: Vec<EntryId>) {
        self.channel
            .post(move |model| model.apply_search(results, &removed));
        self.flag.set();
    }
}

/// `window._research_rows(entries)` / `_launch_search_worker(..., bypass_cache=True)`: starts a
/// search over copies of the entries. The search runs on the launcher's own thread; the model
/// is untouched until [`SearchDone::finish`].
pub trait SearchLauncher: Send + Sync {
    /// `Err` is a search that did not start (Python: `worker is None or not isRunning()`).
    fn launch(
        &self,
        batch: Vec<(EntryId, Entry)>,
        pending: Vec<usize>,
        done: SearchDone,
    ) -> Result<(), String>;
}

/// The part of `_research_rows` that runs on the owner: refuse while a search runs, start one
/// otherwise. Returns the flag to wait on.
pub fn start_research(
    model: &mut QueueModel,
    channel: &QueueChannel,
    launcher: &dyn SearchLauncher,
    rows: &[i64],
) -> ResearchStart {
    if model.search_running() {
        return ResearchStart::Declined("A search is already running".to_owned());
    }
    // Rows that exist, in the order asked (a row named twice is searched twice, as the worker
    // was handed the same entry twice).
    let mut batch: Vec<(EntryId, Entry)> = Vec::new();
    let mut pending: Vec<usize> = Vec::new();
    let mut filenames: Vec<String> = Vec::new();
    for row in rows {
        let Some(id) = model.id_at(*row) else {
            continue;
        };
        let entry = model.entry(id).expect("id_at gives a live id");
        filenames.push(entry.filename().to_owned());
        match batch.iter().position(|(i, _)| *i == id) {
            Some(p) => pending.push(p),
            None => {
                pending.push(batch.len());
                batch.push((id, entry.clone()));
            }
        }
    }
    if filenames.is_empty() {
        return ResearchStart::Declined("No valid rows to search".to_owned());
    }
    info!(
        "Re-search requested for {} image(s): {filenames:?}",
        filenames.len()
    );
    let flag = Arc::new(DoneFlag::default());
    model.set_search_running(true);
    let done = SearchDone::new(channel.clone(), flag.clone());
    if launcher.launch(batch, pending, done).is_err() {
        model.set_search_running(false);
        return ResearchStart::Declined("Search did not start".to_owned());
    }
    ResearchStart::Started { filenames, flag }
}

/// How a research request began.
pub enum ResearchStart {
    Started {
        filenames: Vec<String>,
        flag: Arc<DoneFlag>,
    },
    Declined(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(n: usize) -> QueueModel {
        let entries = (0..n).map(|i| Entry::new(format!("/q/{i}.png"))).collect();
        let home: std::ffi::OsString = "/tmp/hatate-dan2167-unit".into();
        let paths = crate::paths::Paths::from_lookup(|k| (k == "HOME").then(|| home.clone()))
            .expect("paths from a HOME");
        QueueModel::new(
            entries,
            Arc::new(Mutex::new(Settings::with_defaults(&paths))),
        )
    }

    #[test]
    fn rows_are_positions_in_the_full_queue_and_negative_rows_are_nothing() {
        let m = model(3);
        assert_eq!(m.entry_at(2).unwrap().filename(), "2.png");
        assert!(m.entry_at(3).is_none());
        assert!(m.entry_at(-1).is_none());
    }

    #[test]
    fn removing_the_current_row_lands_on_the_next_survivor_then_the_one_above() {
        let mut m = model(4);
        let ids: Vec<EntryId> = (0..4).map(|r| m.id_at(r).unwrap()).collect();
        m.set_current(Some(ids[1]));
        m.remove(&[ids[1], ids[2]], RemoveReason::Requested);
        assert_eq!(m.current(), Some(ids[3]));
        m.remove(&[ids[3]], RemoveReason::Requested);
        assert_eq!(m.current(), Some(ids[0]));
        m.remove(&[ids[0]], RemoveReason::Requested);
        assert_eq!(m.current(), None);
    }

    #[test]
    fn a_call_reaches_the_owner_and_a_failure_is_the_message() {
        let (owner, ch) = spawn_owner_thread(model(2));
        let n = ch.call(|m| Ok(m.len()), 5.0).unwrap();
        assert_eq!(n, 2);
        let err = ch
            .call::<(), _>(|_| Err("bad".to_owned()), 5.0)
            .unwrap_err();
        assert_eq!(err, InvokeError::Failed("bad".to_owned()));
        let err = ch.call::<(), _>(|_| panic!("exploded"), 5.0).unwrap_err();
        assert_eq!(err.text(), "exploded");
        drop(owner.stop());
    }

    #[test]
    fn an_owner_that_never_answers_gives_pythons_timeout_text() {
        let (owner, ch) = QueueOwner::new(model(1));
        let err = ch.call(|m| Ok(m.len()), 0.05).unwrap_err();
        assert_eq!(
            err,
            InvokeError::Timeout(MainThreadTimeout(
                "Timed out after 0s waiting for the GUI thread".to_owned()
            ))
        );
        // The abandoned call still runs when the owner gets to it, as Python's blocked emit does.
        let mut owner = owner;
        assert_eq!(owner.pump(), 1);
    }

    #[test]
    fn a_shut_down_channel_refuses_new_calls() {
        let (owner, ch) = spawn_owner_thread(model(1));
        ch.shutdown();
        let err = ch.call(|m| Ok(m.len()), 5.0).unwrap_err();
        assert_eq!(
            err,
            InvokeError::Failed("cannot schedule new futures after shutdown".to_owned())
        );
        drop(owner.stop());
    }

    #[test]
    fn reset_keeps_user_and_hydrus_tags_and_the_sent_flags() {
        let mut m = model(1);
        let id = m.id_at(0).unwrap();
        {
            let e = m.entry_mut(id).unwrap();
            e.status = MatchStatus::Good;
            e.reviewed = true;
            e.sent_to_hydrus = true;
            e.hydrus_hash = Some("ab".repeat(32));
            e.tags = vec![
                Tag {
                    name: "mine".into(),
                    namespace: None,
                    source: TagSource::User,
                },
                Tag {
                    name: "theirs".into(),
                    namespace: None,
                    source: TagSource::Booru,
                },
            ];
        }
        let hash = m.reset_result(id).unwrap();
        assert_eq!(hash.as_deref(), Some("ab".repeat(32).as_str()));
        let e = m.entry(id).unwrap();
        assert_eq!(e.status, MatchStatus::NotSearched);
        assert!(!e.reviewed && e.sent_to_hydrus);
        assert_eq!(e.tags.len(), 1);
        assert!(m.reset_result(id).is_none(), "nothing left to reset");
    }

    #[test]
    fn the_error_shape_carries_no_path_or_os_text() {
        let e = InvokeError::Failed("/home/me/.config/hatate/session.json: os error 2".to_owned());
        assert!(!e.user_message().contains("/home") && !e.user_message().contains("os error"));
        assert!(e.detail().contains("os error 2"));
        assert_eq!(e.kind(), ErrorKind::McpQueueFailed);
    }
}
