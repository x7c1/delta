use delta_model::{ContentBlock, Message, PromptId, Role};
use delta_usecase::AgentEvent;
use serde_json::Value;

use super::pending_tool::PendingTool;
use super::{item_uuid, CodexConversationSource};

impl CodexConversationSource {
    /// Fold one neutral [`AgentEvent`] into zero or more canonical messages.
    ///
    /// Returns the messages this event *completed*, in order:
    ///
    /// - [`AgentEvent::UserPromptAccepted`] → one `User` message carrying the
    ///   prompt text, also held as the turn's pending root for re-stamping.
    /// - [`AgentEvent::AssistantMessage`] → one `Assistant` message carrying the
    ///   reply text.
    /// - [`AgentEvent::ThinkingMessage`] → one `Assistant` message carrying a
    ///   [`ContentBlock::Thinking`] — the model's reasoning as its own block,
    ///   never folded into reply text.
    /// - [`AgentEvent::ToolStarted`] → nothing yet; the call is held until its
    ///   completion.
    /// - [`AgentEvent::ToolCompleted`] → one `Assistant` message pairing the
    ///   held `ToolUse` (when a start was seen) with the `ToolResult`.
    /// - [`AgentEvent::TurnStarted`] → records the turn group, and re-emits the
    ///   prompt that was accepted before it (the pending root) with this turn as
    ///   its `prompt_id`; nothing when no prompt is awaiting a turn id or the
    ///   turn was announced without one.
    /// - [`AgentEvent::TurnCompleted`] → flushes any tool calls that never
    ///   completed as `ToolUse`-only messages, so nothing is silently dropped,
    ///   then closes the turn group and drops any prompt still held as the
    ///   pending root — it was steered into the turn that just ended and already
    ///   carries it, so the next turn must not re-stamp it.
    /// - Streaming deltas and every control-only event → nothing.
    pub fn ingest(&mut self, event: &AgentEvent) -> Vec<Message> {
        match event {
            AgentEvent::UserPromptAccepted { text, at_ms, .. } => {
                let uuid = self.user_prompt_uuid();
                // This is the turn's root user message: consume the pending
                // semantic parent so the branch root — and only it — carries the
                // branched-from link, matching the `send` row. A plain turn has
                // none, so this stays `None`.
                let semantic_parent = self.pending_semantic_parent.take();
                let prompt = self.build(
                    uuid,
                    Role::User,
                    vec![text_block(text)],
                    None,
                    semantic_parent,
                    *at_ms,
                );
                // Hold it so the turn's own `TurnStarted` — which has not
                // arrived yet — can re-stamp it with that turn's id.
                self.pending_turn_root = Some(prompt.clone());
                vec![prompt]
            }
            AgentEvent::AssistantMessage {
                provider_item_id,
                text,
                at_ms,
            } => {
                let uuid = item_uuid(provider_item_id);
                vec![self.build(
                    uuid,
                    Role::Assistant,
                    vec![text_block(text)],
                    Some(provider_item_id.clone()),
                    None,
                    *at_ms,
                )]
            }
            AgentEvent::ThinkingMessage {
                provider_item_id,
                text,
                at_ms,
            } => {
                let uuid = item_uuid(provider_item_id);
                vec![self.build(
                    uuid,
                    Role::Assistant,
                    vec![thinking_block(text)],
                    Some(provider_item_id.clone()),
                    None,
                    *at_ms,
                )]
            }
            AgentEvent::ToolStarted {
                provider_item_id,
                name,
                input_json,
                at_ms,
            } => {
                self.pending_tools.insert(
                    provider_item_id.clone(),
                    PendingTool {
                        name: name.clone(),
                        input: input_json.clone(),
                        at_ms: *at_ms,
                    },
                );
                Vec::new()
            }
            AgentEvent::ToolCompleted {
                provider_item_id,
                output_json,
                at_ms,
            } => {
                let mut blocks = Vec::new();
                if let Some(started) = self.pending_tools.remove(provider_item_id) {
                    blocks.push(ContentBlock::ToolUse {
                        id: provider_item_id.clone(),
                        name: started.name,
                        input: started.input,
                    });
                }
                blocks.push(ContentBlock::ToolResult {
                    tool_use_id: provider_item_id.clone(),
                    content: output_json.clone(),
                    is_error: is_error_output(output_json),
                });
                let uuid = item_uuid(provider_item_id);
                vec![self.build(
                    uuid,
                    Role::Assistant,
                    blocks,
                    Some(provider_item_id.clone()),
                    None,
                    *at_ms,
                )]
            }
            AgentEvent::TurnStarted { provider_turn_id } => {
                self.current_turn = provider_turn_id.clone().map(PromptId::from);
                self.restamp_pending_root()
            }
            AgentEvent::TurnCompleted { .. } => {
                // Flush first: the tools left open belong to the turn that is
                // ending, so they must still be stamped with it.
                let flushed = self.flush_pending_tools();
                self.current_turn = None;
                // A prompt still pending here was steered into the turn that
                // just ended and already carries it; the next turn must not
                // claim it.
                self.pending_turn_root = None;
                flushed
            }
            // Streaming previews are the control layer's job (the browser's
            // live `AssistantStreaming`), never persisted here. Usage facts
            // (`TokenUsageUpdated` / `RateLimitsUpdated`) are observability
            // only and deliberately stay out of the conversation: they are
            // measurements *about* the session, not something anyone said.
            // Every other variant carries no content either.
            _ => Vec::new(),
        }
    }
}

/// A plain-text content block.
fn text_block(text: &str) -> ContentBlock {
    ContentBlock::Text {
        text: text.to_owned(),
    }
}

/// An extended-thinking content block: the model's reasoning, kept as its own
/// block kind so it is never displayed as the model's reply.
fn thinking_block(thinking: &str) -> ContentBlock {
    ContentBlock::Thinking {
        thinking: thinking.to_owned(),
    }
}

/// Whether a tool item's completed frame reports an error.
///
/// Reconciled against the vendored tool-item shapes: a `commandExecution` /
/// `fileChange` item carries a terminal `status` (`CommandExecutionStatus` /
/// `PatchApplyStatus`, one of `inProgress` / `completed` / `failed` /
/// `declined`), and a command additionally an `exitCode`. A `failed` / `declined`
/// status, a non-zero `exitCode`, or an explicit `is_error` / non-null `error`
/// (kept for any provider convention that uses them) all mark an error;
/// everything else defaults to `false` (success). Localised here so any later
/// schema correction stays in one place.
fn is_error_output(output: &Value) -> bool {
    if output.get("is_error").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    if output.get("error").is_some_and(|e| !e.is_null()) {
        return true;
    }
    if matches!(
        output.get("status").and_then(Value::as_str),
        Some("failed") | Some("declined")
    ) {
        return true;
    }
    match output.get("exitCode").and_then(Value::as_i64) {
        Some(code) => code != 0,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::testing::source;
    use delta_model::{MessageUuid, ThreadId};
    use serde_json::json;

    #[test]
    fn an_assistant_message_folds_to_one_assistant_message() {
        let mut src = source();
        let msgs = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "item_1".to_owned(),
            text: "hi".to_owned(),
            at_ms: None,
        });
        assert_eq!(msgs.len(), 1);
        let m = &msgs[0];
        assert_eq!(m.uuid, MessageUuid::from("codex-item-item_1"));
        assert_eq!(m.provider_item_id.as_deref(), Some("item_1"));
        assert_eq!(m.role, Role::Assistant);
        assert_eq!(m.thread_id, ThreadId(1));
        assert_eq!(m.seq, 0);
        assert_eq!(m.content, vec![text_block("hi")]);
        assert_eq!(m.content_text.as_deref(), Some("hi"));
        // Every provider fact Codex does not expose degrades to None — including
        // the model, which this source was built without.
        assert!(m.created_at.is_none());
        assert!(m.model.is_none());
        assert!(m.linear_parent_uuid.is_none());
        assert!(m.response_time_ms.is_none());
    }

    #[test]
    fn a_thinking_message_folds_to_a_thinking_block_never_reply_text() {
        let mut src = source();
        let msgs = src.ingest(&AgentEvent::ThinkingMessage {
            provider_item_id: "r1".to_owned(),
            text: "weighing the options".to_owned(),
            at_ms: Some(1_700_000_000_123),
        });
        assert_eq!(msgs.len(), 1);
        let m = &msgs[0];
        assert_eq!(m.role, Role::Assistant);
        assert_eq!(m.uuid, MessageUuid::from("codex-item-r1"));
        assert_eq!(m.provider_item_id.as_deref(), Some("r1"));
        assert_eq!(
            m.content,
            vec![ContentBlock::Thinking {
                thinking: "weighing the options".to_owned()
            }],
            "reasoning is its own block kind, never a Text block"
        );
        assert_eq!(m.created_at.as_deref(), Some("2023-11-14T22:13:20.123Z"));
    }

    #[test]
    fn a_started_then_completed_tool_pairs_into_one_message() {
        let mut src = source();
        assert!(src
            .ingest(&AgentEvent::ToolStarted {
                provider_item_id: "t1".to_owned(),
                name: "Bash".to_owned(),
                input_json: json!({ "command": "ls" }),
                at_ms: None,
            })
            .is_empty());
        let msgs = src.ingest(&AgentEvent::ToolCompleted {
            provider_item_id: "t1".to_owned(),
            output_json: json!({ "exitCode": 0, "stdout": "a\nb" }),
            at_ms: None,
        });
        assert_eq!(msgs.len(), 1);
        let m = &msgs[0];
        assert_eq!(m.uuid, MessageUuid::from("codex-item-t1"));
        assert_eq!(m.provider_item_id.as_deref(), Some("t1"));
        assert_eq!(
            m.content,
            vec![
                ContentBlock::ToolUse {
                    id: "t1".to_owned(),
                    name: "Bash".to_owned(),
                    input: json!({ "command": "ls" }),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "t1".to_owned(),
                    content: json!({ "exitCode": 0, "stdout": "a\nb" }),
                    is_error: false,
                },
            ]
        );
        // Tool blocks carry no display text.
        assert!(m.content_text.is_none());
    }

    #[test]
    fn a_completion_without_a_start_is_a_result_only_message() {
        let mut src = source();
        let msgs = src.ingest(&AgentEvent::ToolCompleted {
            provider_item_id: "t9".to_owned(),
            output_json: json!({ "is_error": true, "message": "boom" }),
            at_ms: None,
        });
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            msgs[0].content,
            vec![ContentBlock::ToolResult {
                tool_use_id: "t9".to_owned(),
                content: json!({ "is_error": true, "message": "boom" }),
                is_error: true,
            }]
        );
    }

    #[test]
    fn a_tool_status_marks_the_result_error_state() {
        // The real command/file-change item carries a terminal `status`; a
        // `failed` / `declined` completion is an error even with no `exitCode`.
        let mut src = source();
        let msgs = src.ingest(&AgentEvent::ToolCompleted {
            provider_item_id: "fc1".to_owned(),
            output_json: json!({ "type": "fileChange", "status": "declined", "changes": [] }),
            at_ms: None,
        });
        assert_eq!(
            msgs[0].content,
            vec![ContentBlock::ToolResult {
                tool_use_id: "fc1".to_owned(),
                content: json!({ "type": "fileChange", "status": "declined", "changes": [] }),
                is_error: true,
            }]
        );
        // A `completed` status with a zero exit code is a success.
        let ok = src.ingest(&AgentEvent::ToolCompleted {
            provider_item_id: "c1".to_owned(),
            output_json: json!({ "type": "commandExecution", "status": "completed", "exitCode": 0 }),
            at_ms: None,
        });
        assert!(matches!(
            &ok[0].content[0],
            ContentBlock::ToolResult {
                is_error: false,
                ..
            }
        ));
    }

    #[test]
    fn streaming_deltas_and_control_events_produce_no_messages() {
        let mut src = source();
        assert!(src
            .ingest(&AgentEvent::AssistantDelta {
                provider_item_id: "item_1".to_owned(),
                text: "partial".to_owned(),
            })
            .is_empty());
        assert!(src
            .ingest(&AgentEvent::ThinkingDelta {
                provider_item_id: "r1".to_owned(),
                text: "half a thought".to_owned(),
            })
            .is_empty());
        assert!(src
            .ingest(&AgentEvent::SessionStarted {
                provider_session_id: "thr".to_owned(),
            })
            .is_empty());
        assert!(src
            .ingest(&AgentEvent::TurnCompleted {
                status: delta_usecase::TurnStatus::Completed,
            })
            .is_empty());
    }
}
