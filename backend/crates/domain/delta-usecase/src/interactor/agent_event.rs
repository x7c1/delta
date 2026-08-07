//! The event pump's actor-side handler: one neutral [`AgentEvent`] from a
//! terminal-less agent session (Codex), folded into content, control, and
//! streaming — all on the session's own mailbox.
//!
//! A push-based provider surfaces its turn/content frames *after* the
//! `enqueue`/spawn call that started the work has already returned, so there is
//! no synchronous `Vec<SessionEvent>` left to fold them into. The pump
//! ([`spawn_agent_event_pump`]) drains the adapter's `events()` stream on a
//! background task and posts each event back to the *same* actor as a
//! [`SessionInput::IngestAgentEvent`]; this handler runs there, in mailbox
//! order, so content persistence and the turn machine mutate in event-arrival
//! order and a `TurnCompleted` always lands after the messages of the turn it
//! ends. Everything it produces goes out on the async seam
//! ([`InteractorCore::emit_async_event`]), which the server drains into the
//! browser broadcast.
//!
//! [`InteractorCore::emit_async_event`]: crate::interactor::InteractorCore::emit_async_event

use std::collections::BTreeSet;

use tokio::sync::mpsc;

use crate::agent::{AgentEvent, AgentEventStream, AgentPermissionRequest, TurnStatus};
use crate::interactor::agent_permission::reduce_permission_event;
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::input::SessionInput;
use crate::interactor::PermissionDecision;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Ingest one neutral [`AgentEvent`] from a Codex session's event stream.
    ///
    /// Two layers run per event, in this order so turn-end lands after the
    /// turn's own content:
    ///
    /// 1. **Content** — fold the event through the session's content
    ///    accumulator and, when it completed any messages, run them through the
    ///    provider-neutral persistence pipeline
    ///    ([`persist_conversation_batch`](Self::persist_conversation_batch)),
    ///    emitting a [`SessionEvent::TranscriptUpdated`] for the threads they
    ///    landed on plus any events the batch's effects produced.
    /// 2. **Control / streaming** — a live [`AgentEvent::AssistantDelta`] becomes
    ///    a non-persisted [`SessionEvent::AssistantStreaming`] preview, and a
    ///    [`AgentEvent::TurnCompleted`] advances the turn machine and emits the
    ///    matching turn-end browser event.
    ///
    /// Fire-and-forget: the pump has no reply channel, so a per-event failure is
    /// logged and the pump continues rather than tearing the session down over a
    /// single bad frame.
    pub(in crate::interactor) async fn on_agent_event(&mut self, event: AgentEvent) {
        self.persist_agent_content(&event).await;

        match event {
            AgentEvent::AssistantDelta {
                provider_item_id,
                text,
            } => self.stream_assistant_delta(provider_item_id, text).await,
            AgentEvent::TurnCompleted { status } => self.complete_agent_turn(status).await,
            AgentEvent::PermissionRequested { request } => {
                self.request_agent_permission(request).await
            }
            AgentEvent::PermissionResolved {
                request_id,
                decision,
            } => self.resolve_agent_permission(&request_id, decision),
            // Every other event either completed content (handled above) or is
            // control-only with no browser signal in this slice (session
            // start/end, turn start — already applied at send — and
            // unsupported-interaction handling lands in a later slice).
            //
            // `ThinkingDelta` is deliberately NOT streamed: the only live
            // preview the browser has is `AssistantStreaming`, which is reply
            // text, and pushing reasoning through it would put the model's
            // thinking on screen as its answer. Thinking therefore appears once
            // the completed `ThinkingMessage` persists — the same way Claude's
            // does, which also has no live thinking preview.
            _ => {}
        }
    }

    /// Ingest a provider-neutral [`AgentEvent::PermissionRequested`] from the
    /// adapter: allocate the Delta permission row, correlate its `i64` id with
    /// the adapter-scoped provider token, index it for decision routing, and
    /// route through [`reduce_permission_event`] under the `i64`-stringified id
    /// so the runtime mirror, the browser notice, and the reducer all speak the
    /// row id — never the provider token.
    ///
    /// Fire-and-forget: a row-allocation failure is logged and the pump
    /// continues, exactly like the content path, rather than tearing the session
    /// down over a single permission frame.
    async fn request_agent_permission(&mut self, request: AgentPermissionRequest) {
        let token = request.request_id;
        let row = match self
            .store
            .record_permission_request(
                self.id,
                &request.tool_name,
                &request.input_json.to_string(),
                request.tool_use_id.as_deref(),
            )
            .await
        {
            Ok(row) => row,
            Err(err) => {
                tracing::error!(
                    session_id = %self.id,
                    error = %err,
                    "failed to record a Codex permission request; dropping it"
                );
                return;
            }
        };

        // Correlate the row id with the opaque provider token (for the decision
        // path) and index the row → session (so a decision, which carries only
        // the row id, routes back to this actor). The decision path removes the
        // index entry; the pump's resolution removes the correlation.
        self.state.correlate_agent_permission(row.id, token);
        self.permission_index
            .lock()
            .expect("permission index poisoned")
            .insert(row.id, self.id.clone());

        // Re-express the fact under the `i64` row id and route it through the
        // shared reducer, so the queryable mirror and the broadcast are
        // byte-identical to the Claude hook path. Emitted on the async seam: the
        // pump has no synchronous `Vec<SessionEvent>` to fold into.
        let event = AgentEvent::PermissionRequested {
            request: AgentPermissionRequest {
                request_id: row.id.to_string(),
                tool_name: request.tool_name,
                input_json: request.input_json,
                tool_use_id: request.tool_use_id,
            },
        };
        for event in reduce_permission_event(self.state, self.id, &event) {
            self.emit_async_event(event);
        }
    }

    /// Ingest a provider-neutral [`AgentEvent::PermissionResolved`] from the
    /// adapter (emitted when a decision was answered over the wire): translate
    /// the adapter-scoped provider token back to the Delta row id, drop the
    /// routing-index entry, and route through [`reduce_permission_event`] under
    /// the `i64` id so the mirror clears and the settle broadcasts.
    ///
    /// A token with no correlation is a no-op (already resolved), so a duplicate
    /// or late resolution frame cannot double-clear or panic the reducer.
    fn resolve_agent_permission(&mut self, token: &str, decision: PermissionDecision) {
        let Some(request_id) = self.state.resolve_agent_permission_token(token) else {
            return;
        };
        // Drop the routing-index entry. The browser-decision path already
        // removed it before posting the decision, so this is only load-bearing
        // for a provider-initiated resolution; either way the remove is
        // idempotent.
        self.permission_index
            .lock()
            .expect("permission index poisoned")
            .remove(&request_id);
        let event = AgentEvent::PermissionResolved {
            request_id: request_id.to_string(),
            decision,
        };
        for event in reduce_permission_event(self.state, self.id, &event) {
            self.emit_async_event(event);
        }
    }

    /// Fold the event through the content accumulator and persist anything it
    /// completed, broadcasting the resulting transcript/effect events.
    async fn persist_agent_content(&mut self, event: &AgentEvent) {
        let Some((messages, effects)) = self.state.fold_agent_content(event) else {
            return;
        };
        if messages.is_empty() && effects.is_empty() {
            return;
        }
        match self
            .persist_conversation_batch(self.id, messages, effects)
            .await
        {
            Ok((persisted, effect_events)) => {
                if !persisted.is_empty() {
                    let thread_ids: Vec<_> = persisted
                        .iter()
                        .map(|m| m.thread_id)
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect();
                    self.emit_async_event(SessionEvent::TranscriptUpdated {
                        session_id: self.id.clone(),
                        thread_ids,
                    });
                }
                for event in effect_events {
                    self.emit_async_event(event);
                }
            }
            Err(err) => {
                tracing::error!(
                    session_id = %self.id,
                    error = %err,
                    "failed to persist a Codex content batch; dropping it"
                );
            }
        }
    }

    /// Broadcast a live assistant-message fragment as a non-persisted streaming
    /// preview, reconciled per turn (the persisted [`AgentEvent::AssistantMessage`]
    /// takes over when the turn ends) — the Codex counterpart of the Claude
    /// `MessageDisplay` hook. The delta's `provider_item_id` is the preview's
    /// message id, so successive fragments of one item accumulate.
    async fn stream_assistant_delta(&mut self, provider_item_id: String, text: String) {
        let thread_id = match self.store.in_progress_turn_thread(self.id).await {
            Ok(thread_id) => thread_id,
            Err(err) => {
                tracing::error!(
                    session_id = %self.id,
                    error = %err,
                    "failed to resolve the in-progress thread for a Codex streaming delta"
                );
                return;
            }
        };
        // Codex deltas carry no chunk index and never carry the final fragment
        // (the completed message arrives as a persisted `AssistantMessage`, not a
        // delta), so the index is auto-assigned and `final_` is always false.
        let index = self.state.accumulate_streaming_delta(
            &provider_item_id,
            thread_id,
            false,
            text.clone(),
        );
        self.emit_async_event(SessionEvent::AssistantStreaming {
            session_id: self.id.clone(),
            thread_id,
            message_id: provider_item_id,
            index,
            final_: false,
            delta: text,
        });
    }

    /// Advance the turn machine on a Codex turn-end fact and emit the matching
    /// browser event. The thread is resolved *before* the machine runs, since
    /// applying the turn end can sweep the in-flight send (the thread source).
    async fn complete_agent_turn(&mut self, status: TurnStatus) {
        let thread_id = match self.store.in_progress_turn_thread(self.id).await {
            Ok(thread_id) => Some(thread_id),
            Err(err) => {
                tracing::error!(
                    session_id = %self.id,
                    error = %err,
                    "failed to resolve the in-progress thread for a Codex turn end"
                );
                None
            }
        };
        if let Err(err) = self.apply_turn_end(status).await {
            tracing::error!(
                session_id = %self.id,
                error = %err,
                "failed to apply a Codex turn end to the turn machine"
            );
        }
        // A cleanly completed turn is a `TurnCompleted`; an interrupted or failed
        // one clears the stuck chip via `TurnInterrupted`, mirroring how the
        // Claude transcript sync signals a turn that ended without a `Stop` hook.
        let event = match status {
            TurnStatus::Completed => SessionEvent::TurnCompleted {
                session_id: self.id.clone(),
                thread_id,
                stop_reason: None,
            },
            TurnStatus::Interrupted | TurnStatus::Failed => SessionEvent::TurnInterrupted {
                session_id: self.id.clone(),
                thread_id,
            },
        };
        self.emit_async_event(event);
    }
}

/// Spawn the event pump for a terminal-less (adapter-backed) agent session.
///
/// Drains the adapter's `events()` stream on a background task and posts each
/// event back to the owning actor as a [`SessionInput::IngestAgentEvent`], so
/// everything the provider pushes flows through the one ordered mailbox. The
/// `self_sender` is *weak*: the pump never keeps the actor alive, so when the
/// registry drops the actor's strong sender (retirement, or the interactor
/// shutting down) the upgrade fails and the pump exits. It also exits when the
/// adapter closes the session's stream (the sender end is dropped), which is
/// what stops it on a normal session close.
pub(in crate::interactor) fn spawn_agent_event_pump(
    self_sender: mpsc::WeakUnboundedSender<SessionInput>,
    mut stream: AgentEventStream,
) {
    tokio::spawn(async move {
        while let Some(event) = stream.recv().await {
            let Some(sender) = self_sender.upgrade() else {
                break;
            };
            if sender
                .send(SessionInput::IngestAgentEvent { event })
                .is_err()
            {
                break;
            }
        }
    });
}
