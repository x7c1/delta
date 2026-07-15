//! Parsing a single JSONL transcript line into a [`TranscriptMessage`].

mod raw_attachment;
mod raw_content;
mod raw_content_block;
mod raw_line;
mod raw_message;

use delta_model::{ContentBlock, MessageUuid, PromptId, Role};
use delta_usecase::TranscriptMessage;

use raw_content::RawContent;
use raw_line::RawLine;

/// What a single transcript line parses into.
///
/// Most lines yield a [`TranscriptMessage`]; a `system`/`turn_duration` line
/// yields no message but carries the turn's latency, which the reader correlates
/// back onto that turn's assistant message (see [`correlate_turn_durations`]).
/// A blank/no-uuid/duration-less line yields [`ParsedLine::Skip`].
#[derive(Debug)]
pub(crate) enum ParsedLine {
    /// A real message line. Boxed because a [`TranscriptMessage`] is far larger
    /// than the other variants, which would otherwise bloat every `ParsedLine`.
    Message(Box<TranscriptMessage>),
    /// A `system`/`turn_duration` line: the turn's response time, to be
    /// correlated onto the turn's assistant message.
    TurnDuration { duration_ms: f64 },
    /// A line that produces nothing (blank, no uuid, or no usable payload).
    Skip,
}

/// Markers Claude Code writes at the start of a slash/local command's captured
/// output, recorded as a `type: "user"` line WITHOUT `isMeta`. These structural
/// prefixes let the parser fold the output to [`Role::Meta`] (like the group's
/// caveat) rather than rendering it as a human user turn.
const LOCAL_COMMAND_OUTPUT_MARKERS: [&str; 2] =
    ["<local-command-stdout>", "<local-command-stderr>"];

/// Whether a user line's leading text is a local command's captured output.
fn is_local_command_output_marker(text: &str) -> bool {
    LOCAL_COMMAND_OUTPUT_MARKERS
        .iter()
        .any(|marker| text.starts_with(marker))
}

/// Parse one JSONL line into a message, ignoring non-message outcomes.
///
/// Thin wrapper over [`parse_line_outcome`] kept for callers (and tests) that
/// only care about message lines: a `turn_duration` system line — which carries
/// no message — collapses to `Ok(None)` here, exactly like a blank or no-uuid
/// line. The reader uses [`parse_line_outcome`] directly so it can correlate a
/// turn's duration onto its assistant message.
pub fn parse_line(line: &str) -> Result<Option<TranscriptMessage>, serde_json::Error> {
    Ok(match parse_line_outcome(line)? {
        ParsedLine::Message(msg) => Some(*msg),
        ParsedLine::TurnDuration { .. } | ParsedLine::Skip => None,
    })
}

/// Parse one JSONL line into a [`ParsedLine`].
pub(crate) fn parse_line_outcome(line: &str) -> Result<ParsedLine, serde_json::Error> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(ParsedLine::Skip);
    }
    let raw: RawLine = serde_json::from_str(trimmed)?;

    // A `system`/`turn_duration` line carries the turn's latency but no message.
    // Surface it as its own outcome so the reader can correlate the duration
    // onto the turn's assistant message (it itself never becomes a message).
    if raw.line_type.as_deref() == Some("system") && raw.subtype.as_deref() == Some("turn_duration")
    {
        return Ok(match raw.duration_ms {
            Some(duration_ms) => ParsedLine::TurnDuration { duration_ms },
            None => ParsedLine::Skip,
        });
    }

    // A line without a uuid is not a message we can address; skip it. This
    // deliberately covers `type: "queue-operation"` lines — the uuid-less
    // bookkeeping records current claude writes when a prompt is submitted
    // mid-turn. The queued prompt's real message is the plain `type: "user"`
    // line claude replays at dequeue (which fires its own `UserPromptSubmit`
    // and flows the normal parse/attribution path), so the bookkeeping line
    // carries nothing Delta needs to surface.
    let Some(uuid) = raw.uuid else {
        return Ok(ParsedLine::Skip);
    };

    // LEGACY FORMAT COMPATIBILITY — keep this path. Older claude versions
    // recorded a prompt composed while a turn was in flight ONLY as a
    // `queued_command` attachment line — never as a normal `type: "user"`
    // line — so without special handling it parses as a contentless
    // `Role::Other` line: invisible in the transcript and uncorrelatable to
    // its queued send, which drops the whole turn (prompt and reply) onto
    // `main`. Surface it as a user message carrying the queued prompt text so
    // it both displays and flows through send correlation. Current claude
    // writes a `queue-operation` line instead and replays the prompt as a
    // plain user line (see the queued-prompt drift note in
    // docs/guides/development.md), but transcripts recorded by older versions
    // are still resumed and viewed.
    let queued_prompt = raw
        .attachment
        .as_ref()
        .filter(|a| a.attachment_type.as_deref() == Some("queued_command"))
        .and_then(|a| a.prompt.clone());
    let is_queued_command = queued_prompt.is_some();

    // `isMeta` lines are harness-injected (skill bodies, system reminders,
    // local-command output) recorded as `type: "user"` but not human-authored.
    // Read it before `raw.message` is moved out below.
    let is_meta = raw.is_meta == Some(true);

    // `isCompactSummary` marks the synthetic `/compact` summary line; see the
    // field docstring on [`RawLine`] for why it must classify away from `User`.
    let is_compact_summary = raw.is_compact_summary == Some(true);

    // The current Claude Code shape records a slash/local command's captured
    // output as a `type: "system"` / `subtype: "local_command"` line whose
    // payload is a TOP-LEVEL `content` string (no embedded `message`, no
    // `promptId`). The legacy shape recorded it as a `type: "user"` line with
    // the payload in `message.content`. Detect the current shape by subtype.
    let is_local_command_subtype = raw.line_type.as_deref() == Some("system")
        && raw.subtype.as_deref() == Some("local_command");

    // The model that produced this line lives on the embedded message
    // (`message.model`), present on assistant lines only. Take it before the
    // message is moved out for content below.
    let model = raw.message.as_ref().and_then(|m| m.model.clone());

    // Resolve the line's effective content: prefer the embedded
    // `message.content`, and fall back to the top-level `content` ONLY for the
    // local_command subtype. Other `type: "system"` subtypes (`away_summary`,
    // `informational`, `scheduled_task_fire`, …) carry a top-level `content`
    // Delta does not render, so they keep producing empty content, as today.
    let message_content = raw.message.and_then(|m| m.content);
    let effective_content = message_content.or(if is_local_command_subtype {
        raw.content
    } else {
        None
    });

    // A slash/local command (e.g. `/review-pr`) records its captured output
    // WITHOUT `isMeta` — only the leading caveat line of the group is flagged.
    // So fold to `Role::Meta` when either the current-shape subtype matches or
    // the content's leading token is a `<local-command-stdout>` /
    // `<local-command-stderr>` marker (the legacy shape), matching the caveat
    // instead of rendering it as a human user turn. (The bare command-name line
    // of the same group carries no marker; it is folded by the attribution
    // layer, which groups it by the caveat's `promptId`.) Detected here, at
    // line-classification time, so the fold is robust even in a sync window that
    // did not include the caveat line.
    let effective_leading_text = match &effective_content {
        Some(RawContent::Text(text)) => Some(text.as_str()),
        Some(RawContent::Blocks(_)) | None => None,
    };
    let is_local_command_output = is_local_command_subtype
        || effective_leading_text
            .is_some_and(|text| is_local_command_output_marker(text.trim_start()));

    // `isApiErrorMessage` marks a synthetic assistant line Claude writes when a
    // turn ends on an API error (usage/session limit, rate limit, any API
    // failure) instead of completing normally. Such a turn-end fires no `Stop`
    // hook and writes no interrupt marker, so the fold keys on this flag to feed
    // the turn machine back to idle. Default `false` when the field is absent.
    let is_api_error = raw.is_api_error_message == Some(true);

    // `promptSource: "queued"` marks the replay of a prompt the CLI buffered
    // while a turn was in flight, later written back as an ordinary
    // `type: "user"` line. Attribution keys on this to keep a post-compact
    // queued replay out of the local-command group it shares a `promptId`
    // with (see the `is_queued_replay` guard in `attribute.rs`). Independent
    // of `is_queued_command`, which flags the LEGACY attachment shape.
    let is_queued_replay = raw.prompt_source.as_deref() == Some("queued");

    let role = if is_compact_summary {
        Role::CompactSummary
    } else if is_queued_command {
        Role::User
    } else if is_meta || is_local_command_output {
        Role::Meta
    } else {
        raw.line_type
            .as_deref()
            .map(Role::from_transcript_type)
            .unwrap_or(Role::Other)
    };

    let content = if let Some(prompt) = queued_prompt {
        vec![ContentBlock::Text { text: prompt }]
    } else {
        match effective_content {
            Some(RawContent::Text(text)) => vec![ContentBlock::Text { text }],
            Some(RawContent::Blocks(blocks)) => {
                blocks.into_iter().map(ContentBlock::from).collect()
            }
            None => Vec::new(),
        }
    };

    Ok(ParsedLine::Message(Box::new(TranscriptMessage {
        uuid: MessageUuid::from(uuid),
        role,
        linear_parent_uuid: raw.parent_uuid.map(MessageUuid::from),
        prompt_id: raw.prompt_id.map(PromptId::from),
        content,
        created_at: raw.timestamp,
        // The reader assigns the real line index; a standalone parse defaults to
        // 0 since it has no file position.
        seq: 0,
        is_queued_command,
        is_queued_replay,
        is_api_error,
        model,
        git_branch: raw.git_branch,
        cwd: raw.cwd,
        // The duration arrives on a separate `turn_duration` line; the reader
        // correlates it onto this message afterwards (see the reader).
        response_time_ms: None,
    })))
}

/// Correlate each turn's `turn_duration` onto its assistant message.
///
/// A `system`/`turn_duration` line carries the turn's latency but no message; it
/// is written right after the turn's final assistant line (the chain is
/// `assistant → stop_hook_summary → turn_duration`). So a duration is attributed
/// to the **most recent assistant message** that precedes it in file order. The
/// `outcomes` slice is the per-line parse results in file order; this back-fills
/// `response_time_ms` on the matching assistant message in place.
///
/// Lives here (not in the reader) so it is unit-testable over a fixture of raw
/// lines, and so the file-position/seq bookkeeping in the reader stays simple.
pub(crate) fn correlate_turn_durations(outcomes: &mut [ParsedLine]) {
    for idx in 0..outcomes.len() {
        let ParsedLine::TurnDuration { duration_ms } = outcomes[idx] else {
            continue;
        };
        // Walk back to the nearest preceding assistant message and stamp it.
        for prior in outcomes[..idx].iter_mut().rev() {
            if let ParsedLine::Message(msg) = prior {
                if msg.role == Role::Assistant {
                    msg.response_time_ms = Some(duration_ms);
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_line_with_string_content() {
        let line = r#"{"uuid":"u1","parentUuid":null,"type":"user","promptId":"p1","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"hello"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.uuid, MessageUuid::from("u1"));
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.linear_parent_uuid, None);
        assert_eq!(msg.prompt_id, Some(PromptId::from("p1")));
        assert_eq!(msg.flatten_text().as_deref(), Some("hello"));
    }

    #[test]
    fn parses_assistant_line_with_block_content() {
        let line = r#"{"uuid":"a1","parentUuid":"u1","type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"hi"},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.linear_parent_uuid, Some(MessageUuid::from("u1")));
        assert_eq!(msg.content.len(), 3);
        assert_eq!(msg.flatten_text().as_deref(), Some("hmm\nhi"));
    }

    #[test]
    fn unknown_content_block_kind_parses_as_explicit_other() {
        // Unknown block kinds must not fail the parse: they surface as the
        // domain's explicit `Other` variant while known siblings still parse.
        let line = r#"{"uuid":"a2","type":"assistant","message":{"role":"assistant","content":[{"type":"image","source":{"x":1}},{"type":"text","text":"hi"}]}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.content[0], ContentBlock::Other);
        assert_eq!(msg.flatten_text().as_deref(), Some("hi"));
    }

    #[test]
    fn queue_operation_line_is_deliberately_skipped() {
        // Current claude's bookkeeping record for a prompt submitted mid-turn:
        // uuid-less, so it must be skipped (not choked on or misclassified).
        // The prompt's real message is the plain user line replayed at dequeue.
        let line = r#"{"type":"queue-operation","operation":"enqueue","content":"Reply with only the word: ok","sessionId":"s1","timestamp":"2026-01-01T00:00:00Z"}"#;
        assert!(parse_line(line).unwrap().is_none());
    }

    #[test]
    fn dequeued_user_line_parses_as_a_plain_user_message() {
        // The replay current claude writes when a queued prompt dequeues: an
        // ordinary `type: "user"` line carrying a `promptSource: "queued"`
        // provenance field. The parse surfaces it as a plain user line (it
        // must not perturb `role` or `flatten_text`) and lifts the provenance
        // to the new `is_queued_replay` flag, which attribution reads to keep
        // the replay out of a local-command group when the queue drains right
        // after a `/compact`. `is_queued_command` — the LEGACY attachment
        // shape's flag — stays `false`; the two flags are distinct.
        let line = r#"{"uuid":"u9","parentUuid":"m8","type":"user","promptSource":"queued","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"Reply with only the word: ok"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.role, Role::User);
        assert_eq!(
            msg.flatten_text().as_deref(),
            Some("Reply with only the word: ok")
        );
        assert!(msg.is_queued_replay);
        assert!(!msg.is_queued_command);
    }

    #[test]
    fn ordinary_user_line_is_not_flagged_queued_replay() {
        // A user line with no `promptSource` field must leave `is_queued_replay`
        // false: the flag is opt-in on the modern replay shape only.
        let line = r#"{"uuid":"u1","type":"user","message":{"role":"user","content":"hi"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert!(!msg.is_queued_replay);
    }

    #[test]
    fn non_queued_prompt_source_leaves_the_replay_flag_false() {
        // A user line whose `promptSource` is anything other than `"queued"`
        // (here `"cli"`, the ordinary interactive-submit provenance) is NOT a
        // buffered-queue replay and must not set the flag.
        let line = r#"{"uuid":"u1","type":"user","promptSource":"cli","message":{"role":"user","content":"hi"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert!(!msg.is_queued_replay);
    }

    #[test]
    fn legacy_queued_command_attachment_parses_as_user_prompt() {
        // LEGACY FORMAT: older claude versions recorded a prompt queued while
        // a turn was in flight only as this attachment, with no `message`
        // content. It must surface as a user message carrying the queued
        // prompt so old transcripts still display and correlate.
        let line = r#"{"uuid":"q1","parentUuid":"a0","type":"attachment","timestamp":"2026-01-01T00:00:00Z","attachment":{"type":"queued_command","prompt":"queued while the turn was busy","commandMode":"prompt"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.uuid, MessageUuid::from("q1"));
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.linear_parent_uuid, Some(MessageUuid::from("a0")));
        assert_eq!(
            msg.flatten_text().as_deref(),
            Some("queued while the turn was busy")
        );
        assert!(msg.is_queued_command);
        // Orthogonality pin: the LEGACY attachment shape sets ONLY
        // `is_queued_command`; the MODERN `promptSource: "queued"` replay flag
        // must stay false so downstream fold logic (compact-group exclusion)
        // keys off the correct provenance.
        assert!(!msg.is_queued_replay);
    }

    #[test]
    fn non_queued_attachment_is_inert_other_line() {
        // An attachment that is not a queued command carries no prompt: it stays
        // a contentless `Other` line and is not flagged as a queued command.
        let line = r#"{"uuid":"x1","type":"attachment","attachment":{"type":"image","path":"/tmp/a.png"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.role, Role::Other);
        assert!(msg.content.is_empty());
        assert!(!msg.is_queued_command);
    }

    #[test]
    fn ordinary_user_line_is_not_flagged_queued() {
        let line = r#"{"uuid":"u1","type":"user","message":{"role":"user","content":"hi"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert!(!msg.is_queued_command);
    }

    #[test]
    fn meta_user_line_parses_as_meta_role() {
        // A harness-injected line: recorded as `type: "user"` but flagged
        // `isMeta`. It must classify as `Role::Meta`, not a human turn.
        let line = r#"{"uuid":"m1","type":"user","isMeta":true,"message":{"role":"user","content":"<system-reminder>injected body</system-reminder>"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.role, Role::Meta);
        assert_eq!(
            msg.flatten_text().as_deref(),
            Some("<system-reminder>injected body</system-reminder>")
        );
        assert!(!msg.is_queued_command);
    }

    #[test]
    fn ordinary_user_line_without_meta_is_user_role() {
        let line = r#"{"uuid":"u3","type":"user","message":{"role":"user","content":"hi"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.role, Role::User);
    }

    #[test]
    fn compact_summary_user_line_parses_as_compact_summary_role() {
        // When `/compact` runs Claude Code writes a single synthetic line as a
        // `type: "user"` record carrying `isCompactSummary: true` with the
        // previous-conversation summary. It is not a human turn: classify it as
        // `Role::CompactSummary` so attribution does not match it against an
        // outstanding send (which would wedge the send forever) nor reset
        // `carry_thread` to main (which would corrupt thread attribution).
        let line = r#"{"uuid":"cs1","type":"user","isCompactSummary":true,"message":{"role":"user","content":"<summary of the previous conversation>"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.role, Role::CompactSummary);
        assert_eq!(
            msg.flatten_text().as_deref(),
            Some("<summary of the previous conversation>")
        );
        assert!(!msg.is_queued_command);
    }

    #[test]
    fn local_command_stdout_line_is_meta_even_without_is_meta_flag() {
        // A slash/local command (e.g. `/review-pr`) records its captured output
        // as a `type: "user"` line WITHOUT `isMeta`. It is command machinery, so
        // it must fold as `Role::Meta`, not render as a human user turn.
        let line = r#"{"uuid":"o1","type":"user","promptId":"p1","message":{"role":"user","content":"<local-command-stdout>\nPENDING review created.\n</local-command-stdout>"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.role, Role::Meta);
        assert!(!msg.is_queued_command);
    }

    #[test]
    fn current_shape_local_command_system_line_folds_to_meta_and_surfaces_content() {
        // CURRENT SHAPE (Claude Code ~v2.1.199): a slash/local command records
        // its captured output as a single `type: "system"` /
        // `subtype: "local_command"` line whose payload is a TOP-LEVEL `content`
        // string — no embedded `message`, no `promptId`. It must fold to
        // `Role::Meta` AND surface that content (the dropped-content bug: the
        // parser previously only read `message.content`, so this rendered as
        // nothing).
        let line = r#"{"uuid":"s1","type":"system","subtype":"local_command","content":"<local-command-stdout>\nPENDING review created.\n</local-command-stdout>","level":"info","isMeta":false}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.role, Role::Meta);
        assert!(
            msg.flatten_text()
                .as_deref()
                .is_some_and(|text| text.contains("PENDING review created.")),
            "the local_command line's top-level content must be surfaced, not dropped"
        );
    }

    #[test]
    fn non_local_command_system_subtype_with_top_level_content_stays_contentless() {
        // Guard against surfacing noise: a `type: "system"` line of some OTHER
        // subtype (e.g. `away_summary`) also carries a top-level `content`, but
        // Delta does not render it. Only the `local_command` subtype falls back
        // to the top-level `content`; every other system subtype keeps producing
        // empty content and rendering nothing, exactly as before.
        let line = r#"{"uuid":"s2","type":"system","subtype":"away_summary","content":"you were away","level":"info"}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.role, Role::System);
        assert!(msg.content.is_empty());
        assert_eq!(msg.flatten_text(), None);
    }

    #[test]
    fn local_command_stderr_line_is_meta_even_without_is_meta_flag() {
        let line = r#"{"uuid":"o2","type":"user","promptId":"p1","message":{"role":"user","content":"<local-command-stderr>boom</local-command-stderr>"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.role, Role::Meta);
    }

    #[test]
    fn bare_command_name_line_stays_user_at_parse_time() {
        // The command-name member of a local-command group (e.g. `/review-pr`)
        // carries no structural marker, so the single-line parser cannot tell it
        // from a human prompt. It is folded by the attribution layer instead,
        // which groups it by the caveat's shared `promptId`. Pinning it as
        // `Role::User` here guards against a future content-sniff that would
        // misclassify a human prompt literally beginning with a slash.
        let line = r#"{"uuid":"c1","type":"user","promptId":"p1","message":{"role":"user","content":"/review-pr"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.role, Role::User);
    }

    #[test]
    fn user_line_merely_mentioning_local_command_markup_stays_user() {
        // A human prompt that contains the marker text mid-line (not as the
        // leading token) is a genuine turn and must not be folded.
        let line = r#"{"uuid":"u4","type":"user","message":{"role":"user","content":"why did <local-command-stdout> appear?"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.role, Role::User);
    }

    #[test]
    fn api_error_assistant_line_is_flagged_is_api_error() {
        // A turn that ends on a usage/session limit (or any API error) is
        // written as a synthetic assistant line carrying `isApiErrorMessage`.
        // It fires no `Stop` hook and writes no interrupt marker, so the flag
        // is the only turn-end signal — it must surface on the parsed line.
        let line = r#"{"uuid":"e1","type":"assistant","isApiErrorMessage":true,"error":"rate_limit","apiErrorStatus":429,"message":{"role":"assistant","model":"<synthetic>","stop_reason":"stop_sequence","content":[{"type":"text","text":"You've hit your session limit"}]}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.role, Role::Assistant);
        assert!(msg.is_api_error);
        assert_eq!(
            msg.flatten_text().as_deref(),
            Some("You've hit your session limit")
        );
    }

    #[test]
    fn ordinary_assistant_line_is_not_flagged_api_error() {
        let line = r#"{"uuid":"a3","type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert!(!msg.is_api_error);
    }

    #[test]
    fn unknown_line_type_parses_as_other() {
        let line = r#"{"uuid":"s1","type":"summary","summary":"x"}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.role, Role::Other);
        assert!(msg.content.is_empty());
    }

    #[test]
    fn unknown_top_level_fields_are_ignored() {
        let line = r#"{"uuid":"u2","type":"user","extra":123,"cwd":"/x","message":{"content":"hi","role":"user"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.uuid, MessageUuid::from("u2"));
    }

    #[test]
    fn blank_line_yields_none() {
        assert!(parse_line("   ").unwrap().is_none());
    }

    #[test]
    fn line_without_uuid_is_skipped() {
        let line = r#"{"type":"user","message":{"content":"hi"}}"#;
        assert!(parse_line(line).unwrap().is_none());
    }

    #[test]
    fn assistant_line_extracts_model_cwd_and_git_branch() {
        // The model lives on the embedded message; cwd/gitBranch are top-level.
        let line = r#"{"uuid":"a1","type":"assistant","cwd":"/repo","gitBranch":"feature/x","message":{"role":"assistant","model":"claude-opus-4-8","content":[{"type":"text","text":"hi"}]}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.model.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(msg.cwd.as_deref(), Some("/repo"));
        assert_eq!(msg.git_branch.as_deref(), Some("feature/x"));
        // The duration arrives on a separate line; it is not set here.
        assert_eq!(msg.response_time_ms, None);
    }

    #[test]
    fn user_line_has_no_model_and_metadata_is_optional() {
        // A user line carries no model; absent cwd/gitBranch stay None.
        let line = r#"{"uuid":"u1","type":"user","message":{"role":"user","content":"hi"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.model, None);
        assert_eq!(msg.cwd, None);
        assert_eq!(msg.git_branch, None);
    }

    #[test]
    fn turn_duration_system_line_yields_no_message() {
        // The `turn_duration` system line carries the turn's latency but is not
        // itself a message: `parse_line` (message-only) drops it, while the
        // outcome form surfaces the duration for correlation.
        let line = r#"{"uuid":"d1","type":"system","subtype":"turn_duration","durationMs":4221,"timestamp":"2026-01-01T00:00:00Z"}"#;
        assert!(parse_line(line).unwrap().is_none());
        match parse_line_outcome(line).unwrap() {
            ParsedLine::TurnDuration { duration_ms } => assert_eq!(duration_ms, 4221.0),
            other => panic!("expected TurnDuration, got {other:?}"),
        }
    }

    #[test]
    fn turn_duration_is_correlated_onto_its_turn_assistant_message() {
        // A realistic turn tail: the user prompt, the turn's assistant reply, a
        // `stop_hook_summary` system line, then the `turn_duration` line whose
        // latency must attach to that assistant message — not the user line and
        // not the system lines. Mirrors claude's real
        // `assistant → stop_hook_summary → turn_duration` ordering.
        let lines = [
            r#"{"uuid":"u1","type":"user","message":{"role":"user","content":"q"}}"#,
            r#"{"uuid":"a1","type":"assistant","message":{"role":"assistant","model":"claude-opus-4-8","content":[{"type":"text","text":"answer"}]}}"#,
            r#"{"uuid":"s1","type":"system","subtype":"stop_hook_summary","message":{"role":"system","content":"x"}}"#,
            r#"{"uuid":"d1","type":"system","subtype":"turn_duration","durationMs":10167}"#,
        ];
        let mut outcomes: Vec<ParsedLine> = lines
            .iter()
            .map(|l| parse_line_outcome(l).unwrap())
            .collect();
        correlate_turn_durations(&mut outcomes);

        let messages: Vec<&TranscriptMessage> = outcomes
            .iter()
            .filter_map(|o| match o {
                ParsedLine::Message(m) => Some(m.as_ref()),
                _ => None,
            })
            .collect();
        let user = messages
            .iter()
            .find(|m| m.uuid == MessageUuid::from("u1"))
            .unwrap();
        let assistant = messages
            .iter()
            .find(|m| m.uuid == MessageUuid::from("a1"))
            .unwrap();
        assert_eq!(
            assistant.response_time_ms,
            Some(10167.0),
            "duration attaches to the turn's assistant message"
        );
        assert_eq!(
            user.response_time_ms, None,
            "the user line is not the turn's assistant message"
        );
    }

    #[test]
    fn each_turns_duration_attaches_to_its_own_turns_assistant() {
        // Two complete turns in one window: each `turn_duration` must stamp the
        // assistant of its OWN turn (the nearest preceding assistant), never
        // bleed onto the other turn's reply.
        let lines = [
            r#"{"uuid":"u1","type":"user","message":{"role":"user","content":"q1"}}"#,
            r#"{"uuid":"a1","type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"r1"}]}}"#,
            r#"{"uuid":"d1","type":"system","subtype":"turn_duration","durationMs":1000}"#,
            r#"{"uuid":"u2","type":"user","message":{"role":"user","content":"q2"}}"#,
            r#"{"uuid":"a2","type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"r2"}]}}"#,
            r#"{"uuid":"d2","type":"system","subtype":"turn_duration","durationMs":2000}"#,
        ];
        let mut outcomes: Vec<ParsedLine> = lines
            .iter()
            .map(|l| parse_line_outcome(l).unwrap())
            .collect();
        correlate_turn_durations(&mut outcomes);

        let by_uuid = |uuid: &str| {
            outcomes
                .iter()
                .find_map(|o| match o {
                    ParsedLine::Message(m) if m.uuid == MessageUuid::from(uuid) => Some(m.as_ref()),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(by_uuid("a1").response_time_ms, Some(1000.0));
        assert_eq!(by_uuid("a2").response_time_ms, Some(2000.0));
    }

    #[test]
    fn turn_duration_with_no_preceding_assistant_is_a_silent_no_op() {
        // A `turn_duration` whose turn has no assistant message in the window
        // (e.g. the assistant line is below the read cursor, or the turn ended
        // before any assistant line) must not panic walking past the start and
        // must not mis-attach to a user/system line. It simply drops.
        let lines = [
            r#"{"uuid":"u1","type":"user","message":{"role":"user","content":"q"}}"#,
            r#"{"uuid":"d1","type":"system","subtype":"turn_duration","durationMs":4221}"#,
        ];
        let mut outcomes: Vec<ParsedLine> = lines
            .iter()
            .map(|l| parse_line_outcome(l).unwrap())
            .collect();
        correlate_turn_durations(&mut outcomes);

        let user = outcomes
            .iter()
            .find_map(|o| match o {
                ParsedLine::Message(m) => Some(m.as_ref()),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            user.response_time_ms, None,
            "no assistant precedes the duration, so it attaches to nothing"
        );
    }

    #[test]
    fn an_assistant_turn_without_a_duration_line_keeps_response_time_none() {
        // An interrupted/in-progress turn whose `turn_duration` line has not been
        // written yet leaves the assistant's `response_time_ms` as None — the
        // correlation only stamps a message when a duration actually exists.
        let lines = [
            r#"{"uuid":"u1","type":"user","message":{"role":"user","content":"q"}}"#,
            r#"{"uuid":"a1","type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"r"}]}}"#,
        ];
        let mut outcomes: Vec<ParsedLine> = lines
            .iter()
            .map(|l| parse_line_outcome(l).unwrap())
            .collect();
        correlate_turn_durations(&mut outcomes);

        let assistant = outcomes
            .iter()
            .find_map(|o| match o {
                ParsedLine::Message(m) if m.role == Role::Assistant => Some(m.as_ref()),
                _ => None,
            })
            .unwrap();
        assert_eq!(assistant.response_time_ms, None);
    }
}
