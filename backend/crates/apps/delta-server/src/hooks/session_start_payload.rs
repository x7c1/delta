//! `SessionStart` payload.

use serde::Deserialize;

/// `SessionStart` payload.
///
/// Claude Code reports the starting session's id, why it started (`source`), and
/// — like every hook payload — its `cwd` and `transcript_path`. The `cwd` and
/// `transcript_path` let a `source=startup` bind register the session row without
/// waiting for the first `UserPromptSubmit`.
#[derive(Debug, Deserialize)]
pub struct SessionStartPayload {
    pub session_id: String,
    pub source: String,
    pub cwd: String,
    pub transcript_path: String,
}
