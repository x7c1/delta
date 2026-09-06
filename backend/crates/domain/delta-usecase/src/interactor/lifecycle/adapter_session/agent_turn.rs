//! Running one turn on an adapter-backed session: writing its `send` row and
//! starting the turn over the provider's adapter.
//!
//! ## Turn-start / send-row model
//!
//! An adapter-backed turn does **not** use Claude's `Dispatch → AwaitingEcho →
//! PromptSubmitted` correlation: the adapter's `send` (Codex: `turn/start`)
//! returns synchronously and is the authoritative confirmation that the turn
//! started, so there is no echo to match. Routing such a send through the
//! Claude path would leave it `AwaitingEcho` for a `UserPromptSubmit` that
//! never comes: nothing would consume it, so the turn end would requeue and
//! re-type a message the provider has already accepted.
//!
//! So an adapter-backed turn is tracked as a prompt that consumed no send
//! ([`TurnInput::PromptSubmitted`] with `send_id: None` →
//! `InFlight { send_id: None }`): the FSM never references the send id, so a
//! later `TurnCompleted → Stop` transitions straight to `Idle` and orphans
//! nothing. The send **row** is completed out of band, at the `turn/start`
//! acknowledgement, by marking it matched to the provider's turn id — so it
//! leaves the open/`dispatched` set immediately rather than lingering. Claude's
//! FSM table is untouched.
//!
//! [`TurnInput::PromptSubmitted`]: crate::turn::TurnInput::PromptSubmitted

use std::sync::Arc;

use delta_model::{MessageUuid, Send, ThreadId};

use crate::agent::{AgentAdapter, AgentSessionHandle, SendRequest};
use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Dispatch one turn to a terminal-less agent over its bound adapter,
    /// writing the `send` row and then starting the turn.
    ///
    /// This is the adapter-backed dispatch path for every send that arrives
    /// against an already-open session (`enqueue_to_thread`): the row is written
    /// against `thread_id`, `dispatched`, carrying the `semantic_parent` and
    /// `locator_quote` the caller resolved (both `None` for a plain turn; a
    /// branch send passes the branch child thread as `thread_id`, the
    /// branched-from message as `semantic_parent`, and the selected passage as
    /// `locator_quote`), and the turn is started through
    /// [`Self::start_agent_turn`].
    ///
    /// The opening turn takes that same start step but not this row-writing one:
    /// its row was written `queued` by the accept phase, long before a provider
    /// thread existed to dispatch into.
    pub(in crate::interactor) async fn dispatch_agent_turn(
        &mut self,
        adapter: &Arc<dyn AgentAdapter>,
        handle: &AgentSessionHandle,
        thread_id: ThreadId,
        semantic_parent: Option<&MessageUuid>,
        text: String,
        locator_quote: Option<&str>,
    ) -> Result<Send> {
        let send = self
            .store
            .enqueue_send(self.id, thread_id, semantic_parent, &text, locator_quote)
            .await?;
        self.start_agent_turn(
            adapter,
            handle,
            send.id,
            thread_id,
            semantic_parent.cloned(),
            text,
        )
        .await?;
        Ok(send)
    }

    /// Start one turn on the adapter for an **already-written** `send` row, and
    /// complete that row at the acknowledgement.
    ///
    /// Shared by every adapter-backed turn — the opening one, whose row the
    /// accept phase wrote `queued`, and every later one, whose row
    /// [`Self::dispatch_agent_turn`] just wrote `dispatched`:
    ///
    /// 1. Route this turn's pushed content onto the lane the `send` row records
    ///    (a branch send folds its messages onto the branch child thread and
    ///    stamps the branched-from message on the root user prompt; a plain send
    ///    stays on `main` with no semantic parent). Set here — on the content
    ///    source the pump folds through, before `adapter.send` starts the turn —
    ///    so it is in place before any of the turn's item frames are ingested
    ///    (the pump posts them to this same actor mailbox, after this returns).
    /// 2. `adapter.send` starts the turn synchronously; on error, cancel the row
    ///    so it does not linger in the open list, then propagate.
    /// 3. Track the turn as a prompt that consumed no send (`send_id: None`):
    ///    the FSM never references this send id, so a later `TurnCompleted →
    ///    Stop` transitions to `Idle` without cancelling the successful send.
    ///    See the module docs for why an adapter-backed turn does not use
    ///    Claude's echo correlation.
    /// 4. Complete the `send` row at the `turn/start` acknowledgement, not by
    ///    echo: mark it matched to the provider's turn id (falling back to the
    ///    provider session id when the ack carried none), so it leaves the
    ///    open/`dispatched` set immediately.
    ///
    /// The turn's assistant frames arrive asynchronously through the event pump
    /// spawned at bind time — this returns as soon as the turn has started.
    pub(in crate::interactor) async fn start_agent_turn(
        &mut self,
        adapter: &Arc<dyn AgentAdapter>,
        handle: &AgentSessionHandle,
        send_id: i64,
        thread_id: ThreadId,
        semantic_parent: Option<MessageUuid>,
        text: String,
    ) -> Result<()> {
        // A Claude (pane-backed) session has no content source, so this is a
        // no-op there.
        self.state.begin_agent_turn(thread_id, semantic_parent);
        let receipt = match adapter.send(handle, SendRequest { text }).await {
            Ok(receipt) => receipt,
            Err(err) => {
                self.store.cancel_send(send_id).await?;
                return Err(err);
            }
        };
        self.apply_turn_input(crate::turn::TurnInput::PromptSubmitted { send_id: None })
            .await?;
        let matched = receipt
            .provider_message_id
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| handle.provider_session_id.clone());
        self.store
            .mark_send_matched(send_id, &MessageUuid::from(matched))
            .await?;
        Ok(())
    }
}
