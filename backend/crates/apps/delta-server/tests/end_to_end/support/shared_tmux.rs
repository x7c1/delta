//! The handle that lets the app own the fake tmux driver while a test
//! observes it.

use std::sync::Arc;

use async_trait::async_trait;

use delta_usecase::TmuxDriver;

use super::FakeTmux;

/// A local wrapper around a shared [`FakeTmux`] so the app can own the driver
/// while the test keeps a handle to observe how many lines were dispatched.
pub(super) struct SharedTmux(pub(super) Arc<FakeTmux>);

#[async_trait]
impl TmuxDriver for SharedTmux {
    async fn has_session(&self, name: &str) -> delta_usecase::Result<bool> {
        self.0.has_session(name).await
    }

    async fn create_session(
        &self,
        name: &str,
        workdir: &str,
        command: &[String],
    ) -> delta_usecase::Result<()> {
        self.0.create_session(name, workdir, command).await
    }

    async fn send_line(&self, pane: &str, text: &str) -> delta_usecase::Result<()> {
        self.0.send_line(pane, text).await
    }

    async fn send_keys(&self, pane: &str, keys: &[&str]) -> delta_usecase::Result<()> {
        self.0.send_keys(pane, keys).await
    }

    async fn clear_input(&self, pane: &str) -> delta_usecase::Result<()> {
        self.0.clear_input(pane).await
    }

    async fn kill_session(&self, name: &str) -> delta_usecase::Result<()> {
        self.0.kill_session(name).await
    }

    async fn kill_server(&self) -> delta_usecase::Result<()> {
        self.0.kill_server().await
    }

    async fn capture_pane(&self, pane: &str) -> delta_usecase::Result<String> {
        self.0.capture_pane(pane).await
    }
}
