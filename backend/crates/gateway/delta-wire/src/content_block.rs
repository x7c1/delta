//! The wire form of [`ContentBlock`].

use delta_model::ContentBlock;
use serde::Serialize;
use ts_rs::TS;

/// JSON shape of one message content block on the REST surface.
///
/// Mirrors the domain [`ContentBlock`] variant-for-variant; see that type for
/// the semantics of each block kind. This wire twin carries the serialization
/// concerns the domain type must not know about: the `type` tag, the
/// snake_case variant names, and the TypeScript export. Tool payloads stay
/// arbitrary JSON (`unknown` in TypeScript) because Delta passes them through
/// without interpreting them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(rename = "ContentBlock")]
pub enum WireContentBlock {
    /// Plain assistant or user text.
    Text { text: String },
    /// Extended-thinking text emitted by the model.
    Thinking { thinking: String },
    /// A request from the model to invoke a tool.
    ToolUse {
        id: String,
        name: String,
        #[ts(type = "unknown")]
        input: serde_json::Value,
    },
    /// The result of a previously requested tool invocation.
    ToolResult {
        tool_use_id: String,
        #[ts(type = "unknown")]
        content: serde_json::Value,
        is_error: bool,
    },
    /// Any block kind Delta does not model explicitly.
    Other,
}

impl From<ContentBlock> for WireContentBlock {
    fn from(block: ContentBlock) -> Self {
        match block {
            ContentBlock::Text { text } => WireContentBlock::Text { text },
            ContentBlock::Thinking { thinking } => WireContentBlock::Thinking { thinking },
            ContentBlock::ToolUse { id, name, input } => {
                WireContentBlock::ToolUse { id, name, input }
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => WireContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            },
            ContentBlock::Other => WireContentBlock::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(block: ContentBlock) -> serde_json::Value {
        serde_json::to_value(WireContentBlock::from(block)).unwrap()
    }

    #[test]
    fn text_and_thinking_blocks_keep_their_tagged_shape() {
        assert_eq!(
            json(ContentBlock::Text {
                text: "hello".into()
            }),
            serde_json::json!({ "type": "text", "text": "hello" }),
        );
        assert_eq!(
            json(ContentBlock::Thinking {
                thinking: "hmm".into()
            }),
            serde_json::json!({ "type": "thinking", "thinking": "hmm" }),
        );
    }

    #[test]
    fn tool_blocks_keep_their_payload_fields_on_the_wire() {
        assert_eq!(
            json(ContentBlock::ToolUse {
                id: "t1".into(),
                name: "Bash".into(),
                input: serde_json::json!({ "command": "ls" }),
            }),
            serde_json::json!({
                "type": "tool_use",
                "id": "t1",
                "name": "Bash",
                "input": { "command": "ls" },
            }),
        );
        assert_eq!(
            json(ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: serde_json::json!("done"),
                is_error: false,
            }),
            serde_json::json!({
                "type": "tool_result",
                "tool_use_id": "t1",
                "content": "done",
                "is_error": false,
            }),
        );
    }

    #[test]
    fn unknown_blocks_serialize_as_bare_other() {
        assert_eq!(
            json(ContentBlock::Other),
            serde_json::json!({ "type": "other" }),
        );
    }
}
