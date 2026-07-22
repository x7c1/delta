//! The launch-option registry.
//!
//! A flat `(label?, name, value?)` record describing one custom `claude` CLI
//! flag the user has registered. It is a generic flag pass-through: `name` is
//! the flag (e.g. `--plugin-dir`, `--permission-mode`, `--model`) and `value`
//! is its argument (e.g. `/path/to/plugins`, `auto`, `opus`). `value` is
//! optional, for valueless flags; a repeatable flag is stored as several
//! separate records. The registry is session-independent — the user manages it
//! once and later multi-selects which options to apply when starting a session.
//!
//! Each option belongs to a single [`AgentProvider`]: Claude's argv flags mean
//! nothing to Codex and vice-versa, so the session-start picker only offers the
//! options registered for the provider the new session will launch on.

use crate::AgentProvider;

/// A registered custom launch option (one `claude` CLI flag record).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOption {
    pub id: i64,
    /// An optional human-friendly note for the row.
    pub label: Option<String>,
    /// The flag itself, e.g. `--plugin-dir`.
    pub name: String,
    /// The flag's argument, e.g. `/path/to/plugins`; `None` for a valueless flag.
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
