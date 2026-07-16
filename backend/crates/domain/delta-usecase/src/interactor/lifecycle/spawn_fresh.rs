use delta_model::Send;

use crate::agent::AgentProvider;
use crate::error::Result;
use crate::interactor::launch_options::expand_leading_tilde;
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::PendingSpawn;
use crate::pane_token::PaneToken;
use crate::ports::{
    pane_for, GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace, WorktreeStartPoint,
};
use crate::repository::{display_name, identity_key, worktree_dir_slug};
use crate::send_target::WorktreeSpec;

use super::{SESSION_ID_FLAG, SETTINGS_FLAG};

/// The result of a fresh spawn: the launch's pane token and — when the spawn
/// carried a first prompt — the already-enqueued `send` row for it (which
/// names the eagerly-created session row and its `main` thread).
pub(in crate::interactor) struct FreshSpawn {
    pub token: PaneToken,
    /// The `dispatched` send row for the first prompt, written before the
    /// launch; `None` for a prompt-less plain spawn.
    pub first_send: Option<Send>,
}

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Spawn this freshly-minted session's pane, optionally dispatching a
    /// first prompt.
    ///
    /// The routing layer minted the session id (a time-ordered UUID v7) and
    /// spawned this actor for it; pinning the id up front via
    /// `claude --session-id <uuid>` means the first `UserPromptSubmit` hook
    /// reports exactly this id, so the launch's hooks route straight back to
    /// this actor — correlation by id, never by working directory.
    ///
    /// This **eagerly inserts the session row** (status `spawning`, transcript
    /// path unknown until the first hook) with its `main` thread, enqueues the
    /// first prompt's `send` row when one is given, and only then launches
    /// `claude --settings <path> --session-id <uuid>` in the launch directory.
    /// Because the row and the send exist before the launch, the REST response
    /// for a composer-initiated New carries real ids instead of placeholders,
    /// and the first `UserPromptSubmit` correlates through the normal
    /// single-outstanding machinery with no bind-time row writing.
    ///
    /// A [`PendingSpawn`] is recorded on this actor's runtime state: the first
    /// hook *activates* the eager row (`spawning` → `active`, filling the
    /// transcript path) via [`SessionRuntime::bind_pending_spawn`].
    ///
    /// When a `first_prompt` is present (a composer-initiated New), it is passed
    /// to `claude` as a trailing positional argument on the launch command line
    /// (`claude … <prompt>`) rather than typed into the pane after launch. An
    /// interactive `claude` invoked with a positional prompt auto-submits it at
    /// startup, which fires the `UserPromptSubmit` hook that binds this spawn.
    /// Submitting at launch avoids the failure mode of injecting keystrokes
    /// after a fixed settle delay: on a slow cold start the TUI input is not yet
    /// ready when the keystrokes land, they are lost, the prompt is never
    /// submitted, and the spawn sits pending forever. The command is forwarded
    /// as an argv tail (no shell), so a multi-line or quoted prompt is already
    /// safe.
    ///
    /// The `PendingSpawn` is recorded *before* `create_session` launches
    /// `claude`, so the `UserPromptSubmit` (or `SessionStart`) that the launch
    /// triggers always finds a spawn to bind rather than racing ahead and being
    /// misread as external input — those hooks land on this same mailbox,
    /// strictly after this message. A failed `create_session` rolls back both
    /// the just-recorded pending *and* the eager session row (the cascade
    /// removes its send), so no dangling spawn or orphan row is left behind.
    ///
    /// When `workdir` is `Some`, it is a user-selected path: it is validated and
    /// canonicalized via [`Workspace::resolve_existing_dir`] *before* anything is
    /// minted or launched, so an invalid path fails cleanly with no token, no
    /// pane, and no pending spawn left behind (mirroring the resume gate in
    /// [`Self::open_session`]). When `None`, the spawn falls back to its default
    /// per-token `<base>/<token>` directory.
    ///
    /// `launch_option_ids` are the registered launch options the user selected
    /// for this session, in selection order. They are resolved to their
    /// `(name, value?)` flag records up front (alongside the workdir gate) and
    /// pushed onto the launch argv after Delta's own flags and before the
    /// positional prompt. A selected id no longer in the registry is skipped
    /// with a warning rather than aborting the launch.
    ///
    /// [`SessionRuntime::bind_pending_spawn`]: crate::interactor::session_actor::runtime::SessionRuntime::bind_pending_spawn
    pub(in crate::interactor) async fn spawn_fresh(
        &mut self,
        first_prompt: Option<String>,
        workdir: Option<String>,
        launch_option_ids: Vec<i64>,
        worktree: Option<WorktreeSpec>,
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
        // effect) is deferred to just before the eager row write below, so a git
        // failure also leaves no orphan. `repo_root` runs no fetch, so this gate
        // stays lightweight.
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

        // Resolve the user-selected launch options to argv flags before minting
        // or launching anything, mirroring the workdir gate above: a resolution
        // failure leaves no side effects. The registry is small, so a single
        // fetch plus a by-id lookup (preserving the user's selection order) is
        // cheap. A selected id that is no longer registered (a concurrent
        // delete after the picker rendered) is skipped with a warning rather
        // than failing the launch, so a stale UI selection cannot kill a spawn.
        // Each option contributes its `name` and, when present, its `value`; a
        // valueless flag contributes only the name.
        let launch_option_args = if launch_option_ids.is_empty() {
            Vec::new()
        } else {
            let by_id = self
                .store
                .list_launch_options()
                .await?
                .into_iter()
                .map(|option| (option.id, option))
                .collect::<std::collections::HashMap<_, _>>();
            // Read HOME once for tilde expansion (see the push below).
            let home = std::env::var("HOME").ok().filter(|h| !h.is_empty());
            let mut args = Vec::new();
            for id in &launch_option_ids {
                match by_id.get(id) {
                    Some(option) => {
                        args.push(option.name.clone());
                        if let Some(value) = &option.value {
                            // Expand a leading `~` ourselves: this command line
                            // is forwarded to `claude` as an argv tail with no
                            // shell, so the shell's tilde expansion never runs.
                            // Left as-is, a `~/...` value would reach `claude`
                            // literally and be resolved against the (worktree)
                            // cwd, yielding a bogus `<cwd>/~/...` path.
                            args.push(expand_leading_tilde(value, home.as_deref()));
                        }
                    }
                    None => tracing::warn!(
                        launch_option_id = id,
                        session_id = %session_id,
                        "selected launch option is no longer registered; skipping it"
                    ),
                }
            }
            args
        };

        // The minter is atomic, so token uniqueness needs no coordination here.
        let token = self.mint_free_token().await?;
        let pane = pane_for(token.as_str());

        // Determine the effective launch directory. With no worktree request,
        // it is the validated workdir (or the default `<base>/<token>`). With a
        // worktree request, launch in a per-session worktree under the neutral
        // `worktree_base` (outside any repo tree, so the worktree does not
        // inherit a surrounding `CLAUDE.md`/settings) instead.
        //
        // For the new-branch start points (`Head`/`RemoteBranch`) the worktree
        // gets its own `delta-<session-id>` branch (the Delta-minted
        // conversation id, not the pane token): the session id is the stable,
        // human-meaningful name a user can later find and clean up, and the
        // frontend's `displayBranch()` shortens this shape on the navigator
        // card. The worktree **directory** name embeds the repository
        // identity (`<org>-<repo>-<session-id>`) so listing
        // `$DELTA_WORKTREE_BASE` shows which clone each worktree belongs to
        // at a glance — see the slug derivation in the match below.
        //
        // For `UseRemoteBranch` the user works on the named branch *itself*:
        // since git forbids checking one branch out in two worktrees, the
        // worktree already holding that branch is reused when one exists
        // (including the main working tree), and otherwise a new worktree at
        // `<base>/<org>-<repo>-<session-id>` that checks the branch out is
        // created.
        //
        // The git work happens here — after workdir validation but *before* the
        // eager session row — so a git failure leaves no orphan row to roll
        // back; only the (side-effect-free) token has been minted. The reuse
        // case has no new worktree at all, so there is nothing to roll back for
        // it either.
        //
        // `seed_trust` records whether the effective launch directory is a git
        // repository whose workspace-trust dialog must be pre-accepted before
        // launching `claude` there (see the seed step below). A worktree is
        // always a git working tree; a user-selected workdir is checked once
        // here; the default `<base>/<token>` scratch dir is empty and never
        // triggers the dialog, so it is never seeded (avoids bloating
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
        // us call `origin_url` exactly once even when the launch dir is
        // resolved later (a worktree's path is not built until below).
        // Unlike `repo_root`, which is the worktree path itself when
        // launched from a linked worktree, this value is stable across
        // worktrees of the same clone.
        let repository_display_name = match launch_repo_root.as_deref() {
            Some(root) => {
                let origin = self.git_worktree.origin_url(root).await?;
                let key = identity_key(origin, root);
                Some(display_name(&key, root))
            }
            None => None,
        };

        let workdir = match worktree {
            Some(spec) => {
                let repo_root = worktree_repo_root
                    .expect("worktree_repo_root is Some whenever a worktree was requested");
                // Build the per-session worktree directory name from the
                // repository identity so a listing of `$DELTA_WORKTREE_BASE`
                // makes each worktree distinguishable at a glance (instead of
                // a wall of UUID-suffixed `delta-<id>` entries). The slug is
                // the display name with `/` rewritten to `-` and any unsafe
                // character replaced — see [`worktree_dir_slug`]. When no
                // display name is available (the path is somehow non-git, or
                // slugifies to an empty string) we fall back to the literal
                // `delta` so the path is never just `<base>/-<id>`. The
                // **branch** name created for new-branch start points stays
                // `delta-<session-id>` so the frontend's `displayBranch()`
                // shortening continues to recognise it.
                let slug = repository_display_name
                    .as_deref()
                    .map(worktree_dir_slug)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "delta".to_owned());
                let default_path =
                    format!("{}/{}-{}", self.worktree_base, slug, session_id.as_str());
                let effective_path = match spec.start_point {
                    // New-branch start points: cut `delta-<id>` at `default_path`.
                    start_point @ (WorktreeStartPoint::Head
                    | WorktreeStartPoint::RemoteBranch(_)) => {
                        let branch = format!("delta-{}", session_id.as_str());
                        self.git_worktree
                            .create_worktree(&repo_root, &default_path, &branch, start_point)
                            .await?;
                        default_path
                    }
                    // Use the branch itself: reuse the worktree already holding
                    // it (incl. the main tree) when one exists, else create one
                    // that checks it out at `default_path`.
                    WorktreeStartPoint::UseRemoteBranch(name) => {
                        match self
                            .git_worktree
                            .worktree_path_for_branch(&repo_root, &name)
                            .await?
                        {
                            Some(existing) => existing,
                            None => {
                                self.git_worktree
                                    .add_worktree_checkout(&repo_root, &default_path, &name)
                                    .await?;
                                default_path
                            }
                        }
                    }
                };
                effective_path
            }
            None => match requested_workdir {
                Some(dir) => dir,
                // The default per-token scratch dir is empty, so `claude` never
                // shows the trust dialog there; skip the git check on the hot path.
                None => self.workdir_for(&token),
            },
        };

        // Snapshot the launch-time local branch. This is the navigator card's
        // line-1 identifier — the branch the conversation was started on, never
        // mutated on resume or a later `git checkout` inside the worktree (the
        // per-message `git_branch` on `Message` carries the per-turn snapshot
        // separately). `None` when the launch dir is not a git repo or HEAD is
        // detached; the frontend then falls back to the session label.
        let branch_at_launch = self.git_worktree.current_branch(&workdir).await?;

        // Pre-accept Claude Code's workspace-trust dialog for git-repo launch
        // directories. A fresh directory containing files otherwise pops a
        // blocking interactive dialog at startup, which means the first
        // `UserPromptSubmit` hook never fires and the spawn is reaped after the
        // pending deadline. Seed *before* `create_session` so a failure fails the
        // spawn cleanly with no half-launched pane (mirroring the workdir/worktree
        // validation ordering above, all of which run before any tmux side effect).
        if seed_trust {
            self.git_worktree.ensure_dir_trusted(&workdir).await?;
        }

        // Eagerly create the session row and its `main` thread, then the first
        // prompt's send row bound to those real ids. Hooks cannot arrive before
        // the launch below (and would queue behind this message anyway), so
        // nothing races this write; if the launch fails the row is deleted
        // again in the rollback. NOTE: the worktree (created above) is
        // deliberately NOT removed on a later close — see `close_session` for
        // the no-cleanup-on-close MVP decision; `session.cwd` stored here is the
        // worktree path, so a resume reattaches to the existing worktree.
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
            .insert_spawning_session(
                &session_id,
                &workdir,
                branch_at_launch.as_deref(),
                launch_repo_root.as_deref(),
                requested_workdir_recorded.as_deref(),
                repository_display_name.as_deref(),
                // The fresh-spawn path drives Claude Code (tmux PTY + hooks).
                AgentProvider::Claude,
            )
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

        self.workspace
            .write_session_settings(&self.session_settings_path, &self.session_settings_json)
            .await?;
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
        if let Some(text) = first_prompt.clone() {
            command.push(text);
        }

        // Record the spawn *before* launching `claude`, so the `UserPromptSubmit`
        // that the launch-submitted prompt triggers finds a pending spawn to bind
        // instead of racing ahead and being misread as external input. With the
        // prompt on the command line the hook fires very soon after launch, so
        // this ordering — not any delay inside `create_session` — is what makes
        // the spawn record reliably present when the hook arrives.
        self.state.push_pending(PendingSpawn {
            token: token.clone(),
            pane: pane.clone(),
            // Stamp the spawn for the watchdog deadline. From here the only thing
            // that binds it is the first `UserPromptSubmit` hook; if that never
            // arrives, the reaper uses this instant to reap the stuck spawn.
            created_at: std::time::Instant::now(),
        });

        // Launch the session. If `create_session` fails, the spawn never starts,
        // so roll back the just-recorded pending (otherwise a later, unrelated
        // hook could mis-bind to this abandoned pane) and the eager session row
        // (the cascade removes its main thread and first send), then surface
        // the error. The REST caller gets the failure synchronously, so no
        // `SpawnFailed` event is needed for this path.
        if let Err(spawn_err) = self
            .tmux
            .create_session(token.as_str(), &workdir, &command)
            .await
        {
            tracing::error!(
                token = %token.as_str(),
                session_id = %session_id,
                error = %spawn_err,
                "fresh spawn failed to launch; rolling back the pending spawn and \
                 the eager session row"
            );
            self.state.remove_pending_for_token(&token);
            // The session row (and its first send, by cascade) is deleted, so
            // the turn entry is dropped without orphan handling.
            self.state.forget_turn();
            self.store.delete_session(&session_id).await?;
            return Err(spawn_err);
        }
        tracing::info!(
            token = %token.as_str(),
            session_id = %session_id,
            workdir = %workdir,
            has_first_prompt = first_send.is_some(),
            "fresh spawn launched; awaiting first UserPromptSubmit to bind"
        );
        Ok(FreshSpawn { token, first_send })
    }
}
