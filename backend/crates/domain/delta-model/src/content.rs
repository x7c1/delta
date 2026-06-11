//! Message content blocks.
//!
//! A Claude Code message carries an ordered list of typed content blocks. Delta
//! models the kinds it cares about and keeps an explicit [`ContentBlock::Other`]
//! variant so unknown block kinds are representable in the domain; the
//! tolerance itself (parsing an unknown kind into `Other`) is implemented by
//! the wire/record twins in the gateway crates.

/// One content block within a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentBlock {
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
        content: serde_json::Value,
        is_error: bool,
    },
    /// Any block kind Delta does not model explicitly.
    Other,
}

impl ContentBlock {
    /// Extract human-readable text, if this block carries any.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text } => Some(text),
            ContentBlock::Thinking { thinking } => Some(thinking),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_text_extracts_text_and_thinking_only() {
        assert_eq!(
            ContentBlock::Text { text: "hi".into() }.as_text(),
            Some("hi")
        );
        assert_eq!(
            ContentBlock::Thinking {
                thinking: "hmm".into()
            }
            .as_text(),
            Some("hmm")
        );
        assert_eq!(ContentBlock::Other.as_text(), None);
        assert_eq!(
            ContentBlock::ToolUse {
                id: "t1".into(),
                name: "Bash".into(),
                input: serde_json::Value::Null,
            }
            .as_text(),
            None
        );
    }
}
