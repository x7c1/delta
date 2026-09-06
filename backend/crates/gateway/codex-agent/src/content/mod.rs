//! [`CodexConversationSource`]: the item → canonical [`Message`] accumulator.
//!
//! Where [`crate::translate`] turns Codex's wire frames into the lossy neutral
//! *control* stream ([`AgentEvent`]s the Turn FSM / permission reducers act on),
//! this is the second, lossless seam: it folds the *content-bearing* subset of
//! that same neutral stream into Delta's canonical [`Message`]s so a Codex
//! session's conversation persists and renders through the exact
//! provider-neutral pipeline Claude already runs through
//! (`persist_conversation_batch`). Keeping the two layers separate is a
//! deliberate design decision: squeezing content through the event enum would
//! drop fidelity, so the FSM consumes `events()` and this consumes the content
//! subset of the same events.
//!
//! It is the Codex analogue of Claude's attribution fold — an
//! accumulator, not a pull-based reader: Codex *pushes* structured `item/*` /
//! `turn/*` frames, so this is fed one [`AgentEvent`] at a time and yields the
//! messages that event completed. It owns the three pieces of cross-event state
//! the fold needs and Claude gets for free from its line-indexed transcript:
//!
//! - a **monotonic `seq` counter** (Codex frames carry no line index), seeded
//!   from the session's current `MAX(seq) + 1` so it survives a resume/restart;
//! - a **pending tool-item map** so a tool call's `ToolStarted` (name + input)
//!   and `ToolCompleted` (output) pair by `provider_item_id` into one message;
//! - the **prompt just accepted**, held until the turn it was sent to announces
//!   its id (see below).
//!
//! ## A prompt arrives before its turn is named
//!
//! The adapter emits [`AgentEvent::UserPromptAccepted`] *before* it issues
//! `turn/start`, so at that instant the turn the prompt belongs to has no id
//! yet: whatever the last [`AgentEvent::TurnStarted`] named is the **previous**
//! turn, or — when the prompt is sent mid-turn — the **running** one (Codex
//! accepts `turn/start` while a turn is in flight and steers the input into
//! that turn, answering with a fresh id that never appears as a
//! `turn/started`). Two consequences shape this fold:
//!
//! - a user prompt's uuid is minted from its own `seq`, never from the turn id
//!   (see [`CodexConversationSource::user_prompt_uuid`]). Keying it off the turn
//!   would give two prompts steered into one turn the same uuid, and the
//!   persistence upsert — which deliberately keeps `thread_id` /
//!   `semantic_parent_uuid` authoritative from the first ingest — would then
//!   drop the earlier prompt's text and pin the survivor to the earlier
//!   prompt's lane;
//! - the prompt is held as the turn's *pending root* and re-emitted once the
//!   turn's own `TurnStarted` names it, with the same uuid / `seq` / thread and
//!   that turn as its `prompt_id`. Until then its `prompt_id` degrades to
//!   `None` rather than borrowing the previous turn's id. A prompt steered into
//!   a running turn keeps that turn's id (correctly — it *is* that turn's
//!   input) and is never re-stamped by the next turn.
//!
//! ## Faithful vs. degraded (never faked)
//!
//! Reproduced faithfully: role (user / assistant), text, the model's extended
//! thinking, tool use ↔ result pairing, the turn group (`prompt_id` from the
//! provider turn id — for a user prompt, from the turn that later announces
//! itself, see *A prompt arrives before its turn is named* above), and the
//! session's metadata — the `model` the server
//! resolved, plus the `cwd` Delta launched the session in and the `git_branch`
//! it observed there, all set once at construction and stamped on every message
//! (see [`CodexConversationSource::new`]). Absent facts degrade to `None` rather
//! than being invented — `created_at`, `response_time_ms`, and the linear parent
//! link, plus `model` when the server reported none and `git_branch` when the
//! launch directory is not a git working tree. `Meta` / `CompactSummary` blocks
//! are simply not produced (Codex does not expose them); a `Thinking` block
//! *is*, folded from the neutral [`AgentEvent::ThinkingMessage`] a `reasoning`
//! item projects to, so a Codex session shows the model's thinking exactly as a
//! Claude one does.
//!
//! A plain turn's messages land on the session's `main` thread the source is
//! constructed with; a *branch* turn (Delta's
//! branch-from-selected-text, delivered as hidden-context injection rather than a
//! native provider fork — Codex is still [`ForkCapability::None`]) is routed onto
//! its branch child thread by [`AgentContentSource::begin_turn`] before its frames
//! arrive, and its root user message carries the branched-from message as its
//! semantic parent — matching the `send` row's own lane + parent, so branch
//! content lives on the branch lane exactly like Claude's.
//!
//! ## How it is driven
//!
//! It is a pure fold with no I/O, so it is unit-tested in isolation here. Live,
//! the event pump drives it: `events()` is drained into both this and the FSM,
//! and the resulting `(messages, effects)` batches are posted into
//! `persist_conversation_batch`. The pump builds one per session at bind time
//! (see [`codex_content_source`]), which is also when the per-session metadata
//! above is captured.
//!
//! ## Split across files
//!
//! One file per element. This module holds the struct, the two constructors and
//! the item-uuid helper the fold shares. The fold's steps get a file each, with
//! the tests that cover them: `ingest` the event → message dispatch, and the
//! `build` / `restamp_pending_root` / `flush_pending_tools` /
//! `user_prompt_uuid` it delegates to. `agent_content_source` wires the neutral
//! trait onto the inherent fold, `pending_tool` is the record a started tool
//! call is held as, and `testing` holds the builders the tests share.
//!
//! [`AgentEvent`]: delta_usecase::AgentEvent
//! [`AgentEvent::ThinkingMessage`]: delta_usecase::AgentEvent::ThinkingMessage
//! [`AgentEvent::TurnStarted`]: delta_usecase::AgentEvent::TurnStarted
//! [`AgentEvent::UserPromptAccepted`]: delta_usecase::AgentEvent::UserPromptAccepted
//! [`ForkCapability::None`]: delta_usecase::ForkCapability
//! [`Message`]: delta_model::Message

mod agent_content_source;
mod build;
mod flush_pending_tools;
mod ingest;
mod restamp_pending_root;
mod user_prompt_uuid;

mod pending_tool;
use pending_tool::PendingTool;

#[cfg(test)]
mod testing;

use std::collections::HashMap;

use delta_model::{Message, MessageUuid, PromptId, SessionId, ThreadId};
use delta_usecase::{AgentContentSource, ContentSourceRequest};

/// Accumulates the content-bearing subset of a Codex session's neutral
/// [`AgentEvent`] stream into canonical [`Message`]s.
///
/// One instance per session. Feed it every event from the session's `events()`
/// stream with [`Self::ingest`]; it returns the messages that event completed
/// (empty for control-only or streaming events). See the module docs for the
/// fidelity contract and the state it owns.
///
/// [`AgentEvent`]: delta_usecase::AgentEvent
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
    /// (the turn/prompt group). Cleared when the turn completes, so a message
    /// folded while no turn is running degrades to no group rather than
    /// borrowing the finished turn's id; also `None` when the turn was
    /// announced without an id.
    current_turn: Option<PromptId>,
    /// The user prompt built from the latest [`AgentEvent::UserPromptAccepted`],
    /// held until the next [`AgentEvent::TurnStarted`] names the turn it was
    /// sent to, so it can be re-emitted with that turn as its `prompt_id` (same
    /// uuid, `seq`, thread and semantic parent — the upsert refreshes the group).
    /// Taken on that `TurnStarted` and dropped on [`AgentEvent::TurnCompleted`],
    /// so a prompt Codex steered into an already-running turn — which keeps that
    /// turn's id and gets no `turn/started` of its own — is never re-stamped by
    /// the following turn. `None` when there is nothing awaiting a turn id.
    ///
    /// [`AgentEvent::UserPromptAccepted`]: delta_usecase::AgentEvent::UserPromptAccepted
    /// [`AgentEvent::TurnStarted`]: delta_usecase::AgentEvent::TurnStarted
    /// [`AgentEvent::TurnCompleted`]: delta_usecase::AgentEvent::TurnCompleted
    pending_turn_root: Option<Message>,
    /// Tool calls started but not yet completed, keyed by `provider_item_id`.
    pending_tools: HashMap<String, PendingTool>,
    /// The model the Codex server resolved for this thread, read by the adapter
    /// off the response that opened it. `None` when the response carried none.
    model: Option<String>,
    /// The directory the agent is running in — the launch directory Delta
    /// resolved for this session.
    cwd: String,
    /// The branch Delta observed in [`Self::cwd`] when the session was bound.
    /// `None` when that directory is not a git working tree, or HEAD is detached
    /// there — neither of which is worth inventing a branch name for.
    git_branch: Option<String>,
}

impl CodexConversationSource {
    /// Build a source for one session.
    ///
    /// `req.main_thread` is the session's `main` thread: a plain turn's messages
    /// land there, and it is where each turn's routing resets to. A branch turn
    /// instead routes onto its branch child thread via
    /// [`AgentContentSource::begin_turn`], set before the turn's frames arrive.
    /// `req.seed_seq` is the next `seq` to mint — the store's current
    /// `MAX(seq) + 1`, or `0` for a fresh session — so the minted ordering
    /// continues past whatever is already persisted.
    ///
    /// `model` and `req.cwd` / `req.git_branch` are the session's metadata,
    /// stamped on every message this source folds. They are taken here, at
    /// construction, because all three are settled before the session's first
    /// frame and never change while it runs.
    ///
    /// They come from two different places, by which party is the authority:
    ///
    /// - `model` is what the **server** resolved, read by the adapter off the
    ///   response that opened the thread. Delta cannot know it — a launch
    ///   option, the user's own config and the server's default all feed that
    ///   decision, and only the response says which won.
    /// - `cwd` and `git_branch` are **Delta's own** launch site: the directory it
    ///   chose to run the agent in, and the branch it observed there. Codex
    ///   reports neither (its `thread/start` echoes `cwd` back and leaves
    ///   `gitInfo` null), so Delta's own record and observation are the only
    ///   honest source.
    pub fn new(req: ContentSourceRequest, model: Option<String>) -> Self {
        Self {
            session_id: req.session_id,
            // A turn defaults to the main thread with no semantic parent, so a
            // session that never branches behaves exactly as before; a branch
            // turn overrides both via `begin_turn` before its frames arrive.
            turn_thread: req.main_thread,
            pending_semantic_parent: None,
            next_seq: req.seed_seq,
            current_turn: None,
            pending_turn_root: None,
            pending_tools: HashMap::new(),
            model,
            cwd: req.cwd,
            git_branch: req.git_branch,
        }
    }
}

/// Build a Codex session's content source as the domain-side
/// [`AgentContentSource`] trait object the event pump holds.
///
/// The Delta-side constructor: the core hands over the neutral
/// [`ContentSourceRequest`] (the session's identity, its `main_thread` — Codex
/// v1 lands every message there —, the `seed_seq` that continues the persisted
/// ordering, and the launch site it resolved and observed), the adapter adds the
/// `model` the Codex server reported for the thread, and the caller gets back
/// the boxed neutral seam without naming the concrete type.
pub fn codex_content_source(
    req: ContentSourceRequest,
    model: Option<String>,
) -> Box<dyn AgentContentSource> {
    Box::new(CodexConversationSource::new(req, model))
}

/// The uuid synthesized for a message reconstructed from a provider item. Stable
/// across re-ingest because it is derived from the provider's own item id.
fn item_uuid(provider_item_id: &str) -> MessageUuid {
    MessageUuid::from(format!("codex-item-{provider_item_id}"))
}
