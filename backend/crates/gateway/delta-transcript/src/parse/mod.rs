//! Parsing a single JSONL transcript line into a [`TranscriptMessage`].

mod raw_content;
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

    // A line without a uuid is not a message we can address; skip it.
    let Some(uuid) = raw.uuid else {
        return Ok(None);
    };

    let role = raw
        .line_type
        .as_deref()
        .map(Role::from_transcript_type)
        .unwrap_or(Role::Other);

    let content = match raw.message.and_then(|m| m.content) {
        Some(RawContent::Text(text)) => vec![ContentBlock::Text { text }],
        Some(RawContent::Blocks(blocks)) => blocks,
        None => Vec::new(),
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
