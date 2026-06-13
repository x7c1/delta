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

/// Parse one JSONL line. Returns `Ok(None)` for blank lines.
pub fn parse_line(line: &str) -> Result<Option<TranscriptMessage>, serde_json::Error> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let raw: RawLine = serde_json::from_str(trimmed)?;

    // A line without a uuid is not a message we can address; skip it. This
    // deliberately covers `type: "queue-operation"` lines — the uuid-less
    // bookkeeping records current claude writes when a prompt is submitted
    // mid-turn. The queued prompt's real message is the plain `type: "user"`
    // line claude replays at dequeue (which fires its own `UserPromptSubmit`
    // and flows the normal parse/attribution path), so the bookkeeping line
    // carries nothing Delta needs to surface.
    let Some(uuid) = raw.uuid else {
        return Ok(None);
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

    let role = if is_queued_command {
        Role::User
    } else if is_meta {
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
        match raw.message.and_then(|m| m.content) {
            Some(RawContent::Text(text)) => vec![ContentBlock::Text { text }],
            Some(RawContent::Blocks(blocks)) => {
                blocks.into_iter().map(ContentBlock::from).collect()
            }
            None => Vec::new(),
        }
    };

    Ok(Some(TranscriptMessage {
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
    }))
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
        // provenance field, which must not perturb the parse.
        let line = r#"{"uuid":"u9","parentUuid":"m8","type":"user","promptSource":"queued","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"Reply with only the word: ok"}}"#;
        let msg = parse_line(line).unwrap().unwrap();
        assert_eq!(msg.role, Role::User);
        assert_eq!(
            msg.flatten_text().as_deref(),
            Some("Reply with only the word: ok")
        );
        assert!(!msg.is_queued_command);
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
}
