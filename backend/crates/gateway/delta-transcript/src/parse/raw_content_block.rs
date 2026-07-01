//! One typed content block as Claude Code writes it in the JSONL transcript.

use delta_model::ContentBlock;
use serde::Deserialize;

/// The transcript form of a message content block.
///
/// Mirrors the domain [`ContentBlock`] variant-for-variant; see that type for
/// the semantics of each block kind. This wire twin carries the
/// deserialization concerns the domain type must not know about: the `type`
/// tag, the snake_case variant names, the field defaults, and the
/// `#[serde(other)]` tolerance. Any block kind Delta does not model parses
/// into the catch-all variant here and converts to the domain's explicit
/// [`ContentBlock::Other`], so unknown-kind tolerance is modeled in the
/// domain rather than via serde attributes on it.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum RawContentBlock {
    /// Plain assistant or user text.
    Text { text: String },
    /// Extended-thinking text emitted by the model.
    Thinking { thinking: String },
    /// A request from the model to invoke a tool.
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// The result of a previously requested tool invocation. `content` and
    /// `is_error` may be omitted on the wire and default.
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: serde_json::Value,
        #[serde(default)]
        is_error: bool,
    },
    /// Any block kind Delta does not model explicitly.
    #[serde(other)]
    Other,
}

impl From<RawContentBlock> for ContentBlock {
    fn from(block: RawContentBlock) -> Self {
        match block {
            RawContentBlock::Text { text } => ContentBlock::Text { text },
            RawContentBlock::Thinking { thinking } => ContentBlock::Thinking { thinking },
            RawContentBlock::ToolUse { id, name, input } => {
                ContentBlock::ToolUse { id, name, input }
            }
            RawContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            },
            RawContentBlock::Other => ContentBlock::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> ContentBlock {
        let raw: RawContentBlock = serde_json::from_str(json).unwrap();
        raw.into()
    }

    #[test]
    fn unknown_type_parses_as_other() {
        assert_eq!(
            parse(r#"{"type":"image","source":{"x":1}}"#),
            ContentBlock::Other,
        );
    }

    #[test]
    fn tool_result_parses_with_explicit_fields() {
        match parse(
            r#"{"type":"tool_result","tool_use_id":"abc","content":"done","is_error":false}"#,
        ) {
            ContentBlock::ToolResult { tool_use_id, .. } => assert_eq!(tool_use_id, "abc"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn tool_result_defaults_omitted_content_and_error_flag() {
        match parse(r#"{"type":"tool_result","tool_use_id":"abc"}"#) {
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                assert_eq!(content, serde_json::Value::Null);
                assert!(!is_error);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn text_and_tool_use_parse_into_their_domain_twins() {
        assert_eq!(
            parse(r#"{"type":"text","text":"hello"}"#),
            ContentBlock::Text {
                text: "hello".into()
            },
        );
        assert_eq!(
            parse(r#"{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}"#),
            ContentBlock::ToolUse {
                id: "t1".into(),
                name: "Bash".into(),
                input: serde_json::json!({"command": "ls"}),
            },
        );
    }
}
