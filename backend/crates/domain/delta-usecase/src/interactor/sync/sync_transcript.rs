use delta_attribution::Effect;
use delta_model::{Message, Session};

use crate::agent::AgentEvent;
use crate::error::Result;
use crate::interactor::agent_permission::reduce_permission_event;
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::RunningSubagent;
use crate::interactor::PermissionDecision;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

use super::conversation_source::{ClaudeConversationSource, ConversationSource};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Pull new transcript lines from disk and persist them as messages,
    /// attributing each to the right thread as it is ingested.
    ///
    /// Runs inside the session's actor, which is what serializes the
    /// read-cursor → read-file → ingest → set-cursor sequence: every caller
    /// (hook handlers, the background tail, open/close) reaches it through
    /// the same mailbox, so two syncs of one session can never interleave and
    /// double-ingest or race the cursor — and *different* sessions ingest in
    /// parallel, which the old global sync lock forbade. The cursor is
    /// per-session state, so per-session serialization is exactly enough.
    ///
    /// This method is only the I/O shell. It pulls a batch of canonical
    /// conversation content from the provider's
    /// [`ConversationSource`](super::conversation_source::ConversationSource)
    /// — for Claude, the JSONL transcript read plus the pure attribution fold,
    /// seeded from the store *inside* that source — and then runs the
    /// provider-neutral persistence pipeline
    /// ([`Self::persist_conversation_batch`]) over the resulting
    /// `(messages, effects)`. The attribution decisions — which thread each
    /// line lands on, which send is consumed, which permission rows a
    /// tool_result settles, whether the turn was interrupted — are made by the
    /// source and surfaced as [`Effect`]s the pipeline executes.
    ///
    /// Returns the newly-ingested messages and any [`SessionEvent`]s that the
    /// ingest produced. Two events can arise here:
    ///
    /// - [`SessionEvent::PermissionResolved`]: when a `tool_result` line is
    ///   ingested, the open permission request correlated by its `tool_use_id`
    ///   is resolved so the browser can clear the "permission requested" notice.
    /// - [`SessionEvent::TurnInterrupted`]: emitted in two cases, both of which
    ///   end a turn without firing Claude's `Stop` hook. (1) A `[Request
    ///   interrupted by user...]` marker line: the user aborted the in-flight
    ///   turn. (2) A synthetic `isApiErrorMessage` assistant line: the turn ended
    ///   on an API error (a usage/session limit, a rate limit, or any other API
    ///   failure). In either case this is the hook-independent signal that clears
    ///   the stuck send. They differ only in the turn-machine input fed (an
    ///   interrupt vs. a genuine stop), not in the browser signal.
    ///
    /// The caller is responsible for broadcasting these events.
    pub(in crate::interactor) async fn sync_transcript(
        &mut self,
        session: &Session,
    ) -> Result<(Vec<Message>, Vec<SessionEvent>)> {
        // Pull the next batch of canonical conversation content from the
        // provider's source. For Claude this reads new transcript lines and
        // folds them; all of the fold's Claude-specific seeding stays inside
        // the source.
        let (messages, effects) = ClaudeConversationSource::new(&self.transcript, &self.store)
            .next_batch(session)
            .await?;

        // No new provider content this window: skip the pipeline entirely (and
        // its empty write transaction), exactly as the pre-seam path returned
        // early on a missing transcript path or an empty read.
        if messages.is_empty() && effects.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        self.persist_conversation_batch(session, messages, effects)
            .await
    }

    /// The provider-neutral persistence pipeline: execute the batch's effects
    /// in decision order, persist its messages, and return them alongside any
    /// [`SessionEvent`]s the effects produced.
    ///
    /// This is the shared body every provider's [`ConversationSource`] output
    /// flows through — nothing here is Claude-specific. It executes each
    /// [`Effect`] the source decided on (permission resolution, turn-end
    /// signals, send matching, subagent indicator/launch bookkeeping) and then
    /// upserts the messages with the overlay-preserving `ON CONFLICT` rule the
    /// store owns. The caller broadcasts the returned events.
    async fn persist_conversation_batch(
        &mut self,
        session: &Session,
        messages: Vec<Message>,
        effects: Vec<Effect>,
    ) -> Result<(Vec<Message>, Vec<SessionEvent>)> {
        // Execute the source's effects in decision order, then persist.
        let mut events = Vec::new();
        for effect in effects {
            match effect {
                Effect::ResolvePermission {
                    tool_use_id,
                    allowed,
                } => {
                    // Resolve the `PreToolUse`-recorded row keyed by
                    // `tool_use_id`, plus any pending dialog row the
                    // `PermissionRequest` hook owns (answered in the TUI after
                    // the browser-decision wait timed out).
                    // The `tool_use_id` → row correlation is projection-owned
                    // and stays here; each resolved row then flows through the
                    // permission reducer as a clean neutral event, which keeps
                    // the queryable mirror in step with the broadcast (clearing
                    // both the dialog and any question — their row ids are
                    // disjoint, so resolving both is safe) and emits the
                    // `PermissionResolved` that settles the browser notice.
                    let decision = if allowed {
                        PermissionDecision::Allow
                    } else {
                        PermissionDecision::Deny
                    };
                    for request_id in self
                        .store
                        .resolve_permission_by_tool_use_id(&session.id, &tool_use_id, allowed)
                        .await?
                    {
                        let event = AgentEvent::PermissionResolved {
                            request_id: request_id.to_string(),
                            decision,
                        };
                        events.extend(reduce_permission_event(self.state, &session.id, &event));
                    }
                }
                Effect::TurnInterrupted => {
                    // Recover the interrupted turn's thread BEFORE the machine
                    // runs: `apply_turn_input` can sweep the head dispatched
                    // send (the authoritative thread source).
                    let thread_id = self.store.in_progress_turn_thread(&session.id).await?;
                    // The interrupt ends the turn: route it as a
                    // `TurnCompleted(Interrupted)` fact (which maps to the
                    // machine's `Interrupt` input, back to `Idle`). Dispatching
                    // any queued send is left to the caller (which acts on the
                    // returned `TurnInterrupted` after this sync returns), so no
                    // keystrokes are sent from inside the ingestion path.
                    self.apply_turn_end(crate::agent::TurnStatus::Interrupted)
                        .await?;
                    events.push(SessionEvent::TurnInterrupted {
                        session_id: session.id.clone(),
                        thread_id: Some(thread_id),
                    });
                }
                Effect::TurnAborted => {
                    // A synthetic `isApiErrorMessage` line ended the turn on an
                    // API error (usage/session limit, rate limit, ...). The turn
                    // genuinely ended in failure, so route it as a
                    // `TurnCompleted(Failed)` fact — which maps to the machine's
                    // `Stop` input (back to `Idle`), giving the same honest
                    // turn-end disposition the missing `Stop` hook would have.
                    // We reuse `TurnInterrupted` as the browser signal: like an
                    // interrupt, no `Stop` hook fired, so the browser must clear
                    // the stuck pending chip and drop any orphaned streaming
                    // preview (which may never get a persisted message). The
                    // caller releases the queued send after this sync returns (it
                    // keys on `TurnInterrupted`), so no keystrokes are sent from
                    // inside the ingestion path.
                    let thread_id = self.store.in_progress_turn_thread(&session.id).await?;
                    self.apply_turn_end(crate::agent::TurnStatus::Failed)
                        .await?;
                    events.push(SessionEvent::TurnInterrupted {
                        session_id: session.id.clone(),
                        thread_id: Some(thread_id),
                    });
                }
                Effect::LocalCommandTurnEnded => {
                    // A dispatched send was consumed by a slash/local command
                    // (e.g. `/review-pr`), not by a model turn. A local command
                    // is handled entirely client-side: it fires no
                    // `UserPromptSubmit` echo and no `Stop` hook, so without this
                    // the turn machine stays in `AwaitingEcho` forever and every
                    // later send defers to `queued` and never dispatches. The
                    // `SendMatched` effect emitted alongside this one already
                    // consumed the send (it left `dispatched`), so a
                    // `TurnCompleted(Completed)` fact here (which maps to the
                    // machine's `Stop` input) returns the machine to `Idle`
                    // cleanly: its defensive requeue/sweep is a no-op against the
                    // now-matched row. Reuse `TurnInterrupted` as the browser
                    // signal — like an interrupt or an API-error abort, no `Stop`
                    // hook fired, so the browser must clear the stuck pending
                    // chip. The caller releases any queued send after this sync
                    // returns (it keys on `TurnInterrupted`), so no keystrokes are
                    // sent from inside the ingestion path.
                    let thread_id = self.store.in_progress_turn_thread(&session.id).await?;
                    self.apply_turn_end(crate::agent::TurnStatus::Completed)
                        .await?;
                    events.push(SessionEvent::TurnInterrupted {
                        session_id: session.id.clone(),
                        thread_id: Some(thread_id),
                    });
                }
                Effect::SendMatched {
                    send_id,
                    matched_uuid,
                } => {
                    self.store.mark_send_matched(send_id, &matched_uuid).await?;
                }
                Effect::SubagentLaunched {
                    tool_use_id,
                    thread_id,
                } => {
                    // Persist the launching thread so a completion notification
                    // landing in a later sync window can be attributed to it.
                    self.store
                        .record_subagent_launch(&session.id, &tool_use_id, thread_id)
                        .await?;
                    // For a background subagent the immediate `PostToolUse`
                    // hook may have ALREADY arrived (and likely has — the call
                    // returned synchronously at launch). The hook recorded the
                    // launching tool's `agentId` on the in-memory running entry
                    // but could not yet persist it on the launch row, which did
                    // not exist until just now. Flush that pending upgrade so a
                    // later `<task-notification>` whose `<tool-use-id>` element
                    // was stripped can still match by `<task-id>` from the
                    // reseeded launch map.
                    if let Some(task_id) = self
                        .state
                        .pending_subagent_task_id(&tool_use_id)
                        .map(str::to_owned)
                    {
                        self.store
                            .upgrade_subagent_task_id(&session.id, &tool_use_id, &task_id)
                            .await?;
                    }
                }
                Effect::SubagentIndicatorStarted {
                    tool_use_id,
                    thread_id,
                    subagent_type,
                    description,
                    background,
                } => {
                    // Parent transcript ingest is the source of truth for the
                    // running-subagent indicator. The matching `tool_use` block
                    // only appears in the parent's JSONL when the launch is a
                    // PARENT launch (a nested subagent's tool_use is written to
                    // the subagent's own JSONL, not the parent's), so this path
                    // can never light a parent indicator for a nested launch —
                    // which is what made the older PreToolUse-driven mechanism
                    // get stuck for depth>=2 subagent trees.
                    //
                    // `start_subagent` de-duplicates by `tool_use_id`, so
                    // re-ingesting the same line (e.g. after a cursor rewind in
                    // tests) is a safe no-op. `task_id` is not knowable at this
                    // point: for a background entry it is learned later via
                    // `PostToolUse(Agent)` / its subsequent flush. The browser
                    // event only fires on a newly-added entry, mirroring the
                    // old hook-driven idempotency contract.
                    let newly = self.state.start_subagent(RunningSubagent {
                        thread_id,
                        tool_use_id: tool_use_id.clone(),
                        task_id: None,
                        subagent_type: subagent_type.clone(),
                        description: description.clone(),
                        background,
                    });
                    // Fold any `agentId` the matching `PostToolUse(Agent)`
                    // stashed before this entry existed: for a top-level
                    // background launch the hook can fire before
                    // `tool_use(Agent)` is flushed to the parent's JSONL, so
                    // the hook's direct upgrade was a no-op and the value
                    // would otherwise be lost. Apply it now to the freshly
                    // created entry and persist it through the launch row,
                    // which `Effect::SubagentLaunched` (emitted earlier in
                    // this fold) has already INSERTed.
                    if let Some(task_id) = self
                        .state
                        .drain_pending_post_tool_use_agent_id(&tool_use_id)
                    {
                        if self.state.upgrade_subagent_task_id(&tool_use_id, &task_id) {
                            self.store
                                .upgrade_subagent_task_id(&session.id, &tool_use_id, &task_id)
                                .await?;
                        }
                    }
                    if newly {
                        events.push(SessionEvent::SubagentStarted {
                            session_id: session.id.clone(),
                            thread_id,
                            tool_use_id,
                            subagent_type,
                            description,
                            background,
                        });
                    }
                }
                Effect::AutoCompactFinished => {
                    // A compaction summary just landed — re-type any send
                    // stuck behind the swallowed echo. The debounce inside
                    // the helper deduplicates against the live
                    // `SessionStart(source=compact)` hook path.
                    self.try_redispatch_after_compact("AutoCompactFinished")
                        .await?;
                }
                Effect::SubagentCompleted { tool_use_id } => {
                    // The notification was folded and matched its launch: the
                    // correlation is consumed, so clear the persisted row.
                    self.store
                        .clear_subagent_launch(&session.id, &tool_use_id)
                        .await?;
                    // This is the BACKGROUND subagent's end signal. A background
                    // `Agent`/`Task` was started by `PreToolUse` and survived its
                    // immediate `PostToolUse` and the turn-end sweep; its
                    // completion `<task-notification>` is what finishes it. Drop
                    // the running entry and broadcast `SubagentFinished` so the
                    // navigator badge / conversation indicator clears. The finish
                    // is id-keyed and kind-agnostic, so a background `Bash`
                    // (which also yields `SubagentCompleted` but never STARTED an
                    // indicator) is a harmless no-op here.
                    if self.state.finish_subagent(&tool_use_id) {
                        events.push(SessionEvent::SubagentFinished {
                            session_id: session.id.clone(),
                            tool_use_id,
                        });
                    }
                }
            }
        }

        self.store.upsert_messages(&messages).await?;
        Ok((messages, events))
    }
}
