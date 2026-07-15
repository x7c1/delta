//! The subset of a transcript line Delta reads.

use serde::Deserialize;

use super::raw_attachment::RawAttachment;
use super::raw_content::RawContent;
use super::raw_message::RawMessage;

/// The subset of a transcript line Delta reads. Unknown fields are ignored.
#[derive(Debug, Deserialize)]
pub(super) struct RawLine {
    pub uuid: Option<String>,
    #[serde(rename = "parentUuid")]
    pub parent_uuid: Option<String>,
    #[serde(rename = "type")]
    pub line_type: Option<String>,
    /// Discriminates a `type: "system"` line; `turn_duration` is the one Delta
    /// reads (it carries the turn's latency).
    pub subtype: Option<String>,
    #[serde(rename = "promptId")]
    pub prompt_id: Option<String>,
    pub timestamp: Option<String>,
    pub message: Option<RawMessage>,
    /// Top-level `content` present on a `type: "system"` / `subtype: "local_command"`
    /// line — a slash/local command's captured `<local-command-stdout>` /
    /// `<local-command-stderr>` output. The legacy shape put this in
    /// `message.content` on a `type: "user"` line instead; this is the current
    /// Claude Code shape, which carries no embedded `message`.
    pub content: Option<RawContent>,
    /// Top-level working directory at this turn. Effectively fixed per session.
    pub cwd: Option<String>,
    /// Top-level git branch at this turn. Can change mid-session.
    #[serde(rename = "gitBranch")]
    pub git_branch: Option<String>,
    /// Present on a `system`/`turn_duration` line: the turn's latency in
    /// milliseconds. An `f64` because the wire value is a JSON number.
    #[serde(rename = "durationMs")]
    pub duration_ms: Option<f64>,
    /// Present on `type: "attachment"` lines; carries a queued command's prompt.
    pub attachment: Option<RawAttachment>,
    /// Set on harness-injected lines (skill bodies, system reminders) that
    /// Claude records as `type: "user"` but are not human-authored turns.
    /// Drives [`Role::Meta`] classification. Note: the current Claude Code
    /// shape records local-command output as a `type: "system"` /
    /// `subtype: "local_command"` line carrying a top-level [`content`] instead
    /// (see that field); the legacy shape recorded it as `type: "user"`.
    ///
    /// [`content`]: RawLine::content
    #[serde(rename = "isMeta")]
    pub is_meta: Option<bool>,
    /// Set on a synthetic assistant line Claude Code writes when a turn ends on
    /// an API error (a usage/session limit, a rate limit, or any other API
    /// failure) instead of completing normally. Such a turn-end fires no `Stop`
    /// hook and writes no interrupt marker, so this flag is the only signal that
    /// the turn ended; it drives the transcript-driven turn-end fallback.
    #[serde(rename = "isApiErrorMessage")]
    pub is_api_error_message: Option<bool>,
    /// Set on the synthetic user line Claude Code writes when `/compact` runs,
    /// carrying the previous-conversation summary. Not a human-authored turn;
    /// classified as [`Role::CompactSummary`] so attribution skips it (never
    /// matches an outstanding send, never resets `carry_thread`).
    #[serde(rename = "isCompactSummary")]
    pub is_compact_summary: Option<bool>,
    /// The provenance tag Claude Code stamps on the replay of a prompt the
    /// user submitted while a turn was in flight: the CLI buffers it in its
    /// internal input queue and, when the queue drains, writes the buffered
    /// prompt back as a plain `type: "user"` line with `promptSource:
    /// "queued"`. Other user lines omit the field or carry a non-`"queued"`
    /// value (e.g. `"cli"`). Attribution reads this to keep a post-compact
    /// queued replay out of the local-command group it shares a `promptId`
    /// with (see the `is_queued_replay` guard in `attribute.rs`).
    #[serde(rename = "promptSource")]
    pub prompt_source: Option<String>,
}
