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
//! `model`, `git_branch`, `cwd`, `response_time_ms`, and the linear parent link.
//! `Thinking` / `Meta` / `CompactSummary` blocks are simply not produced (Codex
//! does not expose them). A plain turn's messages land on the session's `main`
//! thread the source is constructed with; a *branch* turn (Delta's
//! branch-from-selected-text, delivered as hidden-context injection rather than a
//! native provider fork — Codex is still [`ForkCapability::None`]) is routed onto
//! its branch child thread by [`AgentContentSource::begin_turn`] before its frames
//! arrive, and its root user message carries the branched-from message as its
//! semantic parent — matching the `send` row's own lane + parent, so branch
//! content lives on the branch lane exactly like Claude's.
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
    /// The `ToolStarted`'s `at_ms` (the item's `startedAtMs`), used as the
    /// message time if the call is flushed without a completion frame.
    at_ms: Option<i64>,
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
    /// The thread the *current* turn's messages land on. Seeded at construction
    /// from the session's `main` thread and reset to it for a plain turn; set to
    /// the resolved branch child thread by [`AgentContentSource::begin_turn`] when
    /// a branch turn dispatches, so the turn's content follows the same lane the
    /// `send` row already recorded rather than falling back onto `main`.
    turn_thread: ThreadId,
    /// The branched-from message to stamp on the current branch turn's *root*
    /// user message (its first message), mirroring the `send` row's
    /// `semantic_parent`. Set by [`AgentContentSource::begin_turn`] and consumed
    /// (taken) when that root user prompt is built, so only it — not the turn's
    /// later messages — carries the semantic parent. `None` for a plain turn.
    pending_semantic_parent: Option<MessageUuid>,
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
    /// `main_thread` is the session's `main` thread: a plain turn's messages land
    /// there, and it is where each turn's routing resets to. A branch turn instead
    /// routes onto its branch child thread via
    /// [`AgentContentSource::begin_turn`], set before the turn's frames arrive.
    /// `seed_seq` is the next `seq` to mint — the store's current `MAX(seq) + 1`,
    /// or `0` for a fresh session — so the minted ordering continues past whatever
    /// is already persisted.
    pub fn new(session_id: SessionId, main_thread: ThreadId, seed_seq: i64) -> Self {
        Self {
            session_id,
            // A turn defaults to the main thread with no semantic parent, so a
            // session that never branches behaves exactly as before; a branch
            // turn overrides both via `begin_turn` before its frames arrive.
            turn_thread: main_thread,
            pending_semantic_parent: None,
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
            AgentEvent::UserPromptAccepted { text, at_ms, .. } => {
                let uuid = self.user_prompt_uuid();
                // This is the turn's root user message: consume the pending
                // semantic parent so the branch root — and only it — carries the
                // branched-from link, matching the `send` row. A plain turn has
                // none, so this stays `None`.
                let semantic_parent = self.pending_semantic_parent.take();
                vec![self.build(
                    uuid,
                    Role::User,
                    vec![text_block(text)],
                    None,
                    semantic_parent,
                    *at_ms,
                )]
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
            out.push(self.build(
                uuid,
                Role::Assistant,
                vec![block],
                Some(id),
                None,
                started.at_ms,
            ));
        }
        out
    }

    /// Assemble a canonical message, minting the next `seq` and degrading every
    /// provider fact Codex does not expose to `None`.
    ///
    /// `at_ms` is the item's lifecycle timestamp in epoch milliseconds (the
    /// `startedAtMs` / `completedAtMs` the translation carried onto the event),
    /// converted here to the canonical ISO-8601 UTC `created_at` string. It stays
    /// `None` when the provider exposed no time, so `created_at` degrades rather
    /// than being invented. `model` / `git_branch` / `cwd` remain unconditionally
    /// `None` — those provider facts are a separate follow-up.
    fn build(
        &mut self,
        uuid: MessageUuid,
        role: Role,
        content: Vec<ContentBlock>,
        provider_item_id: Option<String>,
        semantic_parent_uuid: Option<MessageUuid>,
        at_ms: Option<i64>,
    ) -> Message {
        let seq = self.next_seq;
        self.next_seq += 1;
        Message {
            uuid,
            provider_item_id,
            session_id: self.session_id.clone(),
            // The current turn's thread: `main` for a plain turn, or the branch
            // child thread `begin_turn` set for a branch turn — so branch content
            // lands on the branch lane, not `main`.
            thread_id: self.turn_thread,
            role,
            linear_parent_uuid: None,
            semantic_parent_uuid,
            prompt_id: self.current_turn.clone(),
            seq,
            content_text: Message::flatten_text(&content),
            content,
            created_at: at_ms.and_then(iso8601_from_epoch_ms),
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

    /// Route the turn about to dispatch: land its messages on `thread_id` (the
    /// branch child thread for a branch send, `main` otherwise) and, for a branch
    /// send, stamp `semantic_parent` on the turn's root user message so the branch
    /// content matches the `send` row's own lane + parent. Set on the mailbox
    /// before the turn's frames are pumped in, so every message this turn folds
    /// uses it.
    fn begin_turn(&mut self, thread_id: ThreadId, semantic_parent: Option<MessageUuid>) {
        self.turn_thread = thread_id;
        self.pending_semantic_parent = semantic_parent;
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

/// Convert an epoch-millisecond timestamp to the canonical ISO-8601 UTC string
/// Delta stores in [`Message::created_at`] (the same RFC 3339 `…Z` shape Claude's
/// transcript timestamps already use). An out-of-range value yields `None` rather
/// than a bogus string.
fn iso8601_from_epoch_ms(at_ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(at_ms)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
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
        // Every provider fact Codex does not expose degrades to None.
        assert!(m.created_at.is_none());
        assert!(m.model.is_none());
        assert!(m.linear_parent_uuid.is_none());
        assert!(m.response_time_ms.is_none());
    }

    #[test]
    fn an_events_at_ms_becomes_an_iso8601_created_at_and_absence_degrades_to_none() {
        let mut src = source();
        // A message built from an event carrying `at_ms` gets a canonical
        // ISO-8601 UTC `created_at` (RFC 3339, `…Z`), converted from epoch ms.
        let with_ts = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "a1".to_owned(),
            text: "hi".to_owned(),
            at_ms: Some(1_700_000_000_123),
        });
        assert_eq!(
            with_ts[0].created_at.as_deref(),
            Some("2023-11-14T22:13:20.123Z")
        );
        // A missing `at_ms` still degrades `created_at` to None (never invented).
        let without_ts = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "a2".to_owned(),
            text: "yo".to_owned(),
            at_ms: None,
        });
        assert!(without_ts[0].created_at.is_none());
    }

    #[test]
    fn a_completed_tools_created_at_comes_from_the_completion_and_a_flush_from_the_start() {
        // A paired tool message is minted at completion, so its `created_at` is
        // the completion's `at_ms`.
        let mut src = source();
        src.ingest(&AgentEvent::ToolStarted {
            provider_item_id: "t1".to_owned(),
            name: "Bash".to_owned(),
            input_json: json!({ "command": "ls" }),
            at_ms: Some(1_700_000_000_000),
        });
        let completed = src.ingest(&AgentEvent::ToolCompleted {
            provider_item_id: "t1".to_owned(),
            output_json: json!({ "exitCode": 0 }),
            at_ms: Some(1_700_000_005_000),
        });
        assert_eq!(
            completed[0].created_at.as_deref(),
            Some("2023-11-14T22:13:25.000Z")
        );

        // A tool left open at turn end is flushed with its `ToolStarted` time.
        src.ingest(&AgentEvent::ToolStarted {
            provider_item_id: "t2".to_owned(),
            name: "Bash".to_owned(),
            input_json: json!({}),
            at_ms: Some(1_700_000_000_000),
        });
        let flushed = src.ingest(&AgentEvent::TurnCompleted {
            status: delta_usecase::TurnStatus::Completed,
        });
        assert_eq!(
            flushed[0].created_at.as_deref(),
            Some("2023-11-14T22:13:20.000Z")
        );
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
            at_ms: None,
        });
        let asst = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "item_1".to_owned(),
            text: "done".to_owned(),
            at_ms: None,
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
            at_ms: None,
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
    fn a_tool_left_open_at_turn_end_flushes_as_tool_use_only() {
        let mut src = source();
        src.ingest(&AgentEvent::ToolStarted {
            provider_item_id: "t1".to_owned(),
            name: "Bash".to_owned(),
            input_json: json!({ "command": "sleep 100" }),
            at_ms: None,
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
            at_ms: None,
        });
        let b = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "two".to_owned(),
            at_ms: None,
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
                at_ms: None,
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
                at_ms: None,
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
            at_ms: None,
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
    fn a_branch_turn_routes_its_messages_to_the_branch_thread_and_stamps_the_semantic_parent() {
        // `begin_turn` sets the branch child thread + the branched-from message
        // before the turn's frames arrive (as the dispatch does on the mailbox).
        let mut src = CodexConversationSource::new(SessionId::from("s"), ThreadId(8), 0);
        src.begin_turn(
            ThreadId(9),
            Some(MessageUuid::from("codex-item-msg_parent")),
        );

        // The root user prompt lands on the branch thread AND carries the
        // semantic parent — reproducing (as the fix) the DB symptom, where these
        // rows wrongly landed on main (thread 8) with no semantic parent.
        let user = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "branch text".to_owned(),
            at_ms: None,
        });
        assert_eq!(user[0].role, Role::User);
        assert_eq!(
            user[0].thread_id,
            ThreadId(9),
            "the branch root user message lands on the branch thread, not main"
        );
        assert_eq!(
            user[0].semantic_parent_uuid,
            Some(MessageUuid::from("codex-item-msg_parent")),
            "the branch root user message is stamped with the branched-from message"
        );

        // The turn's subsequent assistant message also lands on the branch
        // thread, but does NOT re-carry the semantic parent (only the root does).
        let asst = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "item_1".to_owned(),
            text: "reply".to_owned(),
            at_ms: None,
        });
        assert_eq!(
            asst[0].thread_id,
            ThreadId(9),
            "the branch turn's assistant reply also lands on the branch thread"
        );
        assert!(
            asst[0].semantic_parent_uuid.is_none(),
            "only the branch root carries the semantic parent, not later messages"
        );
    }

    #[test]
    fn a_plain_turn_stays_on_main_with_no_semantic_parent() {
        // With no `begin_turn`, or `begin_turn(main, None)`, every message stays
        // on the main thread with no semantic parent — the pre-fix behaviour a
        // non-branching session must keep byte-for-byte.
        let mut src = CodexConversationSource::new(SessionId::from("s"), ThreadId(8), 0);
        src.begin_turn(ThreadId(8), None);
        let user = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "hi".to_owned(),
            at_ms: None,
        });
        let asst = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "item_1".to_owned(),
            text: "yo".to_owned(),
            at_ms: None,
        });
        assert_eq!(user[0].thread_id, ThreadId(8));
        assert!(user[0].semantic_parent_uuid.is_none());
        assert_eq!(asst[0].thread_id, ThreadId(8));
        assert!(asst[0].semantic_parent_uuid.is_none());
    }

    #[test]
    fn a_plain_turn_after_a_branch_turn_resets_back_to_main() {
        // A branch turn overrides the routing; the following plain turn's
        // `begin_turn(main, None)` must reset it, so late/next-turn content does
        // not leak onto the branch lane.
        let mut src = CodexConversationSource::new(SessionId::from("s"), ThreadId(8), 0);
        src.begin_turn(
            ThreadId(9),
            Some(MessageUuid::from("codex-item-msg_parent")),
        );
        let _ = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "branch".to_owned(),
            at_ms: None,
        });
        src.begin_turn(ThreadId(8), None);
        let user = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "plain again".to_owned(),
            at_ms: None,
        });
        assert_eq!(user[0].thread_id, ThreadId(8), "reset back to main");
        assert!(
            user[0].semantic_parent_uuid.is_none(),
            "the reset turn carries no semantic parent"
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
