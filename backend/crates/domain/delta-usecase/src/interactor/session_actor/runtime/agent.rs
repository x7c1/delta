//! The adapter-backed agent-session plumbing: the push-based content
//! accumulator and the permission-row ↔ provider-token correlation for a
//! terminal-less provider (Codex).

use delta_attribution::Effect;
use delta_model::{Message, ThreadId};

use crate::agent::{AgentContentSource, AgentEvent};

use super::SessionRuntime;

impl SessionRuntime {
    /// Record the correlation between a Delta permission-row id and the
    /// adapter-scoped provider token, for the event pump's `PermissionRequested`
    /// ingestion. The token is stored verbatim and never interpreted.
    pub fn correlate_agent_permission(&mut self, request_id: i64, token: String) {
        self.agent_permission_tokens.insert(request_id, token);
    }

    /// The adapter-scoped provider token correlated with a permission-row id, if
    /// this is an adapter-backed permission awaiting a decision. Read by the
    /// browser-decision path to translate the row id back to the token
    /// [`AgentAdapter::resolve_permission`] answers by; its presence is also what
    /// distinguishes an adapter permission from a Claude (hook-path) one.
    ///
    /// [`AgentAdapter::resolve_permission`]: crate::agent::AgentAdapter::resolve_permission
    pub fn agent_permission_token(&self, request_id: i64) -> Option<&str> {
        self.agent_permission_tokens
            .get(&request_id)
            .map(String::as_str)
    }

    /// Resolve an adapter-scoped provider token back to its permission-row id,
    /// removing the correlation. The event pump calls this when the adapter
    /// emits a `PermissionResolved` (which carries the provider token) so it can
    /// route the resolution to the reducer under the `i64` id the runtime mirror
    /// and browser speak. `None` when the token is unknown (already resolved).
    pub fn resolve_agent_permission_token(&mut self, token: &str) -> Option<i64> {
        let request_id = self
            .agent_permission_tokens
            .iter()
            .find(|(_, t)| t.as_str() == token)
            .map(|(id, _)| *id)?;
        self.agent_permission_tokens.remove(&request_id);
        Some(request_id)
    }

    /// Install the push-based content accumulator for the open agent session.
    ///
    /// Called at Codex spawn with the source the adapter built. Replaces any
    /// previous one (a fresh open builds a fresh accumulator seeded from the
    /// store's current sequence).
    pub fn set_agent_content_source(&mut self, source: Box<dyn AgentContentSource>) {
        self.agent_content_source = Some(source);
    }

    /// Set the per-turn routing context on the session's content accumulator,
    /// before the turn's frames arrive through the pump.
    ///
    /// Forwards to [`AgentContentSource::begin_turn`]: the turn's messages land on
    /// `thread_id` (the branch child thread for a branch send, `main` otherwise)
    /// and, for a branch, the root user message is stamped with `semantic_parent`
    /// — so a Codex branch turn's content follows the same lane the `send` row
    /// records, instead of every message falling back onto `main`. A no-op when
    /// the session has no accumulator (not a Codex session), so the Claude path is
    /// untouched.
    ///
    /// [`AgentContentSource::begin_turn`]: crate::agent::AgentContentSource::begin_turn
    pub fn begin_agent_turn(
        &mut self,
        thread_id: ThreadId,
        semantic_parent: Option<delta_model::MessageUuid>,
    ) {
        if let Some(source) = self.agent_content_source.as_mut() {
            source.begin_turn(thread_id, semantic_parent);
        }
    }

    /// Fold one neutral [`AgentEvent`] through the session's content accumulator,
    /// returning the canonical content it completed — the messages plus the
    /// ordered [`Effect`]s the persistence pipeline must run. `None` when the
    /// session has no accumulator (not a Codex session, or already closed), so
    /// the caller skips the persistence step entirely.
    pub fn fold_agent_content(
        &mut self,
        event: &AgentEvent,
    ) -> Option<(Vec<Message>, Vec<Effect>)> {
        self.agent_content_source
            .as_mut()
            .map(|source| source.ingest(event))
    }
}
