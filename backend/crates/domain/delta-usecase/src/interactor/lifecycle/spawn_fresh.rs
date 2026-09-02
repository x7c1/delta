use std::sync::Arc;
use std::time::Instant;

use delta_model::Send;

use crate::agent::{AgentProvider, LaunchOptionSpec};
use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::{
    LaunchTarget, LaunchingSpawn, PaneLaunch, PlannedWorktree,
};
use crate::pane_token::PaneToken;
use crate::ports::{
    pane_for, GitWorktree, SessionStore, SpawningSession, TmuxDriver, Transcript, Workspace,
};
use crate::repository::{display_name, identity_key};
use crate::send_target::WorktreeSpec;

use super::launch_prep::spawn_launch_preparation;
use super::{SESSION_ID_FLAG, SETTINGS_FLAG};

/// The result of a fresh spawn: the launch's pane token and — when the spawn
/// carried a first prompt — the already-enqueued `send` row for it (which
/// names the eagerly-created session row and its `main` thread).
pub(in crate::interactor) struct FreshSpawn {
    /// The launch's tmux pane token. `Some` for a pane-backed spawn (Claude);
    /// `None` for a terminal-less agent spawn (Codex), which has no pane.
    pub token: Option<PaneToken>,
    /// The send row for the first prompt, written before the launch; `None`
    /// for a prompt-less plain spawn. For a Claude spawn it is `dispatched`
    /// (awaiting its echo); for a Codex spawn it is already completed at the
    /// `turn/start` acknowledgement.
    pub first_send: Option<Send>,
}

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// **Accept** this freshly-minted session, optionally with a first prompt,
    /// and hand its launch to a background task.
    ///
    /// The routing layer minted the session id (a time-ordered UUID v7) and
    /// spawned this actor for it; pinning the id up front via
    /// `claude --session-id <uuid>` means the first `UserPromptSubmit` hook
    /// reports exactly this id, so the launch's hooks route straight back to
    /// this actor — correlation by id, never by working directory.
    ///
    /// # What is synchronous, and why
    ///
    /// This method runs inside the `POST /api/sends` request, so it holds only
    /// the work that is cheap *and* the work whose failure the caller should
    /// see as a `4xx`: validating a user-selected workdir
    /// ([`Workspace::resolve_existing_dir`]), the worktree gate
    /// (`WorktreeRequiresWorkdir` / `WorktreeNotAGitRepo`), resolving the
    /// selected launch options, minting the pane token, and the local git
    /// config reads (`repo_root`, `origin_url`) that name the repository. It
    /// then computes the launch directory **without creating it** (see
    /// [`InteractorCore::plan_worktree_launch_dir`]), writes the eager session
    /// row (status `spawning`) with its `main` thread and the first prompt's
    /// `send` row, moves the turn machine to `AwaitingEcho`, records a
    /// [`LaunchingSpawn`], and returns.
    ///
    /// Everything expensive is deliberately *not* here: the worktree build (a
    /// `git fetch` plus a full checkout — seconds to tens of seconds on a large
    /// repository), the trust seed (a rewrite of `~/.claude.json`), the
    /// settings write and the tmux launch all run on the task
    /// [`spawn_launch_preparation`] spawns, which reports back on this actor's
    /// own mailbox ([`SessionInput::LaunchFinished`]). Because the row and the
    /// send exist before that task starts, the REST response already carries
    /// real ids: the browser can switch to the session and watch it start
    /// instead of waiting on a spinner for the checkout.
    ///
    /// A launch failure therefore can no longer be a synchronous error. It
    /// arrives as a [`SessionEvent::SpawnFailed`] carrying the preparation's
    /// failure message as its `reason` — a git or tmux error, a worktree that
    /// landed off the path this phase planned, or the whole sequence outrunning
    /// its deadline — and the eager row is deleted; see
    /// [`Self::finish_launch`].
    ///
    /// # Ordering
    ///
    /// The [`LaunchingSpawn`] is recorded *before* the launch task is spawned,
    /// and the task's `LaunchPrepared`/`LaunchFinished` — like the launch's own
    /// hooks — land on this same mailbox strictly after this message. So a send
    /// arriving anywhere in the accept→launch window finds
    /// [`SessionRuntime::is_launching_or_pending`] true and is handled exactly
    /// as one arriving against a pending spawn is: a plain send is accepted as
    /// a `queued` row and dispatched once the launch binds, and only a branch
    /// send is refused with `session_spawning`.
    ///
    /// The record the first `UserPromptSubmit` binds is the [`PendingSpawn`],
    /// installed by the launch task's `LaunchPrepared` checkpoint — which it
    /// awaits *before* creating the pane. That is what keeps the hook from being
    /// misread as external input, and it is an ordering rather than a race: the
    /// pane cannot exist until the pending spawn is recorded, so every hook the
    /// launch triggers queues behind that record. Timing alone would not do it —
    /// `tmux new-session` returns as soon as the pane's command has started, and
    /// a fast agent (a test double is instant) can submit its launch prompt
    /// before a record made afterwards lands. Keep the record strictly before
    /// the pane.
    ///
    /// # The first prompt
    ///
    /// When a `first_prompt` is present (a composer-initiated New), it is
    /// passed to `claude` as a trailing positional argument on the launch
    /// command line (`claude … <prompt>`) rather than typed into the pane after
    /// launch. An interactive `claude` invoked with a positional prompt
    /// auto-submits it at startup, which fires the `UserPromptSubmit` hook that
    /// binds this spawn. Submitting at launch avoids the failure mode of
    /// injecting keystrokes after a fixed settle delay: on a slow cold start the
    /// TUI input is not yet ready when the keystrokes land, they are lost, the
    /// prompt is never submitted, and the spawn sits pending forever. The
    /// command is forwarded as an argv tail (no shell), so a multi-line or
    /// quoted prompt is already safe.
    ///
    /// `launch_option_ids` are the registered launch options the user selected
    /// for this session, in selection order. They are resolved to their
    /// `(name, value?)` flag records up front (alongside the workdir gate) and
    /// pushed onto the launch argv after Delta's own flags and before the
    /// positional prompt. A selected id no longer in the registry is skipped
    /// with a warning rather than aborting the launch.
    ///
    /// [`InteractorCore::plan_worktree_launch_dir`]: crate::interactor::InteractorCore::plan_worktree_launch_dir
    /// [`PendingSpawn`]: crate::interactor::session_actor::runtime::PendingSpawn
    /// [`SessionInput::LaunchFinished`]: crate::interactor::session_actor::input::SessionInput::LaunchFinished
    /// [`SessionEvent::SpawnFailed`]: crate::ports::SessionEvent::SpawnFailed
    /// [`SessionRuntime::is_launching_or_pending`]: crate::interactor::session_actor::runtime::SessionRuntime::is_launching_or_pending
    pub(in crate::interactor) async fn spawn_fresh(
        &mut self,
        first_prompt: Option<String>,
        workdir: Option<String>,
        launch_option_ids: Vec<i64>,
        worktree: Option<WorktreeSpec>,
        pull_request_number: Option<i64>,
    ) -> Result<FreshSpawn> {
        let session_id = self.id.clone();
        // Validate a user-selected workdir before minting or launching anything,
        // so an invalid path is rejected with no side effects. The canonical
        // path becomes the launch directory; `None` defers to `<base>/<token>`
        // computed after the token is minted, below.
        let requested_workdir = match workdir {
            Some(dir) => Some(self.workspace.resolve_existing_dir(&dir).await?),
            None => None,
        };

        // Resolve an opt-in worktree request against the validated workdir,
        // before minting or launching anything so a bad request is rejected with
        // no side effects. A worktree needs a selected directory that is a git
        // repository: no workdir is `WorktreeRequiresWorkdir`, and a workdir that
        // is not a git repo is `WorktreeNotAGitRepo` — both `400`s. On success we
        // hold the repository root; the actual `git worktree add` (a real side
        // effect) is deferred to the launch task. `repo_root` runs no fetch, so
        // this gate stays lightweight.
        let worktree_repo_root = match &worktree {
            Some(_) => {
                let Some(dir) = requested_workdir.as_deref() else {
                    return Err(crate::error::Error::WorktreeRequiresWorkdir);
                };
                match self.git_worktree.repo_root(dir).await? {
                    Some(root) => Some(root),
                    None => return Err(crate::error::Error::WorktreeNotAGitRepo(dir.to_owned())),
                }
            }
            None => None,
        };

        // Resolve the user-selected launch options before minting or launching
        // anything, mirroring the workdir gate above: a resolution failure
        // leaves no side effects (see [`Self::resolve_launch_options`], which
        // both spawn paths share). Claude is an argv-launched provider, so each
        // resolved `(name, value?)` pair renders as its flag followed by its
        // argument — a valueless flag contributes only the name.
        let launch_option_args: Vec<String> = self
            .resolve_launch_options(&launch_option_ids)
            .await?
            .iter()
            .flat_map(LaunchOptionSpec::to_argv)
            .collect();

        // The minter is atomic, so token uniqueness needs no coordination here.
        let token = self.mint_free_token().await?;
        let pane = pane_for(token.as_str());

        // `seed_trust` records whether the effective launch directory is a git
        // repository whose workspace-trust dialog must be pre-accepted before
        // launching `claude` there (the launch task does the seeding). A
        // worktree is always a git working tree; a user-selected workdir is
        // checked once here; the default `<base>/<token>` scratch dir is empty
        // and never triggers the dialog, so it is never seeded (avoids bloating
        // `~/.claude.json` for ordinary sessions).
        //
        // `launch_repo_root` is the repository root containing the effective
        // launch directory at spawn time — the worktree's repo for a worktree
        // spawn, the user-selected workdir's repo for a plain spawn, `None`
        // for the default `<base>/<token>` scratch dir (always non-git). It
        // feeds the navigator's "repo name" line via `insert_spawning_session`
        // below, doubles as the trust-seeding signal (so we never call
        // `repo_root` twice on the same path), and drives the
        // `repository_display_name` derivation below — which in turn shapes
        // the worktree directory name. The user-selected workdir as it will
        // be stored in `session.requested_workdir` is captured alongside,
        // because the workdir-resolution match below consumes
        // `requested_workdir` to compute the effective launch dir.
        let requested_workdir_recorded = requested_workdir.clone();
        let (launch_repo_root, seed_trust): (Option<String>, bool) = match &worktree {
            Some(_) => {
                let root = worktree_repo_root
                    .clone()
                    .expect("worktree_repo_root is Some whenever a worktree was requested");
                // A worktree is by definition a git working tree, so its
                // trust dialog must be pre-accepted; no extra git call
                // needed. Trust seeding is idempotent, so reusing an
                // already-trusted path (e.g. the main tree) is fine.
                (Some(root), true)
            }
            None => match requested_workdir.as_deref() {
                // A user-selected workdir may be a real git repo (without a
                // worktree request). Look up `repo_root` once, both to gate
                // trust-seeding (idem) and to feed the navigator's "repo
                // name" line — `None` here is "launched outside a repo", and
                // the frontend then falls back to the cwd basename.
                Some(dir) => {
                    let root = self.git_worktree.repo_root(dir).await?;
                    let trust = root.is_some();
                    (root, trust)
                }
                // The default per-token scratch dir is empty, so `claude`
                // never shows the trust dialog there; skip the git check on
                // the hot path.
                None => (None, false),
            },
        };

        // Snapshot a short repository identity label for the navigator card's
        // repo line (line 2 left). Derived from the launch directory's
        // `origin` URL (normalised to `host/org/repo` and shortened to
        // `org/repo`), falling back to the working-tree basename when no
        // origin is configured. `None` when the launch dir is not a git repo
        // at all; the frontend then falls back to the cwd basename.
        //
        // Looked up against `launch_repo_root` rather than the effective
        // launch dir: `remote.origin.url` lives in the shared `.git/config`,
        // so the answer is the same from any path inside the same
        // repository, and reading against the (already-known) repo root lets
        // us call `origin_url` exactly once even when the launch dir does not
        // exist yet (a worktree's directory is not built until the launch
        // task runs). Unlike `repo_root`, which is the worktree path itself
        // when launched from a linked worktree, this value is stable across
        // worktrees of the same clone.
        let repository_display_name = match launch_repo_root.as_deref() {
            Some(root) => {
                let origin = self.git_worktree.origin_url(root).await?;
                let key = identity_key(origin, root);
                Some(display_name(&key, root))
            }
            None => None,
        };

        // Determine the effective launch directory and the branch the session
        // will be on there — both **without doing any git work on the working
        // tree**, so the eager row below can record them while the build is
        // still ahead of us.
        //
        // With no worktree request the launch dir is the validated workdir (or
        // the default `<base>/<token>`), and its branch is read straight off
        // the directory (`None` for the scratch dir, which is never a repo).
        // With a worktree request the session launches in a per-session
        // worktree under the neutral `worktree_base` — outside any repo tree,
        // so the worktree does not inherit a surrounding `CLAUDE.md`/settings —
        // whose path and branch the planner derives; see
        // [`InteractorCore::plan_worktree_launch_dir`] for the start-point
        // rules and the directory-name slug.
        //
        // `branch_at_launch` is the navigator card's line-1 identifier — the
        // branch the conversation was started on, never mutated on resume or a
        // later `git checkout` inside the worktree (the per-message `git_branch`
        // on `Message` carries the per-turn snapshot separately).
        let (workdir, branch_at_launch, planned_worktree) = match &worktree {
            Some(spec) => {
                let repo_root = worktree_repo_root
                    .expect("worktree_repo_root is Some whenever a worktree was requested");
                let planned = self
                    .plan_worktree_launch_dir(
                        &session_id,
                        &repo_root,
                        repository_display_name.as_deref(),
                        spec,
                    )
                    .await?;
                (
                    planned.path,
                    Some(planned.branch.clone()),
                    Some(PlannedWorktree {
                        repo_root,
                        repository_display_name: repository_display_name.clone(),
                        spec: spec.clone(),
                        branch: planned.branch,
                    }),
                )
            }
            None => match requested_workdir {
                Some(dir) => {
                    let branch = self.git_worktree.current_branch(&dir).await?;
                    (dir, branch, None)
                }
                // The default per-token scratch dir is created empty and is
                // never a git repository, so there is no branch to read and no
                // trust dialog to seed; skip both on the hot path.
                None => (self.workdir_for(&token), None, None),
            },
        };

        // Eagerly create the session row and its `main` thread, then the first
        // prompt's send row bound to those real ids. Hooks cannot arrive before
        // the launch (and would queue behind this message anyway), so nothing
        // races this write; if the launch fails the row is deleted again in the
        // rollback (see [`Self::finish_launch`]). NOTE: the worktree the launch
        // task builds is deliberately NOT removed on a later close — see
        // `close_session` for the no-cleanup-on-close MVP decision; `session.cwd`
        // stored here is the worktree path, so a resume reattaches to the
        // existing worktree.
        // Record the dir the user picked, before any worktree resolution. For
        // a worktree spawn `cwd` (= `workdir` above) holds the auto-generated
        // worktree path under `$DELTA_WORKTREE_BASE`;
        // `requested_workdir_recorded` holds the canonical user-selected dir
        // (which is also the worktree's `repo_root`). For a plain spawn with a
        // user-selected workdir this equals `cwd`. `None` only for the default
        // per-token scratch dir, so a scratch session contributes nothing to
        // Recent dirs.
        let (_session, main_thread_id) = self
            .store
            .insert_spawning_session(SpawningSession {
                id: &session_id,
                cwd: &workdir,
                branch_at_launch: branch_at_launch.as_deref(),
                repo_root: launch_repo_root.as_deref(),
                requested_workdir: requested_workdir_recorded.as_deref(),
                repository_display_name: repository_display_name.as_deref(),
                // The fresh-spawn path drives Claude Code (tmux PTY + hooks).
                provider: AgentProvider::Claude,
                // The PR this session was opened from, when the composer's
                // origin was the PR tab. A spawn-time snapshot like the git
                // columns above: written here, never updated on resume.
                pull_request_number,
            })
            .await?;
        let first_send = match first_prompt.as_deref() {
            Some(text) => Some(
                self.store
                    .enqueue_send(&session_id, main_thread_id, None, text, None)
                    .await?,
            ),
            None => None,
        };
        // The first prompt is delivered on the launch command line below, so it
        // is already "dispatched": move the turn machine to `AwaitingEcho` now,
        // before the launch, so the first `UserPromptSubmit` the auto-submitted
        // prompt fires always finds the dispatch recorded.
        if let Some(send) = &first_send {
            self.apply_turn_input(crate::turn::TurnInput::Dispatch { send_id: send.id })
                .await?;
        }

        let mut command = vec![
            self.launch.claude_bin.clone(),
            SETTINGS_FLAG.to_owned(),
            self.session_settings_path.clone(),
            SESSION_ID_FLAG.to_owned(),
            session_id.as_str().to_owned(),
        ];
        // Apply the user-selected launch options after the Delta-owned
        // `--settings`/`--session-id` flags and before the trailing positional
        // prompt, so the prompt stays the last argument `claude` auto-submits.
        command.extend(launch_option_args);
        // Carry the first prompt on the launch command line as a trailing
        // positional argument. `claude` auto-submits a positional prompt at
        // startup, so the prompt is delivered without any post-launch keystroke
        // injection (which is lost when the TUI input is not yet ready on a slow
        // cold start). The argv tail is forwarded without a shell, so a
        // multi-line or quoted prompt is safe.
        if let Some(text) = first_prompt {
            command.push(text);
        }

        // Record the launch *before* the task that performs it is spawned: the
        // task reports back on this same mailbox, so the entry it settles is
        // always already there, and any send arriving meanwhile sees the
        // session as starting rather than as idle-and-closed.
        let launching = LaunchingSpawn {
            token: token.clone(),
            workdir,
            worktree: planned_worktree,
            target: LaunchTarget::Pane(PaneLaunch {
                pane,
                seed_trust,
                command,
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
            token = %token.as_str(),
            session_id = %session_id,
            has_first_prompt = first_send.is_some(),
            "fresh session accepted; preparing its launch in the background"
        );
        Ok(FreshSpawn {
            token: Some(token),
            first_send,
        })
    }
}
