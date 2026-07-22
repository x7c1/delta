use delta_model::{AgentProvider, LaunchOption};

use crate::error::Result;
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// All registered launch options for the settings screen, newest first.
    pub async fn list_launch_options(&self) -> Result<Vec<LaunchOption>> {
        self.store.list_launch_options().await
    }

    /// Register a new launch option. `label` and `value` are optional (a
    /// valueless flag carries no `value`); `name` is the flag itself.
    /// `default_enabled` marks it to start pre-checked in the session-start
    /// picker. `provider` is the provider the option applies to.
    pub async fn create_launch_option(
        &self,
        label: Option<&str>,
        name: &str,
        value: Option<&str>,
        default_enabled: bool,
        provider: AgentProvider,
    ) -> Result<LaunchOption> {
        self.store
            .create_launch_option(label, name, value, default_enabled, provider)
            .await
    }

    /// Set a launch option's `default_enabled` flag, returning the updated row,
    /// or `None` if no option has that id.
    pub async fn set_launch_option_default_enabled(
        &self,
        id: i64,
        default_enabled: bool,
    ) -> Result<Option<LaunchOption>> {
        self.store
            .set_launch_option_default_enabled(id, default_enabled)
            .await
    }

    /// Delete a launch option by id. Deleting an unknown id is a no-op.
    pub async fn delete_launch_option(&self, id: i64) -> Result<()> {
        self.store.delete_launch_option(id).await
    }
}
