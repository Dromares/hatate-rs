//! The window's side of the embedded MCP server (`gui/mcp_bridge.py` at `76c85c4`: the lifecycle
//! `MainWindow.__init__` and `closeEvent` give it, and `MainThreadInvoker` as the channel the
//! tools reach the list through).
//!
//! The server runs on its own thread and the queue belongs to the GTK thread. Python marshals each
//! tool call onto the Qt thread with a signal; here a call is a closure sent down a
//! [`QueueChannel`] and run against the window's [`QueueModel`] by [`QueueInbox::pump`] on the
//! main thread, woken by the channel itself so an idle window spends nothing. What the closure
//! changed comes back as [`QueueEvent`]s, which [`MainWindow::render_queue_events`] draws the way
//! the window's own actions do.
//!
//! The tools themselves are `hatate_core::mcp::tool_handlers::McpToolHandlers`, over the window's
//! settings, its search backend and its runtime. This module re-implements none of them.
//!
//! A re-search starts a search of the window's own: the launcher below only records the request,
//! because it is called from inside the pump with the model borrowed, and the window starts the
//! run once the pump has let go.

use crate::search_run::LaunchOptions;
use crate::window::MainWindow;
use gtk4::glib;
use hatate_core::config::McpSettings;
use hatate_core::mcp::handlers::McpHandlers;
use hatate_core::mcp::server::McpServerController;
use hatate_core::mcp::tool_handlers::{HandlerEnv, McpToolHandlers, Timeouts};
use hatate_core::model::Entry;
use hatate_core::queue::{
    EntryId, QueueChannel, QueueEvent, QueueInbox, SearchDone, SearchLauncher,
};
use std::cell::{Cell, RefCell};
use std::sync::{Arc, Mutex};

use crate::mcp_audit_view::ServerReport;

/// What the card says when the server could not even be built (the window has no search backend).
const NOT_BUILT_USER_MESSAGE: &str =
    "The MCP server could not start. Try again, or restart Hatate.";

/// A re-search a tool asked for, waiting for the pump to let go of the model.
struct Launch {
    ids: Vec<EntryId>,
    done: SearchDone,
}

/// `_research_rows` as the server's thread sees it: it asks, the window runs.
struct GuiLauncher {
    pending: Arc<Mutex<Vec<Launch>>>,
}

impl SearchLauncher for GuiLauncher {
    fn launch(
        &self,
        batch: Vec<(EntryId, Entry)>,
        pending: Vec<usize>,
        done: SearchDone,
    ) -> Result<(), String> {
        // a row named twice is searched twice, as the worker was handed the same entry twice
        let ids = pending.iter().map(|p| batch[*p].0).collect();
        self.pending
            .lock()
            .expect("launch list")
            .push(Launch { ids, done });
        Ok(())
    }
}

/// `self._mcp_server` and the channel to the window's queue.
pub struct McpBridge {
    inbox: QueueInbox,
    channel: QueueChannel,
    launches: Arc<Mutex<Vec<Launch>>>,
    /// The tool that is waiting for the window's search to end.
    research_done: RefCell<Option<SearchDone>>,
    /// Built when the server is first wanted: building it builds the search engines.
    controller: RefCell<Option<McpServerController>>,
    /// Why the controller could not be built, if it could not.
    not_built: RefCell<Option<String>>,
    /// The limits on a marshaled call; a test shortens them.
    pub timeouts: Cell<Timeouts>,
}

impl McpBridge {
    /// The bridge and the receiver whose messages mean "the inbox has jobs" (see
    /// [`MainWindow::attach_mcp`]).
    pub fn new() -> (McpBridge, async_channel::Receiver<()>) {
        let (wake_tx, wake_rx) = async_channel::unbounded::<()>();
        let (inbox, channel) = QueueInbox::channel(move || {
            let _ = wake_tx.try_send(());
        });
        (
            McpBridge {
                inbox,
                channel,
                launches: Arc::new(Mutex::new(Vec::new())),
                research_done: RefCell::new(None),
                controller: RefCell::new(None),
                not_built: RefCell::new(None),
                timeouts: Cell::new(Timeouts::default()),
            },
            wake_rx,
        )
    }

    /// Whether a server has been built and is listening.
    pub fn running(&self) -> bool {
        self.controller
            .borrow()
            .as_ref()
            .is_some_and(McpServerController::running)
    }
}

impl MainWindow {
    /// Pumps the inbox on the main thread each time a call is queued, for as long as the window
    /// lives.
    pub fn attach_mcp(&self, wake: async_channel::Receiver<()>) {
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            while wake.recv().await.is_ok() {
                let Some(window) = weak.upgrade() else { break };
                window.pump_mcp();
            }
        });
    }

    /// Runs the tool calls that are waiting, draws what they changed, and starts the re-searches
    /// they asked for.
    pub fn pump_mcp(&self) {
        let current = self.current_id();
        let events = {
            let mut queue = self.s.queue.borrow_mut();
            // `window.table.currentIndex()`: the row the preview follows
            queue.set_current(current);
            self.s.mcp.inbox.pump(&mut queue);
            queue.take_events()
        };
        self.render_queue_events(&events);
        self.start_pending_researches();
    }

    /// What `_refresh_entry_row`, `_refresh_table` and `_on_selection_changed` do for the changes
    /// a tool made.
    fn render_queue_events(&self, events: &[QueueEvent]) {
        let table = events
            .iter()
            .any(|e| matches!(e, QueueEvent::Removed(_) | QueueEvent::TableRefreshed));
        if table {
            self.list_changed();
            // `_remove_rows` leaves the selection on the row that followed the removed ones
            if self.s.queue_view.selected_ids().is_empty() {
                let next = self.s.queue.borrow().current();
                if let Some(row) = next.and_then(|id| self.s.queue_view.row_of(id)) {
                    self.s.queue_view.select_row(row);
                }
            }
        } else {
            let mut seen: Vec<EntryId> = Vec::new();
            for event in events {
                if let QueueEvent::RowChanged(id) = event {
                    if !seen.contains(id) {
                        seen.push(*id);
                        self.refresh_entry_row(*id);
                    }
                }
            }
        }
        if events
            .iter()
            .any(|e| matches!(e, QueueEvent::SelectionChanged))
        {
            self.on_selection_changed();
        }
        // `_remove_rows` ends on the status line, after the selection has moved
        if let Some(QueueEvent::Removed(ids)) = events
            .iter()
            .rev()
            .find(|e| matches!(e, QueueEvent::Removed(_)))
        {
            self.say(&format!("Removed {} file(s) from the list", ids.len()));
        }
    }

    /// The window half of `_research_rows`: the search a tool asked for runs as any re-search
    /// does, with the cache bypassed, and the tool is released when it ends.
    fn start_pending_researches(&self) {
        let launches = std::mem::take(&mut *self.s.mcp.launches.lock().expect("launch list"));
        for launch in launches {
            *self.s.mcp.research_done.borrow_mut() = Some(launch.done);
            let started = self.launch_search_worker(
                launch.ids,
                LaunchOptions {
                    status_prefix: "Re-searching".to_owned(),
                    bypass_cache: true,
                    ..LaunchOptions::default()
                },
            );
            if !started {
                self.release_mcp_research();
            }
        }
    }

    /// The window's search ended (or never started): lets the waiting tool go on. The model is
    /// already up to date, so there is nothing to hand back.
    pub(crate) fn release_mcp_research(&self) {
        let done = self.s.mcp.research_done.borrow_mut().take();
        if let Some(done) = done {
            // clears the model's `search_running`, which the tool set when it asked
            done.finish(Vec::new(), Vec::new());
        }
    }

    /// `McpToolHandlers(self)`: the tools over this window.
    fn mcp_handlers(&self) -> Result<Arc<dyn McpHandlers>, String> {
        let backend = self.s.search.backend(&self.s.paths)?;
        let runtime = self
            .s
            .workers
            .handle()
            .ok_or_else(|| "the worker runtime is gone".to_owned())?;
        let env = HandlerEnv {
            timeouts: self.s.mcp.timeouts.get(),
            settings: self.s.settings.clone(),
            engine: backend.engine.clone(),
            hydrus_client: backend.hydrus_client.clone(),
            launcher: Arc::new(GuiLauncher {
                pending: self.s.mcp.launches.clone(),
            }),
            runtime,
            audit_path: self.s.paths.mcp_audit_log(),
        };
        Ok(Arc::new(McpToolHandlers::new(
            self.s.mcp.channel.clone(),
            env,
        )))
    }

    /// Builds the controller if it is not there yet.
    fn ensure_mcp_controller(&self) -> bool {
        if self.s.mcp.controller.borrow().is_some() {
            return true;
        }
        match self.mcp_handlers() {
            Ok(handlers) => {
                *self.s.mcp.controller.borrow_mut() = Some(McpServerController::new(handlers));
                *self.s.mcp.not_built.borrow_mut() = None;
                true
            }
            Err(error) => {
                tracing::warn!("MCP server could not be built: {error}");
                *self.s.mcp.not_built.borrow_mut() = Some(error);
                false
            }
        }
    }

    /// `MainWindow.__init__`: starts the server if Settings > MCP has it on. A blank token or a
    /// busy port is logged, not raised.
    pub fn start_mcp_at_launch(&self) {
        let settings = self.s.settings.lock().unwrap().mcp.clone();
        if !settings.enabled || !self.ensure_mcp_controller() {
            return;
        }
        let mut controller = self.s.mcp.controller.borrow_mut();
        let controller = controller.as_mut().expect("just built");
        if !controller.start(&settings) {
            if let Some(error) = controller.last_error() {
                tracing::warn!("MCP server did not start: {error}");
            }
        }
    }

    /// `self._mcp_server.restart(settings)`: re-binds with these settings, which are not
    /// necessarily the saved ones (the Restart button uses what is typed).
    pub fn restart_mcp(&self, settings: &McpSettings) -> bool {
        // switching it off with nothing running builds nothing
        if !settings.enabled && self.s.mcp.controller.borrow().is_none() {
            return false;
        }
        if !self.ensure_mcp_controller() {
            return false;
        }
        let mut controller = self.s.mcp.controller.borrow_mut();
        let controller = controller.as_mut().expect("just built");
        let started = controller.restart(settings);
        if !started && settings.enabled {
            if let Some(error) = controller.last_error() {
                tracing::warn!("MCP server did not restart: {error}");
            }
        }
        started
    }

    /// `closeEvent`: no new tool calls, then the server comes down.
    pub fn stop_mcp(&self) {
        self.s.mcp.channel.shutdown();
        if let Some(controller) = self.s.mcp.controller.borrow_mut().as_mut() {
            controller.stop(true);
        }
    }

    /// What the Connection card is drawn from.
    pub fn mcp_report(&self) -> ServerReport {
        match self.s.mcp.controller.borrow().as_ref() {
            Some(c) => ServerReport {
                running: c.running(),
                last_error: c.last_error().map(str::to_owned),
                user_message: c.last_error_user_message(),
            },
            None => match self.s.mcp.not_built.borrow().as_ref() {
                Some(error) => ServerReport {
                    running: false,
                    last_error: Some(format!("MCP server not built: {error}")),
                    user_message: Some(NOT_BUILT_USER_MESSAGE.to_owned()),
                },
                None => ServerReport::default(),
            },
        }
    }
}
