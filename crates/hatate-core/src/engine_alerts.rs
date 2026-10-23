//! Engine failures the user has to be told about, not only logged (`core/engine_alerts.py`).
//!
//! A missing optional dependency is not a failed request: the engine cannot run at all, and every
//! image it is asked about fails the same way for the rest of the session. This is the channel for
//! that: core raises an [`EngineAlert`], whoever subscribed shows it. Each alert carries a `key`,
//! and a key is delivered once per session, so a batch of four hundred images does not stack four
//! hundred identical dialogs.
//!
//! Python keeps the listeners and the set of keys already raised in module globals; here they are
//! an [`EngineAlerts`] the engine client is handed, so two clients (or two tests) do not share them.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// `EngineAlert`: one thing worth interrupting the user for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineAlert {
    /// What the once-per-session rule is applied to: the condition, not the occurrence.
    pub key: String,
    pub engine: String,
    pub title: String,
    pub body: String,
    /// The command to run, kept apart so it can be shown as something copyable.
    pub remedy: String,
}

impl EngineAlert {
    /// `EngineAlert.text`: the whole thing as one block, for a plain message box.
    pub fn text(&self) -> String {
        if self.remedy.is_empty() {
            self.body.clone()
        } else {
            format!("{}\n\n{}", self.body, self.remedy)
        }
    }
}

/// `Listener`.
pub type Listener = Arc<dyn Fn(&EngineAlert) + Send + Sync + 'static>;

#[derive(Default)]
struct Inner {
    listeners: Vec<Listener>,
    raised: HashSet<String>,
}

/// `subscribe`, `unsubscribe`, `raise_alert`, `already_raised` and `reset`, over one set of listeners and keys.
#[derive(Default)]
pub struct EngineAlerts {
    inner: Mutex<Inner>,
}

impl EngineAlerts {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// `subscribe`.
    pub fn subscribe(&self, listener: Listener) {
        let mut inner = self.lock();
        if !inner.listeners.iter().any(|l| Arc::ptr_eq(l, &listener)) {
            inner.listeners.push(listener);
        }
    }

    /// `unsubscribe`: a listener that was never subscribed is left alone, as in Python.
    pub fn unsubscribe(&self, listener: &Listener) {
        self.lock().listeners.retain(|l| !Arc::ptr_eq(l, listener));
    }

    /// `raise_alert`: delivers `alert` unless its key has already been delivered. Returns whether
    /// it was delivered. A listener that panics must not take the search down with it, so each is
    /// called on its own.
    pub fn raise_alert(&self, alert: &EngineAlert) -> bool {
        let listeners = {
            let mut inner = self.lock();
            if !inner.raised.insert(alert.key.clone()) {
                return false;
            }
            inner.listeners.clone()
        };
        for listener in listeners {
            let call = std::panic::AssertUnwindSafe(|| listener(alert));
            if std::panic::catch_unwind(call).is_err() {
                tracing::debug!("An engine-alert listener failed");
            }
        }
        true
    }

    /// `already_raised`.
    pub fn already_raised(&self, key: &str) -> bool {
        self.lock().raised.contains(key)
    }

    /// `reset`: forget which alerts have been shown. Deliberately not called when a search starts.
    pub fn reset(&self) {
        self.lock().raised.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(key: &str) -> EngineAlert {
        EngineAlert {
            key: key.to_owned(),
            engine: "Google Lens".to_owned(),
            title: "t".to_owned(),
            body: "b".to_owned(),
            remedy: String::new(),
        }
    }

    #[test]
    fn a_key_is_delivered_once_until_reset() {
        let alerts = EngineAlerts::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        alerts.subscribe(Arc::new(move |a: &EngineAlert| {
            sink.lock().unwrap().push(a.key.clone());
        }));
        assert!(alerts.raise_alert(&alert("k")));
        assert!(!alerts.raise_alert(&alert("k")));
        assert!(alerts.already_raised("k"));
        assert!(alerts.raise_alert(&alert("other")));
        alerts.reset();
        assert!(alerts.raise_alert(&alert("k")));
        assert_eq!(*seen.lock().unwrap(), vec!["k", "other", "k"]);
    }

    #[test]
    fn a_listener_that_panics_does_not_stop_the_others_or_the_caller() {
        let alerts = EngineAlerts::new();
        let seen = Arc::new(Mutex::new(0));
        let sink = Arc::clone(&seen);
        alerts.subscribe(Arc::new(|_: &EngineAlert| panic!("boom")));
        alerts.subscribe(Arc::new(move |_: &EngineAlert| *sink.lock().unwrap() += 1));
        assert!(alerts.raise_alert(&alert("k")));
        assert_eq!(*seen.lock().unwrap(), 1);
    }

    #[test]
    fn an_unsubscribed_listener_stops_hearing() {
        let alerts = EngineAlerts::new();
        let seen = Arc::new(Mutex::new(0));
        let sink = Arc::clone(&seen);
        let kept: Listener = Arc::new(move |_: &EngineAlert| *sink.lock().unwrap() += 1);
        let other_sink = Arc::new(Mutex::new(0));
        let sink = Arc::clone(&other_sink);
        let dropped: Listener = Arc::new(move |_: &EngineAlert| *sink.lock().unwrap() += 1);
        alerts.subscribe(Arc::clone(&kept));
        alerts.subscribe(Arc::clone(&dropped));
        alerts.unsubscribe(&dropped);
        // A listener that was never subscribed is not an error.
        alerts.unsubscribe(&dropped);
        assert!(alerts.raise_alert(&alert("k")));
        assert_eq!(*seen.lock().unwrap(), 1);
        assert_eq!(*other_sink.lock().unwrap(), 0);
    }

    #[test]
    fn text_puts_the_remedy_in_its_own_block() {
        let mut a = alert("k");
        assert_eq!(a.text(), "b");
        a.remedy = "run x".to_owned();
        assert_eq!(a.text(), "b\n\nrun x");
    }
}
