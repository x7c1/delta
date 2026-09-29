//! [`LaunchOptionCardinality`]: whether a launch-option name takes one value
//! per session or may repeat.

/// How many values one launch-option `name` can take in a single session, read
/// in the provider's vocabulary (see [`super::LaunchOptionVocabulary`]).
///
/// Registry rows that share a `(provider, name)` are candidate values of one
/// setting; the cardinality says whether a session may select several of them
/// at once or at most one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchOptionCardinality {
    /// The setting takes one value: rows sharing the name are mutually
    /// exclusive choices (Claude's `--model`, any Codex `thread/start` field but
    /// `config`). At most one of them may be selected, and at most one may be
    /// default-enabled.
    Single,
    /// The setting may repeat: every row sharing the name is an independent
    /// option (Claude's `--add-dir`, Codex's deep-merged `config`).
    Multiple,
}
