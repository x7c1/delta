//! The tmux driver the end-to-end tests run against.

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;

use delta_usecase::TmuxDriver;

/// A `TmuxDriver` that records the lines it would have sent instead of touching
/// a real tmux pane, so the test can assert the keystrokes were dispatched. It
/// also models the session lifecycle in memory so a spawn works without a real
/// tmux.
#[derive(Default)]
pub(crate) struct FakeTmux {
    /// The number of lines dispatched via `send_line`.
    pub(crate) sent: AtomicUsize,
    /// The number of sessions spawned via `create_session`.
    pub(crate) created: AtomicUsize,
    /// The `command` argv of the most recent `create_session` call, so a test
    /// can assert the first prompt rides on the launch command line.
    pub(crate) last_command: std::sync::Mutex<Vec<String>>,
    /// The keystroke sequences injected via `send_keys`, in order, so a test
    /// can assert the exact keys an answered question drove into the pane.
    pub(crate) keyed: std::sync::Mutex<Vec<Vec<String>>>,
}

#[async_trait]
impl TmuxDriver for FakeTmux {
    async fn has_session(&self, _name: &str) -> delta_usecase::Result<bool> {
        Ok(false)
    }

    async fn create_session(
        &self,
        _name: &str,
        _workdir: &str,
        command: &[String],
    ) -> delta_usecase::Result<()> {
        self.created.fetch_add(1, Ordering::SeqCst);
        *self.last_command.lock().unwrap() = command.to_vec();
        Ok(())
    }

    async fn send_line(&self, _pane: &str, _text: &str) -> delta_usecase::Result<()> {
        self.sent.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn send_keys(&self, _pane: &str, keys: &[&str]) -> delta_usecase::Result<()> {
        self.keyed
            .lock()
            .unwrap()
            .push(keys.iter().map(|k| (*k).to_owned()).collect());
        Ok(())
    }

    async fn clear_input(&self, _pane: &str) -> delta_usecase::Result<()> {
        Ok(())
    }

    async fn kill_session(&self, _name: &str) -> delta_usecase::Result<()> {
        Ok(())
    }

    async fn kill_server(&self) -> delta_usecase::Result<()> {
        Ok(())
    }

    async fn capture_pane(&self, _pane: &str) -> delta_usecase::Result<String> {
        Ok(String::new())
    }
}
