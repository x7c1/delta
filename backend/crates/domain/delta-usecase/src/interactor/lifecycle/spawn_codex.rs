//! Terminal-less session creation for a structured provider (Codex).
//!
//! The Codex counterpart of [`spawn_fresh`](super::spawn_fresh): where a Claude
//! spawn mints a tmux pane, launches `claude`, and waits for the first
//! `UserPromptSubmit` hook to bind it, a Codex session is created entirely over
//! the `codex app-server` connection — there is no pane, no hook, and no
//! transcript file. This is the composition-layer half of provider dispatch;
//! the actor's `SpawnFresh` handler routes here on [`AgentProvider::Codex`] and
//! keeps the Claude path byte-for-byte unchanged.
//!
//! ## Turn-start / send-row model (the C3e-2 decision)
//!
//! Codex does **not** use Claude's `Dispatch → AwaitingEcho → EchoMatched`
//! correlation: `turn/start` returns synchronously and is the authoritative
//! confirmation that the turn started, so there is no echo to match. Routing a
//! Codex send through the Claude path would leave it `AwaitingEcho` and then
//! `CancelIfUnmatched` at turn end — cancelling a *successful* send, because
//! Codex never calls `mark_send_matched` from an echo.
//!
//! So a Codex turn is tracked **`ExternalPrompt`-style** ([`TurnInput::ExternalPrompt`]
//! → `InFlight { send_id: None }`): the FSM never references the send id, so a
//! later `TurnCompleted → Stop` transitions straight to `Idle` and orphans
//! nothing. The send **row** is completed out of band, at the `turn/start`
//! acknowledgement, by marking it matched to the provider's turn id — so it
//! leaves the open/`dispatched` set immediately rather than lingering. Claude's
//! FSM table is untouched.

use delta_model::{AgentProvider, MessageUuid};

use crate::agent::{LaunchRequest, SendRequest};
use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::OpenAgentSession;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::send_target::WorktreeSpec;

use super::FreshSpawn;

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Create a terminal-less Codex session, optionally delivering a first
    /// prompt as its opening turn.
    ///
    /// Connects the Codex adapter via the injected factory (which stands up the
    /// shared `codex app-server` and completes its handshake), starts a thread
    /// (`launch` → `thread/start`), persists the provider-minted conversation
    /// ids and activates the eager session row, and represents the running
    /// session as **open without a pane** in the runtime state. When a first
    /// prompt is given it starts the opening turn and completes the send row at
    /// the `turn/start` acknowledgement (see the module docs for the FSM
    /// decision).
    ///
    /// Rolls the eager session row back on any connect/launch failure, so a
    /// provider that is unavailable leaves no orphan row behind — mirroring
    /// `spawn_fresh`'s rollback on a failed tmux launch.
    pub(in crate::interactor) async fn spawn_codex(
        &mut self,
        first_prompt: Option<String>,
        workdir: Option<String>,
        launch_option_ids: Vec<i64>,
        worktree: Option<WorktreeSpec>,
    ) -> Result<FreshSpawn> {
        let session_id = self.id.clone();

        // A git worktree is a tmux/PTY-session concern, and per-provider launch
        // options are not yet modeled for Codex (they map to `thread/start`
        // fields, not argv flags). Reject a request carrying either rather than
        // silently dropping it — the Codex start flow in this slice takes only a
        // workdir and a first prompt. Neither is reachable from the UI yet.
        if worktree.is_some() {
            return Err(Error::Agent(
                "a git worktree is not supported for a Codex session".to_owned(),
            ));
        }
        if !launch_option_ids.is_empty() {
            return Err(Error::Agent(
                "launch options are not supported for a Codex session yet".to_owned(),
            ));
        }

        // The factory lazily stands up the Codex adapter (spawns `codex
        // app-server` + handshake). Absent means Codex was never wired into
        // this interactor — surface it rather than proceeding into a null path.
        let factory = self.codex_adapter_factory.clone().ok_or_else(|| {
            Error::Agent("no Codex adapter factory is wired into the interactor".to_owned())
        })?;

        // Validate a user-selected workdir before anything is created, so an
        // invalid path fails cleanly with no side effects (mirrors the workdir
        // gate in `spawn_fresh`). `None` falls back to a per-session directory
        // under the spawn base, keyed by the session id (there is no pane token
        // to key it by).
        let requested_workdir = match workdir {
            Some(dir) => Some(self.workspace.resolve_existing_dir(&dir).await?),
            None => None,
        };
        let cwd = match &requested_workdir {
            Some(dir) => dir.clone(),
            None => std::path::Path::new(&self.session_workdir_base)
                .join(session_id.as_str())
                .to_string_lossy()
                .into_owned(),
        };

        // Eagerly insert the `spawning` session row (provider = Codex). The
        // provider-minted ids are unknown until `launch` returns, so they stay
        // NULL here and are filled — and the row activated — via
        // `set_provider_ids` below. The git snapshot columns are left NULL: a
        // terminal-less session does no git detection in this slice.
        let (_session, main_thread_id) = self
            .store
            .insert_spawning_session(
                &session_id,
                &cwd,
                None,
                None,
                requested_workdir.as_deref(),
                None,
                AgentProvider::Codex,
            )
            .await?;

        // Stand up the adapter and start the thread. Both spawn a process /
        // issue an RPC, so either can fail; roll the eager row back on failure
        // (its main thread goes by cascade) so nothing dangles.
        let adapter = match factory.connect().await {
            Ok(adapter) => adapter,
            Err(err) => {
                self.rollback_codex_spawn().await;
                return Err(err);
            }
        };
        let handle = match adapter
            .launch(LaunchRequest {
                session_id: session_id.as_str().to_owned(),
                workdir: cwd.clone(),
                // Launch options are rejected above; a first prompt is delivered
                // as its own turn below (not on launch) so we can complete the
                // send row at the `turn/start` acknowledgement.
                extra_args: Vec::new(),
                first_prompt: None,
            })
            .await
        {
            Ok(handle) => handle,
            Err(err) => {
                self.rollback_codex_spawn().await;
                return Err(err);
            }
        };

        // Record the provider-minted ids and activate the row (spawning →
        // active). Session ↔ thread is 1:1, so both ids are the thread id.
        self.store
            .set_provider_ids(
                &session_id,
                Some(&handle.provider_session_id),
                Some(&handle.provider_session_id),
            )
            .await?;

        // Represent the running session as open-without-pane: hold the live
        // adapter + handle so the connection stays up and the session reads as
        // open, with no `OpenHandle` (so the PTY bridge has nothing to attach).
        self.state.bind_agent(OpenAgentSession {
            adapter: adapter.clone(),
            handle: handle.clone(),
        });

        // Build the push-based content accumulator for this session's event
        // stream and hand it to the runtime, so the event pump (spawned below)
        // can fold each pushed frame into canonical messages. A fresh Codex
        // session has nothing persisted yet, so the sequence is seeded at 0.
        self.state.set_agent_content_source(adapter.content_source(
            session_id.clone(),
            main_thread_id,
            0,
        ));

        // Spawn the event pump: it drains the adapter's `events()` stream and
        // posts each frame back to THIS actor as an `IngestAgentEvent`, so
        // control (turn machine), content (persistence), and streaming all run
        // in mailbox order. `events()` is handed out once per session; take it
        // here, after `bind_agent`, so the buffered opener (`SessionStarted`) and
        // the first turn's frames are all captured. Codex frames arrive after the
        // send below has already returned to the browser — exactly why they reach
        // the browser through the async seam rather than a synchronous return.
        crate::interactor::agent_event::spawn_codex_event_pump(
            self.self_sender.clone(),
            adapter.events(&handle),
        );

        let first_send = match first_prompt {
            Some(text) => {
                // The send row names the real session + main thread, exactly
                // like a Claude first send, so the REST response carries real
                // ids. It is written `dispatched`; the `turn/start` ack below
                // completes it.
                let send = self
                    .store
                    .enqueue_send(&session_id, main_thread_id, None, &text, None)
                    .await?;
                let receipt = match adapter.send(&handle, SendRequest { text }).await {
                    Ok(receipt) => receipt,
                    Err(err) => {
                        // The turn never started; drop the just-written send so
                        // it does not linger in the open list.
                        self.store.cancel_send(send.id).await?;
                        return Err(err);
                    }
                };
                // Track the turn ExternalPrompt-style (send_id: None): the FSM
                // never references this send id, so `TurnCompleted → Stop`
                // cannot cancel it. See the module docs.
                self.apply_turn_input(crate::turn::TurnInput::ExternalPrompt)
                    .await?;
                // Complete the send row at the `turn/start` acknowledgement, not
                // by echo: mark it matched to the provider's turn id (falling
                // back to the thread id when the ack carried no turn id), so it
                // leaves the open/`dispatched` set immediately.
                let matched = receipt
                    .provider_message_id
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| handle.provider_session_id.clone());
                self.store
                    .mark_send_matched(send.id, &MessageUuid::from(matched))
                    .await?;
                Some(send)
            }
            None => None,
        };

        tracing::info!(
            session_id = %session_id,
            provider_session_id = %handle.provider_session_id,
            has_first_prompt = first_send.is_some(),
            "codex session created (terminal-less); provider ids persisted"
        );
        Ok(FreshSpawn {
            token: None,
            first_send,
        })
    }

    /// Roll back the eagerly-inserted Codex session row after a connect/launch
    /// failure. Best-effort: a delete failure is logged, not surfaced, so the
    /// original launch error is what the caller sees.
    async fn rollback_codex_spawn(&mut self) {
        if let Err(err) = self.store.delete_session(self.id).await {
            tracing::error!(
                session_id = %self.id,
                error = %err,
                "failed to roll back the eager Codex session row after a launch failure"
            );
        }
    }
}
