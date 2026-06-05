//! Message content blocks.
//!
//! A Claude Code message carries an ordered list of typed content blocks. Delta
//! models the kinds it cares about and keeps an `Other` escape hatch so unknown
//! block types parse without error.

use serde::{Deserialize, Serialize};

/// One content block within a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
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
        #[serde(default)]
        content: serde_json::Value,
        #[serde(default)]
        is_error: bool,
    },
    /// Any block kind Delta does not model explicitly.
    #[serde(other)]
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
