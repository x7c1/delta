//! The in-memory registry of live (open) Claude Code panes.
//!
//! Open/closed is **process-runtime** state, never persisted: after a restart
//! the registry is empty, so every session that survives in the store is
//! considered "closed" until it is resumed. The registry maps each open session
//! to the tmux pane driving it, and tracks freshly-spawned panes that do not yet
//! have a Claude `session_id` (they are bound to one by the first
//! `UserPromptSubmit` hook).

use std::collections::HashMap;

use delta_model::SessionId;

use crate::pane_token::PaneToken;

/// A live, bound session: its Claude `session_id` is known and it is mapped to
/// the tmux pane driving it.
#[derive(Debug, Clone)]
pub struct OpenHandle {
    /// The Delta-minted tmux session name.
    pub token: PaneToken,
    /// The pane keystrokes are sent to and the PTY attaches to (`<token>:0.0`).
    pub pane: String,
    /// The working directory the session runs in.
    pub workdir: String,
}

/// A freshly-spawned pane awaiting its first `UserPromptSubmit`.
///
/// A new spawn has no Claude `session_id` yet, so it cannot be keyed by id.
/// It is correlated to its session by matching the hook's `cwd` against
/// [`Self::workdir`] (per-spawn unique workdirs make this exact). If the spawn
/// was initiated by a composer send, [`Self::first_prompt`] carries the text to
/// enqueue once the session id is known.
#[derive(Debug, Clone)]
pub struct PendingSpawn {
    /// The Delta-minted tmux session name.
    pub token: PaneToken,
    /// The pane keystrokes are sent to (`<token>:0.0`).
    pub pane: String,
    /// The unique working directory this spawn runs in; the binding key.
    pub workdir: String,
    /// The deferred first send, if this spawn was initiated by a composer send.
    ///
    /// The `pending_send` row cannot be written before the spawn binds (it
    /// references `session(id)`, which does not exist yet), so the text is held
    /// here and enqueued once the binding supplies the session id.
    pub first_prompt: Option<String>,
}

/// The in-memory map of live panes.
///
/// Held behind a mutex by the interactor. Rebuilt empty on every boot.
#[derive(Debug, Default)]
pub struct OpenSessions {
    /// Sessions whose Claude `session_id` is known, keyed by it.
    bound: HashMap<SessionId, OpenHandle>,
    /// Spawned panes awaiting their first `UserPromptSubmit` to learn the id.
    pending: Vec<PendingSpawn>,
}

impl OpenSessions {
    /// Whether any pane is live: bound to a session, or spawned and awaiting its
    /// first `UserPromptSubmit`. Used to keep the single-session cold start
    /// idempotent.
    pub fn has_any_live(&self) -> bool {
        !self.bound.is_empty() || !self.pending.is_empty()
    }

    /// Whether a session is currently open (bound to a live pane).
    pub fn is_open(&self, id: &SessionId) -> bool {
        self.bound.contains_key(id)
    }

    /// The open handle for a session, if it is currently open.
    pub fn handle(&self, id: &SessionId) -> Option<&OpenHandle> {
        self.bound.get(id)
    }

    /// Bind a freshly-spawned pane to a now-known session id.
    pub fn bind(&mut self, id: SessionId, handle: OpenHandle) {
        self.bound.insert(id, handle);
    }

    /// Record a freshly-spawned pane awaiting its first `UserPromptSubmit`.
    pub fn push_pending(&mut self, spawn: PendingSpawn) {
        self.pending.push(spawn);
    }

    /// Take the oldest pending spawn whose workdir matches `cwd` (FIFO).
    ///
    /// With per-spawn unique workdirs this matches at most one spawn; the FIFO
    /// tie-break is a defensive fallback should two ever share a workdir.
    pub fn take_pending_for_workdir(&mut self, cwd: &str) -> Option<PendingSpawn> {
        let idx = self.pending.iter().position(|p| p.workdir == cwd)?;
        Some(self.pending.remove(idx))
    }

    /// Remove a session from the bound map (closing it), returning its handle.
    pub fn remove(&mut self, id: &SessionId) -> Option<OpenHandle> {
        self.bound.remove(id)
    }

    /// One currently-open session's pane, for the still-single PTY bridge.
    ///
    /// The single-session server has at most one open session, so this returns
    /// that pane. With several open it returns an arbitrary one; the
    /// multi-session transport that lands next routes the PTY by session id
    /// instead and will not rely on this.
    pub fn any_open_pane(&self) -> Option<String> {
        self.bound.values().next().map(|h| h.pane.clone())
    }
}
