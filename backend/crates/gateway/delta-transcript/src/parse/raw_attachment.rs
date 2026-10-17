//! The `attachment` field of a transcript line.

use serde::Deserialize;

/// A Claude Code `attachment` payload. Delta only models the `queued_command`
/// variant, which carries a prompt delivered while a turn was in flight; all
/// other attachment kinds are ignored (their `prompt` is never read).
///
/// The `queued_command` shape has two uses, only one of them legacy:
///
/// - **Human-typed queued prompts (legacy).** Older claude versions recorded a
///   prompt the user typed mid-turn only as this attachment. Current claude
///   writes a uuid-less `queue-operation` line instead and replays the prompt
///   as a plain user line (see the queued-prompt drift note in
///   docs/guides/development/canary.md).
/// - **Harness injections (current).** Current claude still writes this
///   attachment for content the harness delivers mid-turn: background-task
///   `<task-notification>`s (`commandMode: "task-notification"`, no `isMeta`)
///   and subagent hand-backs (`commandMode: "prompt"`, `isMeta: true`, a peer
///   `origin`).
#[derive(Debug, Deserialize)]
pub(super) struct RawAttachment {
    #[serde(rename = "type")]
    pub attachment_type: Option<String>,
    pub prompt: Option<String>,
    /// The attachment's own `isMeta` flag (distinct from the line-level one).
    /// Claude sets it on a harness injection that is not human-authored, such
    /// as a mid-turn subagent hand-back; the parser classifies such a
    /// `queued_command` as `Role::Meta` instead of a user prompt.
    #[serde(rename = "isMeta")]
    pub is_meta: Option<bool>,
}
