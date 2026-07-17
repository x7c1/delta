//! [`CodexConversationSource`]: the item → canonical [`Message`] accumulator.
//!
//! Where [`crate::translate`] turns Codex's wire frames into the lossy neutral
//! *control* stream ([`AgentEvent`]s the Turn FSM / permission reducers act on),
//! this is the second, lossless seam: it folds the *content-bearing* subset of
//! that same neutral stream into Delta's canonical [`Message`]s so a Codex
//! session's conversation persists and renders through the exact
//! provider-neutral pipeline Claude already runs through
//! (`persist_conversation_batch`). Keeping the two layers separate is the design
//! decision A: squeezing content through the event enum would drop fidelity, so
//! the FSM consumes `events()` and this consumes the content subset of the same
//! events.
//!
//! It is the Codex analogue of Claude's attribution fold — an
//! accumulator, not a pull-based reader: Codex *pushes* structured `item/*` /
//! `turn/*` frames, so this is fed one [`AgentEvent`] at a time and yields the
//! messages that event completed. It owns the two pieces of cross-event state
//! the fold needs and Claude gets for free from its line-indexed transcript:
//!
//! - a **monotonic `seq` counter** (Codex frames carry no line index), seeded
//!   from the session's current `MAX(seq)` so it survives a resume/restart;
//! - a **pending tool-item map** so a tool call's `ToolStarted` (name + input)
//!   and `ToolCompleted` (output) pair by `provider_item_id` into one message.
//!
//! ## Faithful vs. degraded (never faked)
//!
//! Reproduced faithfully: role (user / assistant), text, tool use ↔ result
//! pairing, and the turn group (`prompt_id` from the provider turn id). Absent
//! provider facts degrade to `None` rather than being invented — `created_at`,
//! `model`, `git_branch`, `cwd`, `response_time_ms`, and both parent links.
//! `Thinking` / `Meta` / `CompactSummary` blocks are simply not produced (Codex
//! does not expose them). Codex v1 is single-thread (decision D:
//! [`ForkCapability::None`]), so every message lands on the session's `main`
//! thread the source is constructed with.
//!
//! ## Dormant in this slice
//!
//! Like [`crate::CodexAppServerAdapter`] before it was wired, this is
//! **dormant-but-tested**: it is a pure fold with no I/O, unit-tested in
//! isolation here. The event pump that feeds it live — draining `events()` into
//! both this and the FSM, and posting the resulting `(messages, effects)`
//! batches into `persist_conversation_batch` — lands in the following slice.
//!
//! [`AgentEvent`]: delta_usecase::AgentEvent
//! [`ForkCapability::None`]: delta_usecase::ForkCapability
//! [`Message`]: delta_model::Message

use std::collections::HashMap;

use delta_model::{ContentBlock, Message, MessageUuid, PromptId, Role, SessionId, ThreadId};
use delta_usecase::{AgentContentSource, AgentEvent, Effect};
use serde_json::Value;

/// A tool call awaiting its completion frame: the name and input captured from
/// its `ToolStarted`, held until the matching `ToolCompleted` (same
/// `provider_item_id`) lets the two fold into one message.
#[derive(Debug, Clone)]
struct PendingTool {
    name: String,
    input: Value,
}

/// Accumulates the content-bearing subset of a Codex session's neutral
/// [`AgentEvent`] stream into canonical [`Message`]s.
///
/// One instance per session. Feed it every event from the session's `events()`
/// stream with [`Self::ingest`]; it returns the messages that event completed
/// (empty for control-only or streaming events). See the module docs for the
/// fidelity contract and the state it owns.
#[derive(Debug)]
pub struct CodexConversationSource {
    session_id: SessionId,
    /// Codex v1 is single-thread (decision D), so every message lands here.
    main_thread: ThreadId,
    /// The next `seq` to mint. Monotonic per session; seeded from the store's
    /// current `MAX(seq) + 1` so it never collides with a persisted message.
    next_seq: i64,
    /// The current turn's provider id, used as every message's `prompt_id`
    /// (the turn/prompt group). `None` outside a turn or when the turn was
    /// announced without an id.
    current_turn: Option<PromptId>,
    /// Tool calls started but not yet completed, keyed by `provider_item_id`.
    pending_tools: HashMap<String, PendingTool>,
}

impl CodexConversationSource {
    /// Build a source for one session.
    ///
    /// `main_thread` is the session's `main` thread (Codex v1 lands every
    /// message there). `seed_seq` is the next `seq` to mint — the store's
    /// current `MAX(seq) + 1`, or `0` for a fresh session — so the minted
    /// ordering continues past whatever is already persisted.
    pub fn new(session_id: SessionId, main_thread: ThreadId, seed_seq: i64) -> Self {
        Self {
            session_id,
            main_thread,
            next_seq: seed_seq,
            current_turn: None,
            pending_tools: HashMap::new(),
        }
    }

    /// Fold one neutral [`AgentEvent`] into zero or more canonical messages.
    ///
    /// Returns the messages this event *completed*, in order:
    ///
    /// - [`AgentEvent::UserPromptAccepted`] → one `User` message carrying the
    ///   prompt text.
    /// - [`AgentEvent::AssistantMessage`] → one `Assistant` message carrying the
    ///   reply text.
    /// - [`AgentEvent::ToolStarted`] → nothing yet; the call is held until its
    ///   completion.
    /// - [`AgentEvent::ToolCompleted`] → one `Assistant` message pairing the
    ///   held `ToolUse` (when a start was seen) with the `ToolResult`.
    /// - [`AgentEvent::TurnStarted`] → nothing; records the turn group.
    /// - [`AgentEvent::TurnCompleted`] → flushes any tool calls that never
    ///   completed as `ToolUse`-only messages, so nothing is silently dropped.
    /// - Streaming deltas and every control-only event → nothing.
    pub fn ingest(&mut self, event: &AgentEvent) -> Vec<Message> {
        match event {
            AgentEvent::UserPromptAccepted { text, .. } => {
                let uuid = self.user_prompt_uuid();
                vec![self.build(uuid, Role::User, vec![text_block(text)], None)]
            }
            AgentEvent::AssistantMessage {
                provider_item_id,
                text,
            } => {
                let uuid = item_uuid(provider_item_id);
                vec![self.build(
                    uuid,
                    Role::Assistant,
                    vec![text_block(text)],
                    Some(provider_item_id.clone()),
                )]
            }
            AgentEvent::ToolStarted {
                provider_item_id,
                name,
                input_json,
            } => {
                self.pending_tools.insert(
                    provider_item_id.clone(),
                    PendingTool {
                        name: name.clone(),
                        input: input_json.clone(),
                    },
                );
                Vec::new()
            }
            AgentEvent::ToolCompleted {
                provider_item_id,
                output_json,
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
                )]
            }
            AgentEvent::TurnStarted { provider_turn_id } => {
                self.current_turn = provider_turn_id.clone().map(PromptId::from);
                Vec::new()
            }
            AgentEvent::TurnCompleted { .. } => self.flush_pending_tools(),
            // Streaming previews are the control layer's job (the browser's
            // live `AssistantStreaming`), never persisted here; every other
            // variant carries no content.
            _ => Vec::new(),
        }
    }

    /// Flush any tool calls left open at turn end as `ToolUse`-only messages, so
    /// a call whose completion never arrived is still recorded rather than
    /// dropped. Drained in ascending `provider_item_id` order for determinism.
    fn flush_pending_tools(&mut self) -> Vec<Message> {
        if self.pending_tools.is_empty() {
            return Vec::new();
        }
        let mut ids: Vec<String> = self.pending_tools.keys().cloned().collect();
        ids.sort();
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let started = self
                .pending_tools
                .remove(&id)
                .expect("id came from the map's own keys");
            let block = ContentBlock::ToolUse {
                id: id.clone(),
                name: started.name,
                input: started.input,
            };
            let uuid = item_uuid(&id);
            out.push(self.build(uuid, Role::Assistant, vec![block], Some(id)));
        }
        out
    }

    /// Assemble a canonical message, minting the next `seq` and degrading every
    /// provider fact Codex does not expose to `None`.
    fn build(
        &mut self,
        uuid: MessageUuid,
        role: Role,
        content: Vec<ContentBlock>,
        provider_item_id: Option<String>,
    ) -> Message {
        let seq = self.next_seq;
        self.next_seq += 1;
        Message {
            uuid,
            provider_item_id,
            session_id: self.session_id.clone(),
            thread_id: self.main_thread,
            role,
            linear_parent_uuid: None,
            semantic_parent_uuid: None,
            prompt_id: self.current_turn.clone(),
            seq,
            content_text: Message::flatten_text(&content),
            content,
            created_at: None,
            model: None,
            git_branch: None,
            cwd: None,
            response_time_ms: None,
        }
    }

    /// Synthesize a stable uuid for the current turn's user prompt. Codex gives
    /// the accepted prompt no item id, so it is keyed from the turn id when one
    /// is known (one user prompt per turn), and otherwise from the `seq` this
    /// message will be minted with.
    ///
    /// Keying the fallback off `next_seq` — rather than a per-source counter —
    /// makes it unique across the session's whole sequence space, including after
    /// a **resume** that re-seeds `next_seq` at the persisted `MAX(seq) + 1`. A
    /// counter would reset to 0 on the fresh post-resume source and collide its
    /// first prompt's uuid with the pre-restart first prompt (`codex-user-0`),
    /// silently overwriting that earlier message. For a fresh session (seeded at
    /// 0) this yields the same `codex-user-0`, `codex-user-1`, … a bare counter
    /// would, since consecutive prompt-less turns advance `next_seq` in lockstep.
    fn user_prompt_uuid(&self) -> MessageUuid {
        match &self.current_turn {
            Some(turn) => MessageUuid::from(format!("codex-turn-{}-user", turn.as_str())),
            None => MessageUuid::from(format!("codex-user-{}", self.next_seq)),
        }
    }
}

impl AgentContentSource for CodexConversationSource {
    /// Fold one neutral [`AgentEvent`] into the batch the persistence pipeline
    /// consumes.
    ///
    /// Delegates to the inherent [`CodexConversationSource::ingest`], which
    /// produces the messages the event completed. Codex emits no neutral
    /// [`Effect`]s through this content seam: the turn-end / permission
    /// correlation the effects encode is driven off the control stream
    /// (`events()` → the Turn FSM / permission reducers), not the content fold,
    /// so the batch is messages-only — the effect list is always empty.
    fn ingest(&mut self, event: &AgentEvent) -> (Vec<Message>, Vec<Effect>) {
        (CodexConversationSource::ingest(self, event), Vec::new())
    }
}

/// Build a Codex session's content source as the domain-side
/// [`AgentContentSource`] trait object the event pump holds.
///
/// The Delta-side constructor: the pump knows a session's `session_id`, its
/// `main_thread` (Codex v1 lands every message there), and `seed_seq` — the
/// store's current `MAX(seq) + 1`, so minted ordering continues past whatever
/// is already persisted — and gets back the boxed neutral seam without naming
/// the concrete type. Dormant in this slice: nothing wires the pump yet.
pub fn codex_content_source(
    session_id: SessionId,
    main_thread: ThreadId,
    seed_seq: i64,
) -> Box<dyn AgentContentSource> {
    Box::new(CodexConversationSource::new(
        session_id,
        main_thread,
        seed_seq,
    ))
}

/// The uuid synthesized for a message reconstructed from a provider item. Stable
/// across re-ingest because it is derived from the provider's own item id.
fn item_uuid(provider_item_id: &str) -> MessageUuid {
    MessageUuid::from(format!("codex-item-{provider_item_id}"))
}

/// A plain-text content block.
fn text_block(text: &str) -> ContentBlock {
    ContentBlock::Text {
        text: text.to_owned(),
    }
}

/// Whether a tool item's completed frame reports an error.
///
/// Reconciled (R3) against the vendored tool-item shapes: a `commandExecution` /
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
    use serde_json::json;

    fn source() -> CodexConversationSource {
        CodexConversationSource::new(SessionId::from("sess-1"), ThreadId(1), 0)
    }

    #[test]
    fn an_assistant_message_folds_to_one_assistant_message() {
        let mut src = source();
        let msgs = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "item_1".to_owned(),
            text: "hi".to_owned(),
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
        // Every provider fact Codex does not expose degrades to None.
        assert!(m.created_at.is_none());
        assert!(m.model.is_none());
        assert!(m.linear_parent_uuid.is_none());
        assert!(m.response_time_ms.is_none());
    }

    #[test]
    fn a_turn_id_becomes_the_prompt_group_and_seq_is_monotonic() {
        let mut src = source();
        assert!(src
            .ingest(&AgentEvent::TurnStarted {
                provider_turn_id: Some("turn_9".to_owned()),
            })
            .is_empty());
        let user = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "do it".to_owned(),
        });
        let asst = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "item_1".to_owned(),
            text: "done".to_owned(),
        });
        assert_eq!(user[0].role, Role::User);
        assert_eq!(user[0].uuid, MessageUuid::from("codex-turn-turn_9-user"));
        assert_eq!(user[0].prompt_id, Some(PromptId::from("turn_9")));
        assert_eq!(user[0].seq, 0);
        assert_eq!(asst[0].prompt_id, Some(PromptId::from("turn_9")));
        assert_eq!(asst[0].seq, 1);
    }

    #[test]
    fn seed_seq_continues_past_persisted_messages() {
        let mut src = CodexConversationSource::new(SessionId::from("s"), ThreadId(1), 42);
        let msgs = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "i".to_owned(),
            text: "x".to_owned(),
        });
        assert_eq!(msgs[0].seq, 42);
    }

    #[test]
    fn a_started_then_completed_tool_pairs_into_one_message() {
        let mut src = source();
        assert!(src
            .ingest(&AgentEvent::ToolStarted {
                provider_item_id: "t1".to_owned(),
                name: "Bash".to_owned(),
                input_json: json!({ "command": "ls" }),
            })
            .is_empty());
        let msgs = src.ingest(&AgentEvent::ToolCompleted {
            provider_item_id: "t1".to_owned(),
            output_json: json!({ "exitCode": 0, "stdout": "a\nb" }),
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
    fn a_tool_left_open_at_turn_end_flushes_as_tool_use_only() {
        let mut src = source();
        src.ingest(&AgentEvent::ToolStarted {
            provider_item_id: "t1".to_owned(),
            name: "Bash".to_owned(),
            input_json: json!({ "command": "sleep 100" }),
        });
        let flushed = src.ingest(&AgentEvent::TurnCompleted {
            status: delta_usecase::TurnStatus::Completed,
        });
        assert_eq!(flushed.len(), 1);
        assert_eq!(
            flushed[0].content,
            vec![ContentBlock::ToolUse {
                id: "t1".to_owned(),
                name: "Bash".to_owned(),
                input: json!({ "command": "sleep 100" }),
            }]
        );
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

    #[test]
    fn two_prompt_less_turns_get_distinct_user_uuids() {
        let mut src = source();
        let a = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "one".to_owned(),
        });
        let b = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "two".to_owned(),
        });
        assert_ne!(a[0].uuid, b[0].uuid);
        assert_eq!(a[0].uuid, MessageUuid::from("codex-user-0"));
        assert_eq!(b[0].uuid, MessageUuid::from("codex-user-1"));
    }

    #[test]
    fn a_resumed_sources_first_prompt_does_not_collide_with_the_pre_restart_one() {
        // A fresh source seeds at 0: its first prompt-less user prompt is
        // `codex-user-0` at seq 0.
        let fresh_uuid = source()
            .ingest(&AgentEvent::UserPromptAccepted {
                provider_message_id: None,
                text: "first message".to_owned(),
            })
            .remove(0)
            .uuid;
        assert_eq!(fresh_uuid, MessageUuid::from("codex-user-0"));

        // After a restart the source is re-seeded at the persisted count (2 here).
        // Its first prompt-less user prompt must NOT reuse `codex-user-0` — that
        // would overwrite the pre-restart message — so it is keyed off the seeded
        // seq (2) instead, and lands at seq 2.
        let mut resumed = CodexConversationSource::new(SessionId::from("sess-1"), ThreadId(1), 2);
        let resumed_msg = resumed
            .ingest(&AgentEvent::UserPromptAccepted {
                provider_message_id: None,
                text: "second message".to_owned(),
            })
            .remove(0);
        assert_ne!(
            resumed_msg.uuid, fresh_uuid,
            "the resumed prompt must not collide with the pre-restart one"
        );
        assert_eq!(resumed_msg.uuid, MessageUuid::from("codex-user-2"));
        assert_eq!(
            resumed_msg.seq, 2,
            "and it continues the persisted sequence"
        );
    }

    #[test]
    fn the_content_source_trait_yields_the_messages_and_no_effects() {
        // Drive through the domain-side `AgentContentSource` seam (the shape the
        // pump holds), built by the Delta-side factory. It must return the same
        // messages the inherent fold produces, plus an empty effect list.
        let mut src = codex_content_source(SessionId::from("sess-1"), ThreadId(1), 7);
        let (messages, effects) = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "item_1".to_owned(),
            text: "hi".to_owned(),
        });
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].uuid, MessageUuid::from("codex-item-item_1"));
        assert_eq!(messages[0].role, Role::Assistant);
        assert_eq!(messages[0].seq, 7, "the factory's seed_seq is honoured");
        assert!(
            effects.is_empty(),
            "Codex emits no neutral effects through the content seam"
        );
    }

    #[test]
    fn the_content_source_trait_returns_an_empty_batch_for_control_events() {
        let mut src = codex_content_source(SessionId::from("s"), ThreadId(1), 0);
        let (messages, effects) = src.ingest(&AgentEvent::TurnStarted {
            provider_turn_id: Some("turn_1".to_owned()),
        });
        assert!(messages.is_empty());
        assert!(effects.is_empty());
    }
}
