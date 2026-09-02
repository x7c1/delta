//! The accept phase of an adapter-backed spawn: everything `POST /api/sends`
//! does before the launch moves to its background task.

use std::sync::Arc;
use std::time::Instant;

use delta_model::AgentProvider;

use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::{
    AdapterLaunch, LaunchTarget, LaunchingSpawn, PlannedWorktree,
};
use crate::pane_token::PaneToken;
use crate::ports::{GitWorktree, SessionStore, SpawningSession, TmuxDriver, Transcript, Workspace};
use crate::repository::{display_name, identity_key};
use crate::send_target::WorktreeSpec;

use super::super::launch_prep::spawn_launch_preparation;
use super::super::FreshSpawn;

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// **Accept** a terminal-less session for an adapter-backed provider,
    /// optionally with a first prompt, and hand its launch to a background
    /// task.
    ///
    /// # What is synchronous, and why
    ///
    /// The same division [`Self::spawn_fresh`] makes, for the same reason: this
    /// runs inside `POST /api/sends`, so it holds only the work that is cheap
    /// *and* the work whose failure the caller should see as a `4xx` —
    /// resolving the user-selected launch options and having the provider's
    /// adapter vet them, looking the provider's factory up in the registry,
    /// validating a user-selected workdir, the worktree gate
    /// (`WorktreeRequiresWorkdir` / `WorktreeNotAGitRepo`), and the local git
    /// config reads (`repo_root`, `origin_url`) that name the repository. It
    /// then *plans* the launch directory without creating it
    /// ([`InteractorCore::plan_worktree_launch_dir`]), writes the eager session
    /// row (status `spawning`) with its `main` thread and the first prompt's
    /// `send` row, records a [`LaunchingSpawn`], and returns.
    ///
    /// Everything expensive is deliberately *not* here: the worktree build (a
    /// `git fetch` plus a full checkout), standing the provider connection up
    /// (Codex: spawning `codex app-server` and running its handshake) and
    /// starting the thread (`thread/start`) all run on the task
    /// [`spawn_launch_preparation`] spawns. Because the row and the send exist
    /// before that task starts, the REST response already carries real ids: the
    /// browser can switch to the session and watch it start instead of holding
    /// the composer on the new-session screen for the whole checkout.
    ///
    /// A launch failure therefore can no longer be a synchronous error. It
    /// arrives as a [`SessionEvent::SpawnFailed`] carrying the failure's message
    /// as its `reason` — a git error, a provider that will not connect or will
    /// not start a thread, or the whole sequence outrunning its deadline — and
    /// the eager row is deleted; see [`Self::finish_launch`].
    ///
    /// A launch option the provider's adapter **refuses** is deliberately not
    /// one of those. Whether the selections render onto the provider's launch
    /// request is a property of the request alone, so it is decided here
    /// ([`AgentAdapterFactory::validate_launch_options`], which runs the very
    /// builder the launch will run, without connecting) and answered with the
    /// synchronous `4xx` the composer shows on the failed send row — rather
    /// than accepting a session only to tear it down again over a mistake that
    /// was visible before anything was created.
    ///
    /// # The first prompt
    ///
    /// It is written as a **`queued`** send row, not a `dispatched` one:
    /// nothing has received it yet. Unlike Claude — where the prompt rides on
    /// the launch argv, so the launch itself is the delivery — an adapter-backed
    /// prompt is a `turn/start` that can only happen once the thread exists. The
    /// bind step promotes and dispatches it (see
    /// [`Self::dispatch_first_agent_prompt`]).
    ///
    /// `launch_option_ids` are the registered launch options the user selected
    /// for this session, in selection order. They are resolved here to their
    /// neutral `(name, value?)` records and handed to the adapter on the launch
    /// request; the adapter renders them for its provider (Codex maps them onto
    /// `thread/start` fields). A selected id no longer in the registry is
    /// skipped with a warning rather than aborting the launch, exactly as on
    /// the Claude path.
    ///
    /// [`AgentAdapterFactory::validate_launch_options`]: crate::agent::AgentAdapterFactory::validate_launch_options
    /// [`InteractorCore::plan_worktree_launch_dir`]: crate::interactor::InteractorCore::plan_worktree_launch_dir
    /// [`SessionEvent::SpawnFailed`]: crate::ports::SessionEvent::SpawnFailed
    /// [`Self::dispatch_first_agent_prompt`]: super::activate_adapter_session
    /// [`spawn_launch_preparation`]: super::super::launch_prep::spawn_launch_preparation
    pub(in crate::interactor) async fn spawn_adapter_session(
        &mut self,
        provider: AgentProvider,
        first_prompt: Option<String>,
        workdir: Option<String>,
        launch_option_ids: Vec<i64>,
        worktree: Option<WorktreeSpec>,
        pull_request_number: Option<i64>,
    ) -> Result<FreshSpawn> {
        let session_id = self.id.clone();

        // Resolve the user-selected launch options up front, before anything is
        // created — the same side-effect-free gate the Claude path runs (see
        // [`Self::resolve_launch_options`]). They travel to the adapter as
        // neutral `(name, value?)` pairs and are rendered there (Codex maps
        // them onto `thread/start` fields), so this layer never learns a
        // provider's launch wire shape.
        let launch_options = self.resolve_launch_options(&launch_option_ids).await?;

        // Resolve the registered factory now, while a caller is still listening:
        // absent means the provider was never wired into this interactor, which
        // is a configuration error the request should report rather than a
        // launch that fails in the background. The launch task looks it up again
        // by the same key when it actually connects. It is also what answers
        // whether the selected launch options are ones this provider can be
        // launched with, below.
        let Some(factory) = self.adapter_backed_factory(provider) else {
            return Err(Error::Agent(format!(
                "no {provider:?} adapter factory is wired into the interactor"
            )));
        };

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
        // click takes. `launch_repo_root` / `repository_display_name` feed the
        // navigator's repo line (a worktree is always a git working tree), and
        // `branch_at_launch` records the branch the conversation started on —
        // the same columns `spawn_fresh` fills, and like there it comes from the
        // *planned* worktree rather than a `current_branch` read, because the
        // directory does not exist yet (the bind re-observes the built
        // worktree's real branch for the per-message stamp). Without a worktree
        // the cwd is the user-selected dir (its git snapshot columns stay NULL,
        // as before) or a per-session scratch dir under the spawn base, keyed by
        // the session id (there is no pane token to key it by).
        let (launch_repo_root, repository_display_name, branch_at_launch, cwd, planned_worktree) =
            match worktree {
                Some(spec) => {
                    let Some(dir) = requested_workdir.as_deref() else {
                        return Err(Error::WorktreeRequiresWorkdir);
                    };
                    let repo_root = match self.git_worktree.repo_root(dir).await? {
                        Some(root) => root,
                        None => return Err(Error::WorktreeNotAGitRepo(dir.to_owned())),
                    };
                    // Snapshot the repo's short identity (from `origin`, falling
                    // back to the working-tree basename) — it names the worktree
                    // directory and feeds the navigator card, mirroring
                    // `spawn_fresh`.
                    let origin = self.git_worktree.origin_url(&repo_root).await?;
                    let identity = identity_key(origin, &repo_root);
                    let display = display_name(&identity, &repo_root);
                    let planned = self
                        .plan_worktree_launch_dir(&session_id, &repo_root, Some(&display), &spec)
                        .await?;
                    let planned_worktree = PlannedWorktree {
                        repo_root: repo_root.clone(),
                        repository_display_name: Some(display.clone()),
                        spec,
                        branch: planned.branch.clone(),
                    };
                    (
                        Some(repo_root),
                        Some(display),
                        Some(planned.branch),
                        planned.path,
                        Some(planned_worktree),
                    )
                }
                None => {
                    let cwd = match &requested_workdir {
                        Some(dir) => dir.clone(),
                        None => std::path::Path::new(&self.session_workdir_base)
                            .join(session_id.as_str())
                            .to_string_lossy()
                            .into_owned(),
                    };
                    (None, None, None, cwd, None)
                }
            };

        // Ask the provider's adapter whether it can be launched with these
        // options — see [`AgentAdapterFactory::validate_launch_options`] for
        // what it can refuse and why the answer needs no connection.
        //
        // This is deliberately the last gate before the first row is written,
        // so a refusal returns a plain `Err` — the synchronous `400` on the send
        // the user just made — with nothing to roll back. Everything the launch
        // can fail at *for real* (the worktree build, `connect`, `thread/start`,
        // the deadline) stays asynchronous.
        factory.validate_launch_options(
            &cwd,
            &launch_options,
            planned_worktree
                .as_ref()
                .map(|worktree| worktree.repo_root.as_str()),
        )?;

        // Eagerly insert the `spawning` session row for this provider. The
        // provider-minted ids are unknown until `launch` returns, so they stay
        // NULL here and are filled — and the row activated — by the bind step.
        // The git snapshot columns carry the worktree spawn's repo/branch
        // identity (planned above); for a plain workdir with no worktree they
        // stay NULL, as before.
        let (_session, main_thread_id) = self
            .store
            .insert_spawning_session(SpawningSession {
                id: &session_id,
                cwd: &cwd,
                branch_at_launch: branch_at_launch.as_deref(),
                repo_root: launch_repo_root.as_deref(),
                requested_workdir: requested_workdir.as_deref(),
                repository_display_name: repository_display_name.as_deref(),
                provider,
                // The PR this session was opened from, when the composer's
                // origin was the PR tab — the same spawn-time snapshot
                // `spawn_fresh` records.
                pull_request_number,
            })
            .await?;

        // The first prompt is recorded `queued`, not `dispatched`: no provider
        // thread exists yet, so nothing has received it. It is promoted and
        // dispatched by the bind step, the first moment a `turn/start` is
        // possible.
        let first_send = match first_prompt.as_deref() {
            Some(text) => Some(
                self.store
                    .enqueue_queued_send(&session_id, main_thread_id, None, text, None)
                    .await?,
            ),
            None => None,
        };

        // Record the launch *before* the task that performs it is spawned: the
        // task reports back on this same mailbox, so the entry it settles is
        // always already there, and any send arriving meanwhile sees the
        // session as starting rather than as idle-and-closed.
        let launching = LaunchingSpawn {
            token: PaneToken::for_adapter_launch(&session_id),
            workdir: cwd,
            worktree: planned_worktree,
            target: LaunchTarget::Adapter(AdapterLaunch {
                provider,
                launch_options,
                main_thread_id,
                first_send_id: first_send.as_ref().map(|send| send.id),
            }),
            accepted_at: Instant::now(),
        };
        self.state.start_launching(launching.clone());
        spawn_launch_preparation(
            Arc::clone(self.core),
            self.self_sender.clone(),
            session_id.clone(),
            launching,
        );
        tracing::info!(
            session_id = %session_id,
            provider = provider.as_str(),
            has_first_prompt = first_send.is_some(),
            "adapter-backed session accepted (terminal-less); preparing its launch \
             in the background"
        );
        Ok(FreshSpawn {
            token: None,
            first_send,
        })
    }
}
