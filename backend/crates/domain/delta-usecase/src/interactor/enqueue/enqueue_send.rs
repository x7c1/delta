use delta_model::{MessageUuid, Send, ThreadId};

use crate::agent::ContextInjectionCapability;
use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Enqueue a user input to a thread of this session.
    ///
    /// The routing layer already derived the owning session from the target
    /// thread (a stale or wrong id surfaced as a clean `ThreadNotFound` there,
    /// before reaching this actor). Here the session is ensured open — resumed
    /// via [`Self::open_session`] (`claude --resume <id>`) when it is known
    /// but closed — and then the normal pre-dispatch path runs: the `send`
    /// row is written *before* the keystrokes, so the correlation head is in
    /// place when the `UserPromptSubmit` hook fires, with the
    /// cancel-on-dispatch-failure rollback.
    ///
    /// A session whose own launch has not bound yet has nothing to dispatch
    /// into and nothing to resume from, so a plain send is **accepted as a
    /// `queued` row** there and flushed once the launch binds. A branch send in
    /// that window is refused ([`Error::SessionSpawning`]): it needs a message
    /// to branch from, and a session that has not bound has none.
    ///
    /// A branch send (`branch_from: Some`) requires an existing session —
    /// there must be a message to branch from — which the thread target
    /// inherently provides.
    ///
    /// Returns the created send plus any [`SessionEvent`]s the enqueue
    /// produced (e.g. a `send_dispatched` when the idle-flush promoted a
    /// previously queued send); the transport broadcasts them.
    pub(in crate::interactor) async fn enqueue_to_thread(
        &mut self,
        thread_id: ThreadId,
        branch_from: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<(Send, Vec<SessionEvent>)> {
        // A fresh spawn that has not bound yet is listed (as `spawning`) from
        // the moment its first send was accepted, so a browser can reach this
        // session's composer before the launch has produced a single hook —
        // indeed before the launch has been *prepared* at all, since the
        // worktree build and the agent launch run in the background. Nothing
        // can be *dispatched* into it, whatever the provider: a Claude spawn
        // has no pane mapped and no transcript yet, so `ensure_open()` below
        // would take the known-but-closed branch and launch a SECOND
        // `claude --resume <id>` against a conversation the first launch has not
        // written; an adapter-backed spawn has no provider thread yet, so the
        // reconnect just below would try to resume a NULL `provider_session_id`
        // and fail as a `5xx` instead of the honest "still starting".
        //
        // But "cannot dispatch yet" is not "cannot accept": a PR-origin session
        // can spend a minute checking out, and the user who already knows their
        // next message should not have to wait at a disabled button. So the
        // send is recorded as a `queued` row here — the same state a send
        // composed mid-turn takes — and the flush that runs when the session
        // binds types it in order, on the ordinary queued path. This branch is
        // explicit and terminal: it must never fall through to `ensure_open()`
        // below, which is exactly the rival-agent launch described above.
        //
        // No event accompanies it. There is no `send_queued` kind: a queued row
        // reaches clients as this call's own 201 body and through
        // `GET /api/sessions/{id}/sends`, which is how every other queued send
        // is announced today.
        //
        // A **branch** send stays refused (see the doc comment): a session that
        // has never bound has no message for `resolve_branch_target` to anchor
        // to.
        if self.state.is_launching_or_pending() {
            if branch_from.is_some() {
                return Err(Error::SessionSpawning(self.id.as_str().to_owned()));
            }
            let send = self
                .store
                .enqueue_queued_send(self.id, thread_id, None, text, locator_quote)
                .await?;
            tracing::info!(
                session_id = %self.id,
                send_id = send.id,
                "a send arrived while the session was still spawning; queued it \
                 to flush once the launch binds"
            );
            return Ok((send, Vec::new()));
        }

        // A closed adapter-backed session (e.g. Codex) — its in-process adapter
        // binding lost (e.g. across a server restart) but its persisted row +
        // provider ids intact — must be reconnected before it can dispatch, NOT
        // sent down Claude's `claude --resume` path (which a terminal-less
        // session cannot take: no pane, no transcript, so it would fail with
        // `ResumeUnavailable`). The registry predicate — not the provider's
        // identity — decides the branch, so any provider whose registered
        // factory declares an adapter-backed launch reattaches to its provider
        // thread via `thread/resume` here, and the `open_agent()` branch below
        // then dispatches over the freshly-bound adapter exactly like the
        // opening turn. A closed **Claude** session resolves no factory and
        // takes the pane path unchanged.
        if self.state.open_agent().is_none() {
            if let Some(session) = self.store.session(self.id).await? {
                if let Some(factory) = self.adapter_backed_factory(session.provider) {
                    self.resume_adapter_agent(&factory, &session).await?;
                }
            }
        }

        // A terminal-less (adapter-backed) session has no tmux pane and no
        // resumable transcript, so it cannot take Claude's `ensure_open()` →
        // `open_session()` (`claude --resume`) path: that would fail with
        // `ResumeUnavailable` on every send after the first. Dispatch it
        // through its bound adapter instead, exactly like the opening turn does
        // (see [`Self::dispatch_agent_turn`]). The non-destructive
        // `open_agent()` accessor tells the two paths apart: it is `Some` only
        // while an adapter session is live (either never closed, or just
        // reconnected above).
        if let Some(agent) = self.state.open_agent() {
            let adapter = agent.adapter.clone();
            let handle = agent.handle.clone();

            // Branch-from-selected-text is enabled by hidden-context injection,
            // NOT by native provider fork (`ForkCapability` is `None` for every
            // v1 provider). Gate a branch send on `ContextInjectionCapability`:
            // a provider that cannot inject hidden context (`None`) genuinely
            // cannot branch, so reject it cleanly rather than silently dropping
            // the branch intent. Codex is `HiddenPerTurn` (via
            // `thread/inject_items`), so it passes and branches like Claude.
            if branch_from.is_some()
                && adapter.capabilities().context_injection == ContextInjectionCapability::None
            {
                return Err(Error::Agent(format!(
                    "branching is not supported for a {:?} session: it cannot inject hidden context",
                    adapter.provider()
                )));
            }

            // Create the same delta-side branch structure Claude uses — a new
            // thread lane + semantic parent — through the shared
            // `resolve_branch_target`. A plain send leaves the target thread
            // unchanged with no semantic parent.
            let (target_thread, semantic_parent) = self
                .resolve_branch_target(thread_id, branch_from, locator_quote)
                .await?;

            // Deliver the branched-from passage as hidden context BEFORE the
            // turn dispatches, so the model sees it on this turn without it
            // polluting the visible prompt (Codex: `thread/inject_items`). Only
            // a branch send carries a quote to inject; a plain send injects
            // nothing.
            if branch_from.is_some() {
                if let Some(quote) = locator_quote {
                    adapter.inject_context(&handle, quote).await?;
                }
            }

            // A Codex dispatch produces no `SessionEvent`s synchronously: the
            // turn's frames arrive asynchronously through the already-running
            // event pump, just like the opening turn.
            let send = self
                .dispatch_agent_turn(
                    &adapter,
                    &handle,
                    target_thread,
                    semantic_parent.as_ref(),
                    text.to_owned(),
                    locator_quote,
                )
                .await?;
            return Ok((send, Vec::new()));
        }

        // Claude (pane-backed): ensure the session is open — resume it if it is
        // known but closed (no live pane). Once open we have a pane to dispatch
        // to and the normal pre-dispatch path applies.
        let pane = self.ensure_open().await?;
        self.enqueue_into_open(&pane, thread_id, text, locator_quote, branch_from)
            .await
    }
}
