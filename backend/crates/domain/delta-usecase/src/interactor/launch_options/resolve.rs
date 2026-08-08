//! Resolving the ids a session-start request carries into the launch options
//! themselves.
//!
//! Shared by both spawn paths — Claude's pane spawn
//! ([`spawn_fresh`](crate::interactor::session_actor::actor::SessionContext::spawn_fresh))
//! and the adapter-backed spawn
//! ([`spawn_adapter_session`](crate::interactor::session_actor::actor::SessionContext::spawn_adapter_session))
//! — so a selection is resolved once, the same way, for every provider. The
//! result is a list of neutral [`LaunchOptionSpec`] pairs; how a pair becomes a
//! CLI flag or a request field is the adapter's business, not this layer's.

use crate::agent::LaunchOptionSpec;
use crate::error::Result;
use crate::interactor::launch_options::expand_leading_tilde;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Resolve the launch-option ids a session-start request selected into
    /// their registered `(name, value?)` records, in the user's selection
    /// order.
    ///
    /// Callers run this alongside their other side-effect-free gates (workdir
    /// validation, the worktree request), before anything is minted or
    /// launched, so a resolution failure leaves nothing behind. The registry is
    /// small, so one fetch plus a by-id lookup is cheaper than a query per id.
    ///
    /// A selected id that is no longer registered (a concurrent delete after
    /// the picker rendered) is skipped with a warning rather than failing the
    /// launch, so a stale UI selection cannot kill a spawn.
    ///
    /// Values get a leading `~` expanded here rather than in a provider's
    /// renderer: no shell ever runs over a launch-option value — Claude's
    /// values ride an argv tail, Codex's ride a JSON-RPC field — so a `~/...`
    /// value would otherwise reach the agent literally and be resolved against
    /// its (worktree) cwd as a bogus `<cwd>/~/...` path.
    pub(in crate::interactor) async fn resolve_launch_options(
        &self,
        launch_option_ids: &[i64],
    ) -> Result<Vec<LaunchOptionSpec>> {
        if launch_option_ids.is_empty() {
            return Ok(Vec::new());
        }
        let by_id = self
            .store
            .list_launch_options()
            .await?
            .into_iter()
            .map(|option| (option.id, option))
            .collect::<std::collections::HashMap<_, _>>();
        // Read HOME once for the tilde expansion below.
        let home = std::env::var("HOME").ok().filter(|h| !h.is_empty());
        let mut resolved = Vec::new();
        for id in launch_option_ids {
            match by_id.get(id) {
                Some(option) => resolved.push(LaunchOptionSpec {
                    name: option.name.clone(),
                    value: option
                        .value
                        .as_deref()
                        .map(|value| expand_leading_tilde(value, home.as_deref())),
                }),
                None => tracing::warn!(
                    launch_option_id = id,
                    session_id = %self.id,
                    "selected launch option is no longer registered; skipping it"
                ),
            }
        }
        Ok(resolved)
    }
}
