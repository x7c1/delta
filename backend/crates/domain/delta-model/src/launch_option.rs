//! The launch-option registry.
//!
//! A flat `(label?, name, value?)` record describing one custom startup setting
//! the user has registered for an agent. It is a generic pass-through, and what
//! the pair means is the provider's business:
//!
//! - **Claude** launches a CLI, so `name` is a flag (`--plugin-dir`,
//!   `--permission-mode`, `--model`) and `value` its argument
//!   (`/path/to/plugins`, `auto`, `opus`). A valueless flag carries no `value`,
//!   and a repeatable flag is stored as several separate records.
//! - **Codex** starts a thread over its app-server, so `name` is a
//!   `thread/start` field (`model`, `sandbox`, `config`) and `value` that
//!   field's value (`gpt-5.6-sol`, `read-only`, a JSON object). A record with
//!   no `value` sets a bare boolean field.
//!
//! Delta validates neither the names nor the values: the agent that receives
//! them owns that vocabulary, so a new upstream flag or field works without a
//! Delta change, at the cost of a typo surfacing as an error from the agent.
//! The registry is session-independent — the user manages it once and later
//! multi-selects which options to apply when starting a session.
//!
//! Each option belongs to a single [`AgentProvider`]: Claude's argv flags mean
//! nothing to Codex and vice-versa, so the session-start picker only offers the
//! options registered for the provider the new session will launch on.

use crate::AgentProvider;

/// A registered custom launch option (one `(name, value?)` record).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOption {
    pub id: i64,
    /// An optional human-friendly note for the row.
    pub label: Option<String>,
    /// What the option is called in the provider's own vocabulary — a CLI flag
    /// for Claude (`--plugin-dir`), a `thread/start` field for Codex (`model`).
    pub name: String,
    /// The option's argument/value, e.g. `/path/to/plugins`; `None` for a
    /// valueless option.
    pub value: Option<String>,
    /// Which provider this option applies to. Claude options are argv flags;
    /// other providers register their own option set. Rows that predate
    /// multi-provider support (and any create that omits it) are
    /// [`AgentProvider::Claude`], matching the `launch_option.provider` column
    /// default.
    pub provider: AgentProvider,
    /// Whether this option starts pre-checked in the session-start picker. The
    /// user can still uncheck it in place for an individual session; this only
    /// seeds the initial selection.
    pub default_enabled: bool,
    /// ISO-8601 timestamp.
    pub created_at: String,
}
