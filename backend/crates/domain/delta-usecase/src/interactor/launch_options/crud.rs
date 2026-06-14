use delta_model::LaunchOption;

use crate::error::Result;
use crate::interactor::InteractorCore;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W> InteractorCore<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// All registered launch options for the settings screen, newest first.
    pub async fn list_launch_options(&self) -> Result<Vec<LaunchOption>> {
        self.store.list_launch_options().await
    }

    /// Register a new launch option. `label` and `value` are optional (a
    /// valueless flag carries no `value`); `name` is the flag itself.
    pub async fn create_launch_option(
        &self,
        label: Option<&str>,
        name: &str,
        value: Option<&str>,
    ) -> Result<LaunchOption> {
        self.store.create_launch_option(label, name, value).await
    }

    /// Delete a launch option by id. Deleting an unknown id is a no-op.
    pub async fn delete_launch_option(&self, id: i64) -> Result<()> {
        self.store.delete_launch_option(id).await
    }
}
