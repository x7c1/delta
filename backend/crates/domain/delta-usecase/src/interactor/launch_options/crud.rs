use delta_model::{AgentProvider, LaunchOption};

use crate::error::{Error, Result};
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
    /// All registered launch options for the settings screen: the rows Delta
    /// ships first, then the user's own newest first.
    pub async fn list_launch_options(&self) -> Result<Vec<LaunchOption>> {
        self.store.list_launch_options().await
    }

    /// Whether an option disables the agent's own safety mechanisms, per the
    /// injected [`LaunchOptionDangerPolicy`].
    ///
    /// The pair-based sibling of [`Self::is_launch_option_dangerous`], for the
    /// create path — which has to classify an option before any row exists.
    pub fn is_launch_option_pair_dangerous(
        &self,
        provider: AgentProvider,
        name: &str,
        value: Option<&str>,
    ) -> bool {
        self.launch_option_danger
            .is_dangerous(provider, name, value)
    }

    /// Whether a registered option disables the agent's own safety mechanisms.
    ///
    /// Read on every path that surfaces or writes a row: the wire mapping (so the
    /// browser can mark the row and refuse to pre-check it) and the two writes
    /// that could turn such an option on by default.
    pub fn is_launch_option_dangerous(&self, option: &LaunchOption) -> bool {
        self.is_launch_option_pair_dangerous(option.provider, &option.name, option.value.as_deref())
    }

    /// Register a new launch option. `label` and `value` are optional (a
    /// valueless flag carries no `value`); `name` is the flag itself.
    /// `default_enabled` marks it to start pre-checked in the session-start
    /// picker. `provider` is the provider the option applies to.
    ///
    /// An option that switches the agent's own safety mechanisms off
    /// (`--dangerously-skip-permissions`, a `danger-full-access` Codex sandbox —
    /// see [`LaunchOptionDangerPolicy`]) may be registered, but never with
    /// `default_enabled`: pre-checking it would silently disarm every new
    /// session. Such a create is refused with [`Error::LaunchOptionRejected`];
    /// the same option with `default_enabled: false` is created normally and
    /// stays selectable per send.
    ///
    /// [`LaunchOptionDangerPolicy`]: crate::agent::LaunchOptionDangerPolicy
    pub async fn create_launch_option(
        &self,
        label: Option<&str>,
        name: &str,
        value: Option<&str>,
        default_enabled: bool,
        provider: AgentProvider,
    ) -> Result<LaunchOption> {
        if default_enabled && self.is_launch_option_pair_dangerous(provider, name, value) {
            return Err(dangerous_default_rejected(name));
        }
        self.store
            .create_launch_option(label, name, value, default_enabled, provider)
            .await
    }

    /// Set a launch option's `default_enabled` flag, returning the updated row,
    /// or `None` if no option has that id.
    ///
    /// Turning the flag *on* for an option that disables the agent's own safety
    /// mechanisms is refused with [`Error::LaunchOptionRejected`], for the same
    /// reason the create path refuses it — a dangerous option is never silently
    /// pre-checked. Turning it off is always allowed, so a row that predates this
    /// rule can be disarmed. An id nobody has stays a `None` (a `404` at the
    /// REST layer) rather than a rejection: there is no row to classify.
    pub async fn set_launch_option_default_enabled(
        &self,
        id: i64,
        default_enabled: bool,
    ) -> Result<Option<LaunchOption>> {
        if default_enabled {
            if let Some(option) = self.store.launch_option(id).await? {
                if self.is_launch_option_dangerous(&option) {
                    return Err(dangerous_default_rejected(&option.name));
                }
            }
        }
        self.store
            .set_launch_option_default_enabled(id, default_enabled)
            .await
    }

    /// Delete a launch option by id. Deleting an unknown id is a no-op.
    ///
    /// A row Delta *ships* is refused with [`Error::LaunchOptionIsBuiltin`]:
    /// the declared catalog owns those rows, so a removed row would simply
    /// reappear at the next startup. Leaving it unticked is how a shipped
    /// option is declined.
    pub async fn delete_launch_option(&self, id: i64) -> Result<()> {
        // An unknown id stays a silent no-op, so this only refuses when there
        // is a row and that row is Delta's own.
        if let Some(option) = self.store.launch_option(id).await? {
            if option.builtin_key.is_some() {
                return Err(Error::LaunchOptionIsBuiltin(id));
            }
        }
        self.store.delete_launch_option(id).await
    }
}

/// The refusal both write paths answer with when a dangerous option is asked to
/// be default-enabled.
///
/// One function so the two paths cannot drift into two different explanations of
/// the same rule, and so the message always names the offending option — the
/// registry can hold many rows and the user needs to know which one was refused.
///
/// Being one message for both paths, the way out it names has to hold for both:
/// "leave it off by default" is something a caller registering a new row and a
/// caller flipping an existing row's flag can each act on, whereas "register it
/// undefaulted" would be telling the second one to create what already exists.
fn dangerous_default_rejected(name: &str) -> Error {
    Error::LaunchOptionRejected(format!(
        "`{name}` turns off the agent's own safety mechanism, so it cannot be \
         enabled by default: it would silently apply to every new session. \
         Leave it off by default and select it per session instead"
    ))
}
