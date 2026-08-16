//! The per-session actor: one tokio task owning one session's runtime state.
//!
//! The actor loop pulls [`SessionInput`]s off its mailbox and executes them
//! strictly in order against the session's [`SessionRuntime`]. That ordering
//! is the whole point: hooks, API commands, ticks, and permission decisions
//! for one session can no longer interleave, while different sessions proceed
//! fully in parallel (the old design serialized *all* sessions' transcript
//! ingestion behind one global lock).
//!
//! ## Retirement
//!
//! An actor whose runtime state is empty (closed, no launch in flight, idle
//! turn, no waiters — see [`SessionRuntime::is_empty`]) is indistinguishable
//! from no actor at all, so it retires instead of parking forever: it locks
//! the registry map, re-checks its mailbox is empty (posting happens under
//! the same lock, so this check is race-free), removes its own entry, and
//! exits. If a message slipped in first, retirement is abandoned and the
//! message is processed normally. Messages posted after removal spawn a fresh
//! actor whose default state means exactly the same thing — so per-session
//! ordering across the handover is preserved: every message before removal is
//! handled by the old actor (which only exits on a provably empty mailbox),
//! and every message after goes to the new one.

use std::sync::{Arc, Mutex, Weak};

use delta_model::SessionId;
use tokio::sync::mpsc;

use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

use super::input::SessionInput;
use super::registry::ActorMap;
use super::runtime::SessionRuntime;

/// One session's execution context: the shared core plus the actor-owned
/// runtime state. Every use-case method that used to lock a shared registry
/// now runs as a method on this context, reading and mutating
/// [`SessionRuntime`] directly — the mailbox already serialized it.
///
/// Derefs to the core so port access (`self.store`, `self.tmux`, …) and the
/// core's pure helpers keep their existing call syntax.
pub(in crate::interactor) struct SessionContext<'a, T, X, S, W, G> {
    pub(in crate::interactor) core: &'a InteractorCore<T, X, S, W, G>,
    /// The session this actor exists for. Hook payloads carry the same id;
    /// the routing layer guarantees they match.
    pub(in crate::interactor) id: &'a SessionId,
    pub(in crate::interactor) state: &'a mut SessionRuntime,
    /// A weak handle to this actor's own mailbox, so a use case running here
    /// can spawn a background task (the Codex event pump) that posts more
    /// inputs *back to this same actor* — keeping every signal on one ordered
    /// mailbox. Weak so the spawned task never keeps the actor alive: when the
    /// registry drops the strong sender (actor retired / interactor gone), the
    /// upgrade fails and the task stops.
    pub(in crate::interactor) self_sender: &'a mpsc::WeakUnboundedSender<SessionInput>,
}

impl<T, X, S, W, G> std::ops::Deref for SessionContext<'_, T, X, S, W, G> {
    type Target = InteractorCore<T, X, S, W, G>;

    fn deref(&self) -> &Self::Target {
        self.core
    }
}

/// The actor loop. Spawned by the registry on a session's first contact; runs
/// until its mailbox closes (the registry dropped) or it retires.
pub(in crate::interactor) async fn run<T, X, S, W, G>(
    core: Arc<InteractorCore<T, X, S, W, G>>,
    id: SessionId,
    mut mailbox: mpsc::UnboundedReceiver<SessionInput>,
    self_sender: mpsc::WeakUnboundedSender<SessionInput>,
    registry: Weak<Mutex<ActorMap>>,
) where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    let mut state = SessionRuntime::default();
    let mut carried: Option<SessionInput> = None;
    loop {
        let input = match carried.take() {
            Some(input) => input,
            None => match mailbox.recv().await {
                Some(input) => input,
                None => break,
            },
        };
        let mut ctx = SessionContext {
            core: &core,
            id: &id,
            state: &mut state,
            self_sender: &self_sender,
        };
        handle(&mut ctx, input).await;

        if state.is_empty() {
            // Retire. The mailbox-empty check happens under the registry
            // lock that all posting goes through, so it cannot race a post:
            // either the entry is removed with a provably empty mailbox, or
            // the raced-in message is carried into the next iteration.
            let Some(registry) = registry.upgrade() else {
                break;
            };
            let mut map = registry.lock().expect("actor registry poisoned");
            match mailbox.try_recv() {
                Ok(input) => carried = Some(input),
                Err(mpsc::error::TryRecvError::Empty) => {
                    map.remove(&id);
                    return;
                }
                Err(mpsc::error::TryRecvError::Disconnected) => return,
            }
        }
    }
}

/// Execute one input against the session, sending the result down its reply
/// channel. A dropped reply receiver only means the caller went away; the
/// state change has already happened, so it is not an error here.
async fn handle<T, X, S, W, G>(ctx: &mut SessionContext<'_, T, X, S, W, G>, input: SessionInput)
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    match input {
        SessionInput::EnqueueToThread {
            thread_id,
            branch_from,
            text,
            locator_quote,
            reply,
        } => {
            let result = ctx
                .enqueue_to_thread(
                    thread_id,
                    branch_from.as_ref(),
                    &text,
                    locator_quote.as_deref(),
                )
                .await;
            let _ = reply.send(result);
        }
        SessionInput::SpawnFresh {
            first_prompt,
            workdir,
            launch_option_ids,
            worktree,
            provider,
            reply,
        } => {
            // Provider dispatch lives in composition, never in the core's turn
            // or attribution logic: Claude keeps the tmux + hooks spawn path
            // byte-for-byte, while every other provider takes the terminal-less
            // adapter path, resolving its adapter through the factory registry
            // (`adapter_backed_factory`). This `match` is the only place the
            // provider is branched on for launch, and only to split the one
            // PTY-native provider from the adapter-backed rest — a new
            // adapter-backed provider lands in the catch-all arm with no change
            // here.
            let result = match provider {
                delta_model::AgentProvider::Claude => {
                    ctx.spawn_fresh(first_prompt, workdir, launch_option_ids, worktree)
                        .await
                }
                provider => {
                    ctx.spawn_adapter_session(
                        provider,
                        first_prompt,
                        workdir,
                        launch_option_ids,
                        worktree,
                    )
                    .await
                }
            };
            let _ = reply.send(result);
        }
        SessionInput::OpenSession { reply } => {
            let _ = reply.send(ctx.open_session().await);
        }
        SessionInput::CloseSession { reply } => {
            let _ = reply.send(ctx.close_session().await);
        }
        SessionInput::Interrupt { reply } => {
            let _ = reply.send(ctx.interrupt().await);
        }
        SessionInput::ClearInput { reply } => {
            let _ = reply.send(ctx.clear_session_input().await);
        }
        SessionInput::UserPromptSubmit { hook, reply } => {
            let _ = reply.send(ctx.on_user_prompt_submit(hook).await);
        }
        SessionInput::Stop { hook, reply } => {
            let _ = reply.send(ctx.on_stop(hook).await);
        }
        SessionInput::MessageDisplay { hook, reply } => {
            let _ = reply.send(ctx.on_message_display(hook).await);
        }
        SessionInput::SessionStart { hook, reply } => {
            let _ = reply.send(ctx.on_session_start(hook).await);
        }
        SessionInput::SessionEnd { hook, reply } => {
            let _ = reply.send(ctx.on_session_end(hook).await);
        }
        SessionInput::PreToolUse {
            tool_name,
            tool_input_json,
            tool_use_id,
            transcript_path,
            reply,
        } => {
            let _ = reply.send(
                ctx.on_pre_tool_use(&tool_name, &tool_input_json, &tool_use_id, &transcript_path)
                    .await,
            );
        }
        SessionInput::PostToolUse {
            tool_name,
            tool_use_id,
            tool_response_json,
            transcript_path,
            reply,
        } => {
            let _ = reply.send(
                ctx.on_post_tool_use(
                    &tool_name,
                    &tool_use_id,
                    &tool_response_json,
                    &transcript_path,
                )
                .await,
            );
        }
        SessionInput::PermissionRequest {
            tool_name,
            tool_input_json,
            transcript_path,
            reply,
        } => {
            let _ = reply.send(
                ctx.on_permission_request(&tool_name, &tool_input_json, &transcript_path)
                    .await,
            );
        }
        SessionInput::DecidePermission {
            request_id,
            decision,
            reply,
        } => {
            let _ = reply.send(ctx.decide_permission(request_id, decision).await);
        }
        SessionInput::AbandonPermission { request_id } => {
            ctx.abandon_permission_decision(request_id);
        }
        SessionInput::AnswerQuestion {
            request_id,
            selections,
            reply,
        } => {
            let _ = reply.send(ctx.answer_question(request_id, &selections).await);
        }
        SessionInput::CancelQuestion { request_id, reply } => {
            let _ = reply.send(ctx.cancel_question(request_id).await);
        }
        SessionInput::CancelSend { send_id, reply } => {
            let _ = reply.send(ctx.cancel_send(send_id).await);
        }
        SessionInput::ReleaseSend { send_id, reply } => {
            let _ = reply.send(ctx.release_send(send_id).await);
        }
        SessionInput::IngestAgentEvent { event } => {
            ctx.on_agent_event(event).await;
        }
        SessionInput::SyncTick { reply } => {
            let _ = reply.send(ctx.sync_tick().await);
        }
        SessionInput::ResumeTick { now, reply } => {
            let _ = reply.send(ctx.dispatch_ready_resume(now).await);
        }
        SessionInput::ReapTick { now, reply } => {
            let _ = reply.send(ctx.reap_stale_launch(now).await);
        }
        SessionInput::EchoDeadlineTick { now, reply } => {
            let _ = reply.send(ctx.sweep_echo_deadline(now).await);
        }
        SessionInput::QueryPane { reply } => {
            let _ = reply.send(ctx.state.handle().map(|h| h.pane.clone()));
        }
        SessionInput::QueryIsOpen { reply } => {
            let _ = reply.send(ctx.state.is_open());
        }
        SessionInput::QueryIsLive { reply } => {
            let _ = reply.send(ctx.state.has_live_pane());
        }
        SessionInput::QueryLiveState { reply } => {
            let mut live = ctx.state.live_state();
            // Resolve the in-flight turn's thread so a reconnecting client can
            // re-seed its per-thread running indicator on the exact thread. Only
            // meaningful while a turn is in flight; an idle session has no
            // running thread, so leave it `None`.
            if !matches!(ctx.state.turn(), crate::turn::TurnState::Idle) {
                live.in_progress_thread = ctx.store.in_progress_turn_thread(ctx.id).await.ok();
            }
            let _ = reply.send(live);
        }
        #[cfg(test)]
        SessionInput::WithRuntime(f) => f(ctx.state),
    }
}
