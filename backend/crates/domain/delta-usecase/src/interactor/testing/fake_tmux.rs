//! In-memory [`TmuxDriver`] fake recording the calls the interactor makes.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::Notify;

use crate::error::Result;
use crate::ports::TmuxDriver;

/// A hold on the fake's `create_session`, so a test can act in the window in
/// which a launch has been prepared but its pane does not exist yet.
///
/// That window is where the ordering a launch depends on is decided: the pending
/// spawn must already be recorded when the pane comes up, because the hooks the
/// launched agent fires bind *that* record. Holding `create_session` open lets a
/// test stand exactly there — [`Self::await_entered`] waits until the launch is
/// inside the call — and deliver a hook the way a fast agent would.
///
/// Mirrors [`WorktreeGate`](super::WorktreeGate), which holds the step before
/// this one.
#[derive(Clone)]
pub(crate) struct TmuxGate(Arc<TmuxGateInner>);

#[derive(Default)]
struct TmuxGateInner {
    open: Mutex<bool>,
    opened: Notify,
    /// How many `create_session` calls have reached the gate.
    entered: Mutex<usize>,
    entered_notify: Notify,
}

impl TmuxGate {
    /// A gate that is closed: every `create_session` waits until [`Self::open`].
    pub(crate) fn closed() -> Self {
        Self(Arc::new(TmuxGateInner::default()))
    }

    /// Let the held (and every later) `create_session` through.
    pub(crate) fn open(&self) {
        *self.0.open.lock().unwrap() = true;
        self.0.opened.notify_waiters();
    }

    /// Wait until a `create_session` has reached the gate — i.e. the launch has
    /// finished preparing and is about to create its pane.
    pub(crate) async fn await_entered(&self) {
        loop {
            // Register for the notification *before* re-reading the count, so an
            // entry landing between the two is not missed.
            let entered = self.0.entered_notify.notified();
            if *self.0.entered.lock().unwrap() > 0 {
                return;
            }
            entered.await;
        }
    }

    async fn wait(&self) {
        *self.0.entered.lock().unwrap() += 1;
        self.0.entered_notify.notify_waiters();
        loop {
            let opened = self.0.opened.notified();
            if *self.0.open.lock().unwrap() {
                return;
            }
            opened.await;
        }
    }
}

/// One recorded input into a pane, in the order the interactor produced it.
///
/// `sent` and `keyed` each record their own call kind, which cannot show how
/// the two INTERLEAVE — and some behaviour is exactly an interleaving: a send
/// re-typed by the echo-deadline watchdog must be preceded by an `Escape` into
/// the same pane, so that a lingering TUI dialog is dismissed before the text
/// lands. This single ordered log is what lets a test assert that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PaneInput {
    /// A `send_line`: one submitted line of text.
    Line { pane: String, text: String },
    /// A `send_keys`: the ordered tmux key names injected.
    Keys { pane: String, keys: Vec<String> },
}

/// A single recorded `create_session` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreatedSession {
    pub(crate) name: String,
    pub(crate) workdir: String,
    pub(crate) command: Vec<String>,
}

#[derive(Default)]
pub(crate) struct FakeTmux {
    /// The `(pane, text)` pairs `send_line` was called with.
    pub(crate) sent: Mutex<Vec<(String, String)>>,
    /// The `(pane, keys)` pairs `send_keys` was called with, in order. Each
    /// entry's `keys` is the ordered list of tmux key names injected.
    pub(crate) keyed: Mutex<Vec<(String, Vec<String>)>>,
    /// Every `send_line` and `send_keys` in ONE ordered log, so a test can
    /// assert their interleaving (see [`PaneInput`]). The two vectors above
    /// stay as they are: most tests only care about one kind.
    pub(crate) pane_input: Mutex<Vec<PaneInput>>,
    /// The panes `clear_input` was called with, in order.
    pub(crate) cleared: Mutex<Vec<String>>,
    /// When set, `send_line` fails instead of recording the line, simulating a
    /// dispatch failure into the pane.
    pub(crate) fail: bool,
    /// When set, `create_session` fails instead of recording the spawn,
    /// simulating a failed session launch.
    pub(crate) fail_create: bool,
    /// The session names currently "existing" for `has_session`.
    pub(crate) live: Mutex<Vec<String>>,
    /// The sessions `create_session` was called with, in order.
    pub(crate) created: Mutex<Vec<CreatedSession>>,
    /// The session names `kill_session` was called with, in order.
    pub(crate) killed: Mutex<Vec<String>>,
    /// When set, every `create_session` waits on this gate before recording (or
    /// failing) anything — the seam a test holds open to act while a launch is
    /// between "prepared" and "pane up". `None` (the default) means no wait at
    /// all, so every other test is unaffected.
    pub(crate) gate: Option<TmuxGate>,
}

impl FakeTmux {
    /// Hold every `create_session` on `gate` until the test opens it, so the
    /// prepared→pane window can be observed.
    pub(crate) fn with_gate(mut self, gate: &TmuxGate) -> Self {
        self.gate = Some(gate.clone());
        self
    }
}

#[async_trait]
impl TmuxDriver for FakeTmux {
    async fn has_session(&self, name: &str) -> Result<bool> {
        Ok(self.live.lock().unwrap().iter().any(|n| n == name))
    }

    async fn create_session(&self, name: &str, workdir: &str, command: &[String]) -> Result<()> {
        if let Some(gate) = &self.gate {
            gate.wait().await;
        }
        if self.fail_create {
            return Err(crate::error::Error::Tmux("create failed".into()));
        }
        self.created.lock().unwrap().push(CreatedSession {
            name: name.to_owned(),
            workdir: workdir.to_owned(),
            command: command.to_vec(),
        });
        self.live.lock().unwrap().push(name.to_owned());
        Ok(())
    }

    async fn send_line(&self, pane: &str, text: &str) -> Result<()> {
        if self.fail {
            return Err(crate::error::Error::Tmux("dispatch failed".into()));
        }
        self.sent
            .lock()
            .unwrap()
            .push((pane.to_owned(), text.to_owned()));
        self.pane_input.lock().unwrap().push(PaneInput::Line {
            pane: pane.to_owned(),
            text: text.to_owned(),
        });
        Ok(())
    }

    async fn send_keys(&self, pane: &str, keys: &[&str]) -> Result<()> {
        if self.fail {
            return Err(crate::error::Error::Tmux("key injection failed".into()));
        }
        self.keyed.lock().unwrap().push((
            pane.to_owned(),
            keys.iter().map(|k| (*k).to_owned()).collect(),
        ));
        self.pane_input.lock().unwrap().push(PaneInput::Keys {
            pane: pane.to_owned(),
            keys: keys.iter().map(|k| (*k).to_owned()).collect(),
        });
        Ok(())
    }

    async fn clear_input(&self, pane: &str) -> Result<()> {
        self.cleared.lock().unwrap().push(pane.to_owned());
        Ok(())
    }

    async fn kill_session(&self, name: &str) -> Result<()> {
        self.killed.lock().unwrap().push(name.to_owned());
        self.live.lock().unwrap().retain(|n| n != name);
        Ok(())
    }
}
