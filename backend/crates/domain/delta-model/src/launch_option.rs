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
//! Most rows are the user's own. A row whose `builtin_key` is set is one Delta
//! *ships* — materialized from a [`crate::LaunchOptionPreset`] at startup so
//! the short list of combinations in daily use is already there — and its
//! `label`, `name` and `value` belong to the declared catalog rather than to
//! the user. It is otherwise an ordinary row: an ordinary id that flows through
//! the picker and the launch path unchanged, and a `default_enabled` flag only
//! the user sets.
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
    ///
    /// On a shipped row ([`Self::builtin_key`] non-null) this is the one field
    /// that stays entirely the user's business: reconciliation never touches it.
    pub default_enabled: bool,
    /// ISO-8601 timestamp.
    pub created_at: String,
    /// `None` for a row the user registered; `Some(key)` for one Delta ships
    /// (see [`crate::LaunchOptionPreset`]).
    ///
    /// It is both the marker and the reconciliation key. A non-null row's
    /// [`Self::label`], [`Self::name`] and [`Self::value`] are owned by the
    /// declared catalog — startup reconciliation overwrites them from the
    /// preset this key names — which is harmless because the REST layer cannot
    /// edit those three anyway (`PATCH` carries only `default_enabled`). The
    /// API also refuses to delete such a row, and the UI badges it.
    pub builtin_key: Option<String>,
}
