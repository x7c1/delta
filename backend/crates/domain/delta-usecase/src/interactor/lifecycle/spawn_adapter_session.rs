//! Terminal-less session creation for an adapter-backed provider (currently
//! Codex).
//!
//! The adapter-backed counterpart of [`spawn_fresh`](super::spawn_fresh): where
//! a Claude spawn mints a tmux pane, launches `claude`, and waits for the first
//! `UserPromptSubmit` hook to bind it, an adapter-backed session is created
//! entirely over the provider's adapter connection (Codex: `codex app-server`)
//! — there is no pane, no hook, and no transcript file. This is the
//! composition-layer half of provider dispatch; the actor's `SpawnFresh`
//! handler routes every non-Claude provider here and keeps the Claude path
//! byte-for-byte unchanged. Which adapter drives the session is resolved
//! through the factory registry
//! ([`InteractorCore::adapter_backed_factory`](crate::interactor::InteractorCore::adapter_backed_factory)),
//! so a new adapter-backed provider is a new registered factory, not a new
//! spawn path.
//!
//! ## Turn-start / send-row model (the C3e-2 decision)
//!
//! An adapter-backed turn does **not** use Claude's `Dispatch → AwaitingEcho →
//! EchoMatched` correlation: the adapter's `send` (Codex: `turn/start`) returns
//! synchronously and is the authoritative confirmation that the turn started,
//! so there is no echo to match. Routing such a send through the Claude path
//! would leave it `AwaitingEcho` and then `CancelIfUnmatched` at turn end —
//! cancelling a *successful* send, because the adapter never calls
//! `mark_send_matched` from an echo.
//!
//! So an adapter-backed turn is tracked **`ExternalPrompt`-style** ([`TurnInput::ExternalPrompt`]
//! → `InFlight { send_id: None }`): the FSM never references the send id, so a
//! later `TurnCompleted → Stop` transitions straight to `Idle` and orphans
//! nothing. The send **row** is completed out of band, at the `turn/start`
//! acknowledgement, by marking it matched to the provider's turn id — so it
//! leaves the open/`dispatched` set immediately rather than lingering. Claude's
//! FSM table is untouched.

use std::sync::Arc;

use delta_model::{AgentProvider, MessageUuid, Send, Session, ThreadId};

use crate::agent::{
    AgentAdapter, AgentAdapterFactory, AgentSessionHandle, ContentSourceRequest, LaunchOptionSpec,
    LaunchRequest, ResumeRequest, SendRequest,
};
use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::OpenAgentSession;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::repository::{display_name, identity_key};
use crate::send_target::WorktreeSpec;

use super::FreshSpawn;

/// How to obtain the provider handle when standing up an agent binding: a
/// fresh `thread/start` (launch) or a `thread/resume` reattach to an existing
/// provider thread. The single difference the shared
/// [`SessionContext::bind_adapter_agent`] branches on.
enum AdapterBind {
    /// A fresh spawn: start a new provider thread (`adapter.launch`), carrying
    /// the launch options the user selected for it. Only a fresh thread takes
    /// them — see [`SessionContext::resume_adapter_agent`] for why a resume
    /// does not.
    Launch {
        launch_options: Vec<LaunchOptionSpec>,
    },
    /// A resume: reattach to the persisted provider thread (`adapter.resume`),
    /// so no new thread is minted.
    Resume { provider_session_id: String },
}

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Create a terminal-less session for an adapter-backed provider,
    /// optionally delivering a first prompt as its opening turn.
    ///
    /// Connects the provider's adapter via its registered factory (which stands
    /// up the backing connection — Codex: the shared `codex app-server` and its
    /// handshake), starts a thread (`launch` → `thread/start`), persists the
    /// provider-minted conversation ids and activates the eager session row,
    /// and represents the running session as **open without a pane** in the
    /// runtime state. When a first prompt is given it starts the opening turn
    /// and completes the send row at the `turn/start` acknowledgement (see the
    /// module docs for the FSM decision).
    ///
    /// `launch_option_ids` are the registered launch options the user selected
    /// for this session, in selection order. They are resolved here to their
    /// neutral `(name, value?)` records and handed to the adapter on the launch
    /// request; the adapter renders them for its provider (Codex maps them onto
    /// `thread/start` fields). A selected id no longer in the registry is
    /// skipped with a warning rather than aborting the launch, exactly as on
    /// the Claude path.
    ///
    /// Rolls the eager session row back on any connect/launch failure, so a
    /// provider that is unavailable — or one that rejects a launch option —
    /// leaves no orphan row behind, mirroring `spawn_fresh`'s rollback on a
    /// failed tmux launch.
    pub(in crate::interactor) async fn spawn_adapter_session(
        &mut self,
        provider: AgentProvider,
        first_prompt: Option<String>,
        workdir: Option<String>,
        launch_option_ids: Vec<i64>,
        worktree: Option<WorktreeSpec>,
    ) -> Result<FreshSpawn> {
        let session_id = self.id.clone();

        // Resolve the user-selected launch options up front, before anything is
        // created — the same side-effect-free gate the Claude path runs (see
        // [`Self::resolve_launch_options`]). They travel to the adapter as
        // neutral `(name, value?)` pairs and are rendered there (Codex maps
        // them onto `thread/start` fields), so this layer never learns a
        // provider's launch wire shape. A git worktree, by contrast, is just a
        // working directory: it is resolved below exactly like the Claude path,
        // so a session started from a PR (which always arrives as a
        // `UseRemoteBranch` worktree request) lands in that PR's worktree.
        let launch_options = self.resolve_launch_options(&launch_option_ids).await?;

        // The registered factory lazily stands up the provider's adapter
        // (Codex: spawns `codex app-server` + handshake). Absent means the
        // provider was never wired into this interactor — surface it rather
        // than proceeding into a null path.
        let factory = self.adapter_backed_factory(provider).ok_or_else(|| {
            Error::Agent(format!(
                "no {provider:?} adapter factory is wired into the interactor"
            ))
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

        // Resolve an opt-in worktree request exactly like the Claude path: a
        // worktree needs a selected directory that is a git repository
        // (`WorktreeRequiresWorkdir` / `WorktreeNotAGitRepo` otherwise — both
        // rejected before any side effect), and the effective launch directory
        // becomes the per-session worktree under `$DELTA_WORKTREE_BASE`. A
        // PR-origin start always arrives as a `UseRemoteBranch(<pr-head>)`
        // worktree request, so this is the path a "start a session from a PR"
        // click takes. `launch_repo_root` / `repository_display_name` feed
        // the navigator's repo line (a worktree is always a git working tree),
        // and `branch_at_launch` records the branch the conversation started on
        // — the same columns `spawn_fresh` fills. Without a worktree the cwd is
        // the user-selected dir (its git snapshot columns stay NULL, as before)
        // or a per-session scratch dir under the spawn base, keyed by the
        // session id (there is no pane token to key it by).
        let (launch_repo_root, repository_display_name, branch_at_launch, cwd) = match worktree {
            Some(spec) => {
                let Some(dir) = requested_workdir.as_deref() else {
                    return Err(Error::WorktreeRequiresWorkdir);
                };
                let repo_root = match self.git_worktree.repo_root(dir).await? {
                    Some(root) => root,
                    None => return Err(Error::WorktreeNotAGitRepo(dir.to_owned())),
                };
                // Snapshot the repo's short identity (from `origin`, falling back
                // to the working-tree basename) — it names the worktree directory
                // and feeds the navigator card, mirroring `spawn_fresh`.
                let origin = self.git_worktree.origin_url(&repo_root).await?;
                let identity = identity_key(origin, &repo_root);
                let display = display_name(&identity, &repo_root);
                let path = self
                    .resolve_worktree_launch_dir(&session_id, &repo_root, Some(&display), spec)
                    .await?;
                let branch = self.git_worktree.current_branch(&path).await?;
                (Some(repo_root), Some(display), branch, path)
            }
            None => {
                let cwd = match &requested_workdir {
                    Some(dir) => dir.clone(),
                    None => std::path::Path::new(&self.session_workdir_base)
                        .join(session_id.as_str())
                        .to_string_lossy()
                        .into_owned(),
                };
                (None, None, None, cwd)
            }
        };

        // Eagerly insert the `spawning` session row for this provider. The
        // provider-minted ids are unknown until `launch` returns, so they stay
        // NULL here and are filled — and the row activated — via
        // `set_provider_ids` below. The git snapshot columns carry the worktree
        // spawn's repo/branch identity (filled above); for a plain workdir with
        // no worktree they stay NULL, as before.
        let (_session, main_thread_id) = self
            .store
            .insert_spawning_session(
                &session_id,
                &cwd,
                branch_at_launch.as_deref(),
                launch_repo_root.as_deref(),
                requested_workdir.as_deref(),
                repository_display_name.as_deref(),
                provider,
            )
            .await?;

        // Stand up the adapter, start the thread (`launch` → `thread/start`),
        // bind it as the session's open agent, seed the content source at 0 (a
        // fresh adapter-backed session has nothing persisted yet), and spawn
        // the event pump — the shared connect/bind/pump/content-source sequence
        // a resume reuses (see [`Self::bind_adapter_agent`]). Any failure here
        // spawned a process / issued an RPC that did not complete, so roll the
        // eager row back (its main thread goes by cascade) so nothing dangles.
        let (adapter, handle) = match self
            .bind_adapter_agent(
                &factory,
                AdapterBind::Launch { launch_options },
                cwd.clone(),
                main_thread_id,
                0,
            )
            .await
        {
            Ok(pair) => pair,
            Err(err) => {
                self.rollback_adapter_spawn().await;
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

        let first_send = match first_prompt {
            // The opening turn is dispatched exactly like every subsequent
            // adapter-backed turn (see [`Self::dispatch_agent_turn`]): the send
            // row names the real session + main thread so the REST response
            // carries real ids, and it is completed at the `turn/start`
            // acknowledgement.
            Some(text) => Some(
                self.dispatch_agent_turn(&adapter, &handle, main_thread_id, None, text, None)
                    .await?,
            ),
            None => None,
        };

        tracing::info!(
            session_id = %session_id,
            provider = provider.as_str(),
            provider_session_id = %handle.provider_session_id,
            has_first_prompt = first_send.is_some(),
            "adapter-backed session created (terminal-less); provider ids persisted"
        );
        Ok(FreshSpawn {
            token: None,
            first_send,
        })
    }

    /// Dispatch one turn to a terminal-less agent over its bound adapter,
    /// writing and completing the `send` row the same way the opening turn
    /// does.
    ///
    /// This is the single adapter-backed turn-dispatch path, shared by the
    /// opening turn ([`Self::spawn_adapter_session`]) and every subsequent send
    /// (`enqueue_to_thread`):
    ///
    /// 1. Write the `send` row against `thread_id`, `dispatched`, carrying the
    ///    `semantic_parent` and `locator_quote` the caller resolved (both `None`
    ///    for a plain turn; a branch send passes the branch child thread as
    ///    `thread_id`, the branched-from message as `semantic_parent`, and the
    ///    selected passage as `locator_quote`).
    /// 2. `adapter.send` starts the turn synchronously; on error, cancel the
    ///    just-written row so it does not linger in the open list, then
    ///    propagate — the same rollback the opening turn used.
    /// 3. Track the turn **`ExternalPrompt`-style** (`send_id: None`): the FSM
    ///    never references this send id, so a later `TurnCompleted → Stop`
    ///    transitions to `Idle` without cancelling the successful send. See the
    ///    module docs for why an adapter-backed turn does not use Claude's echo
    ///    correlation.
    /// 4. Complete the `send` row at the `turn/start` acknowledgement, not by
    ///    echo: mark it matched to the provider's turn id (falling back to the
    ///    provider session id when the ack carried none), so it leaves the
    ///    open/`dispatched` set immediately.
    ///
    /// The turn's assistant frames arrive asynchronously through the event pump
    /// spawned once at session creation — this returns as soon as the turn has
    /// started, exactly like the opening turn.
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
        // Route this turn's pushed content onto the same lane the `send` row just
        // recorded: a branch send folds its messages onto the branch child thread
        // and stamps the branched-from message on the root user prompt, a plain
        // send stays on `main` with no semantic parent. Set here — on the content
        // source the pump folds through, before `adapter.send` starts the turn —
        // so it is in place before any of the turn's item frames are ingested
        // (the pump posts them to this same actor mailbox, after this dispatch
        // returns). A Claude (pane-backed) session has no content source, so this
        // is a no-op there.
        self.state
            .begin_agent_turn(thread_id, semantic_parent.cloned());
        let receipt = match adapter.send(handle, SendRequest { text }).await {
            Ok(receipt) => receipt,
            Err(err) => {
                self.store.cancel_send(send.id).await?;
                return Err(err);
            }
        };
        self.apply_turn_input(crate::turn::TurnInput::ExternalPrompt)
            .await?;
        let matched = receipt
            .provider_message_id
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| handle.provider_session_id.clone());
        self.store
            .mark_send_matched(send.id, &MessageUuid::from(matched))
            .await?;
        Ok(send)
    }

    /// Connect the provider's adapter and bind it as this session's open agent
    /// — the shared connect → (`launch`|`resume`) → bind → content-source →
    /// event-pump sequence used by BOTH a fresh spawn
    /// ([`Self::spawn_adapter_session`]) and a resume
    /// ([`Self::resume_adapter_agent`]).
    ///
    /// The only two things that differ between the callers are passed in:
    ///
    /// - `bind` selects the provider handle — [`AdapterBind::Launch`] starts a
    ///   new thread, [`AdapterBind::Resume`] reattaches to the persisted one (no
    ///   new thread is minted);
    /// - `seed_seq` is where the content accumulator begins numbering — `0` for a
    ///   fresh session, the session's persisted `MAX(seq) + 1` on a resume so
    ///   replayed/continued frames extend the existing history instead of
    ///   renumbering or duplicating it.
    ///
    /// `cwd` is the session's launch directory as Delta resolved and recorded it
    /// — the value both callers already hold (the fresh spawn its just-resolved
    /// one, the resume the persisted row's). It is passed on to the content
    /// accumulator so every message reports where the agent is running; the
    /// provider's own facts about the session (model, branch) are the adapter's
    /// to add.
    ///
    /// It holds the live adapter + handle in the runtime (so the connection stays
    /// up and the session reads as open, with no `OpenHandle` for the PTY bridge
    /// to attach to), then spawns the event pump that drains the adapter's
    /// `events()` onto THIS actor's mailbox — so control (turn machine), content
    /// (persistence), and streaming all run in mailbox order. `events()` is taken
    /// after `bind_agent`, so the buffered opener (`SessionStarted`) and the
    /// first frames are all captured.
    ///
    /// Returns the live adapter + handle now bound. It performs no rollback: a
    /// fresh spawn deletes its eager row on failure, while a resume leaves the
    /// already-persisted row untouched — so the caller owns that decision.
    async fn bind_adapter_agent(
        &mut self,
        factory: &Arc<dyn AgentAdapterFactory>,
        bind: AdapterBind,
        cwd: String,
        main_thread_id: ThreadId,
        seed_seq: i64,
    ) -> Result<(Arc<dyn AgentAdapter>, AgentSessionHandle)> {
        let session_id = self.id.clone();
        let adapter = factory.connect().await?;
        let handle = match bind {
            AdapterBind::Launch { launch_options } => {
                adapter
                    .launch(LaunchRequest {
                        session_id: session_id.as_str().to_owned(),
                        workdir: cwd.clone(),
                        // The adapter renders these for its provider. A first
                        // prompt is delivered as its own turn (not on launch) so
                        // the send row completes at the `turn/start`
                        // acknowledgement.
                        launch_options,
                        first_prompt: None,
                    })
                    .await?
            }
            AdapterBind::Resume {
                provider_session_id,
            } => {
                adapter
                    .resume(ResumeRequest {
                        session_id: session_id.as_str().to_owned(),
                        provider_session_id,
                        workdir: cwd.clone(),
                    })
                    .await?
            }
        };

        // Represent the running session as open-without-pane: hold the live
        // adapter + handle so the connection stays up and the session reads as
        // open, with no `OpenHandle` (so the PTY bridge has nothing to attach).
        self.state.bind_agent(OpenAgentSession {
            adapter: adapter.clone(),
            handle: handle.clone(),
        });

        // Build the push-based content accumulator: seeded so minted ordering
        // continues past whatever is already persisted, and carrying the launch
        // directory so every message it folds reports where the agent is
        // running. The adapter joins this with the facts only it knows (Codex:
        // the model the server resolved and the branch it saw, both read off the
        // thread's opening response), which is why it is handed the live handle
        // too.
        self.state.set_agent_content_source(adapter.content_source(
            &handle,
            ContentSourceRequest {
                session_id,
                main_thread: main_thread_id,
                seed_seq,
                cwd,
            },
        ));

        // Spawn the event pump. Adapter frames arrive after the send that
        // started the work has already returned to the browser — exactly why
        // they reach the browser through the async seam rather than a
        // synchronous return.
        crate::interactor::agent_event::spawn_agent_event_pump(
            self.self_sender.clone(),
            adapter.events(&handle),
        );

        Ok((adapter, handle))
    }

    /// Reconnect a **closed** adapter-backed session by resuming its provider
    /// thread, so a send that arrives after the in-process binding was lost
    /// (e.g. across a server restart) can dispatch over the adapter instead of
    /// falling into Claude's `claude --resume` path (which a terminal-less
    /// session cannot take — it has no pane and no transcript).
    ///
    /// This is the adapter-backed mirror of a fresh spawn: it runs the same
    /// [`Self::bind_adapter_agent`] sequence, but with `adapter.resume` against
    /// the session's **persisted** provider id (reattaching to the same thread)
    /// and the content source **seeded at the session's persisted message
    /// count** — which, for a single-thread adapter-backed session whose seqs
    /// are minted densely from 0, is exactly `MAX(seq) + 1`. Seeding at 0 (as a
    /// fresh spawn does) would renumber/duplicate the existing history.
    ///
    /// **Launch options are deliberately not re-applied here.** They configure
    /// the provider *thread*, which the resume reattaches to rather than mints:
    /// Codex's `thread/resume` takes its config fields as optional *overrides*
    /// of what the resumed thread already carries, so sending none keeps the
    /// thread exactly as `thread/start` configured it. Delta also has no
    /// per-session record of which options were selected (the registry is
    /// session-independent and the `session` row stores no selection), so there
    /// is nothing to replay. This matches the Claude path, where a resume is
    /// `claude --settings … --resume <id>` with none of the launch flags the
    /// original spawn carried.
    ///
    /// The session's **provider metadata is still reported after a resume**, and
    /// is re-read rather than remembered: the launch directory comes from the
    /// persisted row (which outlives the restart by definition), and the model
    /// and branch from the `thread/resume` response — Codex's resume response
    /// carries the same `model` and `thread.gitInfo` its start response does, so
    /// the reattached thread re-announces what it is running and where. Nothing
    /// about a resumed session's metadata degrades relative to a fresh one.
    ///
    /// The caller resolves `factory` through the registry
    /// ([`InteractorCore::adapter_backed_factory`](crate::interactor::InteractorCore::adapter_backed_factory))
    /// — the same predicate that decided the session is adapter-backed at all.
    /// The persisted provider ids and session row are the source of truth that
    /// survives the restart; on failure the row is left as-is (unlike a fresh
    /// spawn there is nothing eager to roll back).
    pub(in crate::interactor) async fn resume_adapter_agent(
        &mut self,
        factory: &Arc<dyn AgentAdapterFactory>,
        session: &Session,
    ) -> Result<()> {
        let provider_session_id = session.provider_session_id.clone().ok_or_else(|| {
            Error::Agent(format!(
                "{:?} session `{}` has no persisted provider id to resume",
                session.provider, session.id
            ))
        })?;
        let main_thread_id = self.store.main_thread_id(self.id).await?;
        // The store's current message count is the next `seq` to mint: an
        // adapter-backed session lands every message on its main thread with
        // seqs minted densely from 0, so `message_count == MAX(seq) + 1`.
        // Continuing from here is what keeps resumed history from being
        // renumbered or duplicated.
        let seed_seq = self.store.message_count(self.id).await? as i64;
        self.bind_adapter_agent(
            factory,
            AdapterBind::Resume {
                provider_session_id,
            },
            session.cwd.clone(),
            main_thread_id,
            seed_seq,
        )
        .await?;
        Ok(())
    }

    /// Roll back the eagerly-inserted adapter-backed session row after a
    /// connect/launch failure. Best-effort: a delete failure is logged, not
    /// surfaced, so the original launch error is what the caller sees.
    async fn rollback_adapter_spawn(&mut self) {
        if let Err(err) = self.store.delete_session(self.id).await {
            tracing::error!(
                session_id = %self.id,
                error = %err,
                "failed to roll back the eager adapter-backed session row after a launch failure"
            );
        }
    }
}
