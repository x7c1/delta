//! The launch-option registry.
//!
//! A flat `(label?, name, value?)` record describing one custom `claude` CLI
//! flag the user has registered. It is a generic flag pass-through: `name` is
//! the flag (e.g. `--plugin-dir`, `--permission-mode`, `--model`) and `value`
//! is its argument (e.g. `/path/to/plugins`, `auto`, `opus`). `value` is
//! optional, for valueless flags; a repeatable flag is stored as several
//! separate records. The registry is session-independent — the user manages it
//! once and later multi-selects which options to apply when starting a session.

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
    /// ISO-8601 timestamp.
    pub created_at: String,
}
