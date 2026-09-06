use serde_json::Value;

/// A tool call awaiting its completion frame: the name and input captured from
/// its `ToolStarted`, held until the matching `ToolCompleted` (same
/// `provider_item_id`) lets the two fold into one message.
#[derive(Debug, Clone)]
pub(super) struct PendingTool {
    pub(super) name: String,
    pub(super) input: Value,
    /// The `ToolStarted`'s `at_ms` (the item's `startedAtMs`), used as the
    /// message time if the call is flushed without a completion frame.
    pub(super) at_ms: Option<i64>,
}
