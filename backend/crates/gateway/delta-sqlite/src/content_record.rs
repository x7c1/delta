//! The stored form of message content blocks (`message.content_json`).

use delta_model::ContentBlock;
use serde::{Deserialize, Serialize};

/// One content block as persisted in the `message.content_json` column.
///
/// Mirrors the domain [`ContentBlock`] variant-for-variant; see that type for
/// the semantics of each block kind. This record twin carries the
/// serialization concerns the domain type must not know about: the `type`
/// tag, the snake_case variant names, the read-side field defaults, and the
/// `#[serde(other)]` tolerance for block kinds written by a newer Delta. The
/// JSON it writes is byte-identical to what the previous domain-derive
/// produced, so existing databases keep working unchanged.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlockRecord {
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
    /// The result of a previously requested tool invocation.
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: serde_json::Value,
        #[serde(default)]
        is_error: bool,
    },
    /// Any block kind this Delta does not model explicitly.
    #[serde(other)]
    Other,
}

impl From<&ContentBlock> for ContentBlockRecord {
    fn from(block: &ContentBlock) -> Self {
        match block {
            ContentBlock::Text { text } => ContentBlockRecord::Text { text: text.clone() },
            ContentBlock::Thinking { thinking } => ContentBlockRecord::Thinking {
                thinking: thinking.clone(),
            },
            ContentBlock::ToolUse { id, name, input } => ContentBlockRecord::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            },
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => ContentBlockRecord::ToolResult {
                tool_use_id: tool_use_id.clone(),
                content: content.clone(),
                is_error: *is_error,
            },
            ContentBlock::Other => ContentBlockRecord::Other,
        }
    }
}

impl From<ContentBlockRecord> for ContentBlock {
    fn from(record: ContentBlockRecord) -> Self {
        match record {
            ContentBlockRecord::Text { text } => ContentBlock::Text { text },
            ContentBlockRecord::Thinking { thinking } => ContentBlock::Thinking { thinking },
            ContentBlockRecord::ToolUse { id, name, input } => {
                ContentBlock::ToolUse { id, name, input }
            }
            ContentBlockRecord::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            },
            ContentBlockRecord::Other => ContentBlock::Other,
        }
    }
}

/// Serialize content blocks into the `content_json` column value.
///
/// Serialization of these records cannot realistically fail (they are plain
/// data), but the previous code defaulted to an empty array rather than
/// poisoning the whole upsert, and that behavior is preserved.
pub(crate) fn encode_content(blocks: &[ContentBlock]) -> String {
    let records: Vec<ContentBlockRecord> = blocks.iter().map(ContentBlockRecord::from).collect();
    serde_json::to_string(&records).unwrap_or_else(|_| "[]".into())
}

/// Deserialize a `content_json` column value back into domain blocks.
///
/// A malformed value decodes to an empty list (mirroring the long-standing
/// read-side behavior) rather than failing the row; unknown block kinds inside
/// a well-formed value decode to [`ContentBlock::Other`] individually.
pub(crate) fn decode_content(json: &str) -> Vec<ContentBlock> {
    let records: Vec<ContentBlockRecord> = serde_json::from_str(json).unwrap_or_default();
    records.into_iter().map(ContentBlock::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `content_json` value captured from a database written before the
    /// record twin existed (the domain type's own serde derives produced it).
    const STORED_FIXTURE: &str = "[{\"type\":\"thinking\",\"thinking\":\"hmm\"},\
         {\"type\":\"text\",\"text\":\"hi\"},\
         {\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\"input\":{\"command\":\"ls\"}},\
         {\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"done\",\"is_error\":false},\
         {\"type\":\"other\"}]";

    fn fixture_blocks() -> Vec<ContentBlock> {
        vec![
            ContentBlock::Thinking {
                thinking: "hmm".into(),
            },
            ContentBlock::Text { text: "hi".into() },
            ContentBlock::ToolUse {
                id: "t1".into(),
                name: "Bash".into(),
                input: serde_json::json!({"command": "ls"}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: serde_json::json!("done"),
                is_error: false,
            },
            ContentBlock::Other,
        ]
    }

    #[test]
    fn decodes_the_preexisting_stored_format() {
        assert_eq!(decode_content(STORED_FIXTURE), fixture_blocks());
    }

    #[test]
    fn encodes_byte_identically_to_the_preexisting_stored_format() {
        assert_eq!(encode_content(&fixture_blocks()), STORED_FIXTURE);
    }

    #[test]
    fn unknown_stored_block_kind_decodes_as_other() {
        // A row written by a newer Delta that models more block kinds must
        // still read: the unknown kind degrades to `Other`, not an error.
        let json = r#"[{"type":"video","src":"x"},{"type":"text","text":"hi"}]"#;
        assert_eq!(
            decode_content(json),
            vec![
                ContentBlock::Other,
                ContentBlock::Text { text: "hi".into() }
            ],
        );
    }

    #[test]
    fn malformed_content_json_decodes_to_empty() {
        assert_eq!(decode_content("not json"), Vec::<ContentBlock>::new());
    }
}
