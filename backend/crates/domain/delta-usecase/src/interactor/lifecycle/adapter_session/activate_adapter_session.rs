//! The actor's half of an adapter-backed launch: turning a connected,
//! thread-started adapter into a live session.

use std::sync::Arc;

use delta_model::{SendStatus, ThreadId};

use crate::agent::{AgentAdapter, AgentSessionHandle};
use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::{AdapterLaunch, LaunchingSpawn};
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Finish an adapter-backed launch off on the actor: persist the
    /// provider-minted ids, bind the live adapter, announce the session, and
    /// dispatch the first prompt.
    ///
    /// Everything here either mutates [`SessionRuntime`] or needs this actor's
    /// own mailbox handle, which is exactly why it is not on the launch task:
    /// the task did the slow, state-free part (`connect` + `thread/start`) and
    /// checks in here. The order is deliberate:
    ///
    /// 1. `set_provider_ids` records the ids and activates the row
    ///    (`spawning` → `active`). Session ↔ thread is 1:1, so both ids are the
    ///    thread id. It runs first because it is the last step that can fail
    ///    without leaving a live binding behind.
    /// 2. The binding is installed (adapter held, content source seeded at 0 —
    ///    a fresh session has nothing persisted — event pump started).
    /// 3. The first prompt, if any, is promoted and dispatched.
    /// 4. [`SessionEvent::SessionRegistered`] goes out on the async seam. This
    ///    is the browser's release signal for the spawn it is tracking (the
    ///    composer re-enables on it), so it is announced only once every claim
    ///    it makes about the session is true — and before anything this bind
    ///    dispatches, so no `SendDispatched` reaches the browser for a session
    ///    it has not seen registered. Its Claude counterpart is the first
    ///    `UserPromptSubmit` hook's registration, which an adapter-backed
    ///    session never has.
    /// 5. The queue is flushed ([`Self::flush_queued_send_async`]), covering the
    ///    rows this session accepted while it was still spawning.
    ///
    /// Returns `Err` on any failure, which the launch task reports as
    /// `LaunchFinished(Err)` so the standard rollback runs — including closing
    /// the adapter this bound, if it got that far.
    ///
    /// [`SessionRuntime`]: crate::interactor::session_actor::runtime::SessionRuntime
    /// [`SessionEvent::SessionRegistered`]: crate::ports::SessionEvent::SessionRegistered
    pub(in crate::interactor) async fn activate_adapter_session(
        &mut self,
        launching: &LaunchingSpawn,
        spec: &AdapterLaunch,
        adapter: Arc<dyn AgentAdapter>,
        handle: AgentSessionHandle,
        git_branch: Option<String>,
    ) -> Result<()> {
        let session_id = self.id.clone();
        self.store
            .set_provider_ids(
                &session_id,
                Some(&handle.provider_session_id),
                Some(&handle.provider_session_id),
            )
            .await?;
        self.install_agent_binding(
            adapter.clone(),
            handle.clone(),
            launching.workdir.clone(),
            git_branch,
            spec.main_thread_id,
            0,
        );
        if let Some(send_id) = spec.first_send_id {
            self.dispatch_first_agent_prompt(&adapter, &handle, send_id, spec.main_thread_id)
                .await?;
        }
        tracing::info!(
            session_id = %session_id,
            provider = spec.provider.as_str(),
            provider_session_id = %handle.provider_session_id,
            has_first_prompt = spec.first_send_id.is_some(),
            launched_in_ms = launching.accepted_at.elapsed().as_millis(),
            "adapter-backed session bound (terminal-less); provider ids persisted"
        );
        self.emit_async_event(crate::ports::SessionEvent::SessionRegistered { session_id });
        // A session accepts plain sends as `queued` rows for as long as it is
        // spawning, and this bind is the moment they become dispatchable. The
        // first prompt above usually covers that: dispatching it leaves the
        // turn `InFlight`, so this flush finds a busy turn and does nothing,
        // and the rest of the queue drains at that turn's `turn/completed`.
        // What it covers is the launch that starts no turn at all — a spawn
        // that carried no first prompt, or one whose first prompt was cancelled
        // during the launch window. No turn end is coming there, so without
        // this flush every row accepted in that window would sit `queued` until
        // the user sent again. The turn-idle guard inside makes it exactly one
        // dispatch either way.
        //
        // Called, not posted: unlike the Claude bind (`bind_pending_spawn`),
        // which runs inside a hook that blocks `claude` and so must not type
        // into a TUI that is not yet accepting input, this already runs on the
        // actor, and an adapter-backed send is a request rather than a
        // keystroke.
        self.flush_queued_send_async().await;
        Ok(())
    }

    /// Promote and dispatch the first prompt an adapter-backed spawn accepted,
    /// now that its provider thread exists.
    ///
    /// The row was written `queued` by the accept phase — nothing had received
    /// it — so there are two steps: promote it to `dispatched`, then start its
    /// turn through the shared [`Self::start_agent_turn`], exactly as every
    /// later send does.
    ///
    /// The row is read back rather than carried along, and dispatched only while
    /// it is still `queued`: a `queued` send can be cancelled by the browser,
    /// and the launch window is easily long enough for that to happen. A row
    /// that has left `queued` is skipped, so a cancelled first prompt is not
    /// resurrected by the launch that outran it.
    async fn dispatch_first_agent_prompt(
        &mut self,
        adapter: &Arc<dyn AgentAdapter>,
        handle: &AgentSessionHandle,
        send_id: i64,
        thread_id: ThreadId,
    ) -> Result<()> {
        let Some(send) = self.store.send(send_id).await? else {
            tracing::warn!(
                session_id = %self.id,
                send_id,
                "the accepted first prompt's row is gone by the time its provider \
                 thread started; nothing to dispatch"
            );
            return Ok(());
        };
        if send.status != SendStatus::Queued {
            tracing::info!(
                session_id = %self.id,
                send_id,
                status = ?send.status,
                "the accepted first prompt left the queue during the launch; \
                 not dispatching it"
            );
            return Ok(());
        }
        self.store.promote_queued_send(send_id).await?;
        self.start_agent_turn(adapter, handle, send_id, thread_id, None, send.text)
            .await
    }
}
