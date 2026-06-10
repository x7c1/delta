//! In-memory [`TmuxDriver`] fake recording the calls the interactor makes.

use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::Result;
use crate::ports::TmuxDriver;

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
    /// The panes `clear_input` was called with, in order.
    pub(crate) cleared: Mutex<Vec<String>>,
    /// When set, `send_line` fails instead of recording the line, simulating a
    /// dispatch failure into the pane.
    pub(crate) fail: bool,
    /// The session names currently "existing" for `has_session`.
    pub(crate) live: Mutex<Vec<String>>,
    /// The sessions `create_session` was called with, in order.
    pub(crate) created: Mutex<Vec<CreatedSession>>,
    /// The session names `kill_session` was called with, in order.
    pub(crate) killed: Mutex<Vec<String>>,
}

#[async_trait]
impl TmuxDriver for FakeTmux {
    async fn has_session(&self, name: &str) -> Result<bool> {
        Ok(self.live.lock().unwrap().iter().any(|n| n == name))
    }

    async fn create_session(&self, name: &str, workdir: &str, command: &[String]) -> Result<()> {
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
