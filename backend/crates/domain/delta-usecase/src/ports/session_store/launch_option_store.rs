//! The launch-option registry backing the settings screen.

use async_trait::async_trait;

use delta_model::{AgentProvider, LaunchOption};

use super::SessionStore;
use crate::error::Result;

/// The launch-option registry backing the settings screen: the options Delta
/// ships and the ones the user registered.
///
/// One of the concerns [`SessionStore`] gathers.
#[async_trait]
pub trait LaunchOptionStore: Send + Sync {
    /// All registered launch options: the rows Delta ships first (ascending
    /// `id`, i.e. declared-catalog order), then the user's own rows newest first
    /// (descending `id`).
    ///
    /// The shipped block leads and is fixed-length, so a shipped row's position
    /// in the settings list never moves as the user adds or removes their own.
    async fn list_launch_options(&self) -> Result<Vec<LaunchOption>>;

    /// One launch option by id, or `None` if no option has that id.
    ///
    /// Read before a delete, so the refusal to delete a row Delta ships can be
    /// decided on the row's `builtin_key` rather than guessed from a row count.
    async fn launch_option(&self, id: i64) -> Result<Option<LaunchOption>>;

    /// Register a launch option and return the created row. `label` and `value`
    /// are optional (a valueless flag carries no `value`); `name` is the flag.
    /// `default_enabled` marks it to start pre-checked in the session-start
    /// picker. `provider` is the provider the option applies to (the caller
    /// defaults it to [`AgentProvider::Claude`] for a back-compat create that
    /// omits it).
    async fn create_launch_option(
        &self,
        label: Option<&str>,
        name: &str,
        value: Option<&str>,
        default_enabled: bool,
        provider: AgentProvider,
    ) -> Result<LaunchOption>;

    /// Set a launch option's `default_enabled` flag, returning the updated row,
    /// or `None` if no option has that id.
    async fn set_launch_option_default_enabled(
        &self,
        id: i64,
        default_enabled: bool,
    ) -> Result<Option<LaunchOption>>;

    /// Delete a launch option by id. Deleting an unknown id is a no-op.
    ///
    /// Deliberately unconditional: refusing to remove a row Delta ships is a
    /// domain rule enforced above this port (in the use case's
    /// `delete_launch_option`), not a store concern — reconciliation itself has
    /// to be able to drop a retired shipped row, which it does through
    /// [`Self::delete_builtin_launch_options_except`].
    async fn delete_launch_option(&self, id: i64) -> Result<()>;

    /// Materialize one declared launch-option preset, keyed by `builtin_key`.
    ///
    /// Absent: insert it with `default_enabled` **off** — a shipped option is
    /// offered, never imposed. Present: update `label`, `name`, `value` and
    /// `provider` from the declared catalog (which owns those fields) and
    /// **leave `default_enabled` untouched**, because that flag is the user's.
    /// The row is updated in place, so its id — which may already sit in a saved
    /// selection — survives. Returns the resulting row.
    async fn upsert_builtin_launch_option(
        &self,
        builtin_key: &str,
        label: &str,
        name: &str,
        value: Option<&str>,
        provider: AgentProvider,
    ) -> Result<LaunchOption>;

    /// Delete every Delta-shipped row whose `builtin_key` is **not** in `keys`,
    /// returning how many went. Rows the user registered (`builtin_key` null)
    /// are never in scope, whatever `keys` holds — including when it is empty.
    ///
    /// This is how a preset retired from the catalog leaves the registry.
    async fn delete_builtin_launch_options_except(&self, keys: &[&str]) -> Result<usize>;
}

#[async_trait]
impl LaunchOptionStore for Box<dyn SessionStore> {
    async fn list_launch_options(&self) -> Result<Vec<LaunchOption>> {
        (**self).list_launch_options().await
    }

    async fn launch_option(&self, id: i64) -> Result<Option<LaunchOption>> {
        (**self).launch_option(id).await
    }

    async fn create_launch_option(
        &self,
        label: Option<&str>,
        name: &str,
        value: Option<&str>,
        default_enabled: bool,
        provider: AgentProvider,
    ) -> Result<LaunchOption> {
        (**self)
            .create_launch_option(label, name, value, default_enabled, provider)
            .await
    }

    async fn set_launch_option_default_enabled(
        &self,
        id: i64,
        default_enabled: bool,
    ) -> Result<Option<LaunchOption>> {
        (**self)
            .set_launch_option_default_enabled(id, default_enabled)
            .await
    }

    async fn delete_launch_option(&self, id: i64) -> Result<()> {
        (**self).delete_launch_option(id).await
    }

    async fn upsert_builtin_launch_option(
        &self,
        builtin_key: &str,
        label: &str,
        name: &str,
        value: Option<&str>,
        provider: AgentProvider,
    ) -> Result<LaunchOption> {
        (**self)
            .upsert_builtin_launch_option(builtin_key, label, name, value, provider)
            .await
    }

    async fn delete_builtin_launch_options_except(&self, keys: &[&str]) -> Result<usize> {
        (**self).delete_builtin_launch_options_except(keys).await
    }
}
