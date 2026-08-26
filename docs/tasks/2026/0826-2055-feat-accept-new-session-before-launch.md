---
status: completed
pipeline_phase: null
plan: null
base_ref: feat/instant-session-focus
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -q "LaunchFinished" backend/crates/domain/delta-usecase/src/interactor/session_actor/input.rs && grep -q "launching" backend/crates/domain/delta-usecase/src/interactor/session_actor/runtime/spawn.rs && grep -q "reason" backend/crates/gateway/delta-wire/src/session_event.rs && grep -q "reason" docs/guides/api/live-channels.md && grep -q "before the launch preparation" docs/guides/api/sends.md && test -f backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests/new_session_replies_before_the_worktree_is_built.rs && test -f backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests/failed_launch_preparation_reaps_the_row_and_reports_spawn_failed.rs'
assignee: null
branch: task/0826-2055-feat-accept-new-session-before-launch
created_at: 2026-08-26T20:55:00Z
updated_at: 2026-08-26T22:22:45Z
---

# feat(sessions): answer a new-session send before the launch preparation runs

## Overview

`POST /api/sends { new_session: true }` still blocks for as long as the launch
preparation takes. `spawn_fresh` in
`backend/crates/domain/delta-usecase/src/interactor/lifecycle/spawn_fresh.rs`
runs, inside the request, on the session's actor: workdir validation → git
`repo_root` / `origin_url` lookups → **`resolve_worktree_launch_dir`** (which
runs `git fetch origin <branch>` for a remote start point and then
`git worktree add`, a full checkout) → `ensure_dir_trusted` (a rewrite of
`~/.claude.json`) → the eager `spawning` row + first-send row →
`write_session_settings` → `tmux.create_session`. Only then does the `201`
with the real ids go out. For a worktree session on a large repository that
is seconds to tens of seconds during which the browser cannot switch to the
session (the previous task made the switch happen the moment the response
lands, so the response time is now the whole wait) and `sendInFlight`
disables Send app-wide.

The eager row is the cheap part; the expensive part is preparing the
directory and launching the process. This task splits the two: the request
**accepts** the session (validates, writes the rows, replies), and the launch
runs afterwards on the actor's behalf, reporting back through the actor's own
mailbox — the same pattern the Codex event pump already uses
(`spawn_agent_event_pump` in `interactor/agent_event.rs`: a `tokio::spawn`
holding the `WeakUnboundedSender<SessionInput>` from `SessionContext.self_sender`;
the actor loop (`session_actor/actor.rs`, `run`) owns an
`Arc<InteractorCore>` that the task can hold too).

Scope is the Claude path (`spawn_fresh`). The adapter-backed path
(`spawn_adapter_session.rs`, Codex) shares `resolve_worktree_launch_dir` but
binds synchronously inside the spawn; leave it unchanged and name it as a
follow-up (see Out of scope).

### What changes

1. **Accept phase (`spawn_fresh`, still synchronous on the actor).** Keep
   everything that is cheap and everything whose failure should stay a
   synchronous `4xx`: `resolve_existing_dir`, the worktree gate
   (`WorktreeRequiresWorkdir` / `WorktreeNotAGitRepo`), `resolve_launch_options`,
   `mint_free_token`, `repo_root` / `origin_url` / `repository_display_name`
   (local git config reads). Compute the launch directory **without creating
   it**: a `Head` / `RemoteBranch` start point is always `default_path`
   (`<worktree_base>/<slug>-<id>`, see `resolve_worktree_launch_dir`), a
   `UseRemoteBranch(name)` start point is `worktree_path_for_branch` (a `git
   worktree list`, cheap) or `default_path` when absent, a plain workdir is
   itself, and no workdir is `workdir_for(&token)`. Compute `branch_at_launch`
   the same way: `delta-<id>` for the new-branch start points (today
   `current_branch(&workdir)` reads exactly that back after the checkout),
   `name` for `UseRemoteBranch`, `current_branch(dir)` for a plain workdir,
   `None` for the scratch dir. Insert the `spawning` row and the first send
   row, apply `TurnInput::Dispatch`, exactly as today. Then, instead of the
   worktree / trust / settings / tmux steps, record a **launching** entry on
   the runtime (new `LaunchingSpawn` in `session_actor/runtime/spawn.rs`,
   beside `PendingSpawn`: token, pane, the planned workdir, the remaining
   worktree build to perform — repo root + `WorktreeSpec` when a worktree was
   requested — the seed-trust flag, the launch argv, and `accepted_at`),
   spawn the launch task, and reply with `FreshSpawn` right away.
2. **Launch task.** A `tokio::spawn` that performs, in order:
   `resolve_worktree_launch_dir` (when requested — it must produce the path
   computed in step 1; assert or log if it differs), `ensure_dir_trusted`
   (when flagged), `write_session_settings`, `tmux.create_session`; the whole
   sequence wrapped in `tokio::time::timeout` with a new `LAUNCH_PREP_DEADLINE`
   (10 min; a hung `git fetch` must not stall forever — surface it as a
   failure). It posts a new `SessionInput::LaunchFinished { token, outcome:
   Result<()> }` back to the actor through the weak sender (drop the task
   silently if the actor is gone, like the pump). It never touches the
   runtime state directly — only the actor does, on the mailbox.
3. **`LaunchFinished` on the actor.** `Ok`: move the launching entry into a
   `PendingSpawn` whose `created_at` is *now* (the launch watchdog's
   `PENDING_SPAWN_DEADLINE` starts at the launch, not at acceptance — a long
   fetch must not eat the bind deadline); log at info as today's "fresh spawn
   launched; awaiting first UserPromptSubmit to bind". `Err`: roll back the
   way today's synchronous `create_session` failure does —
   `kill_pane_best_effort` (a pane may or may not exist), `forget_turn`,
   `clean_up_failed_spawn_row` (deletes the row and, by cascade, the first
   send) — and emit `SessionEvent::SpawnFailed` through `emit_async_event`
   (the async seam is wired: `state.rs` builds the interactor `with_event_sink`
   and forwards the receiver to the broadcast). A failure that used to be a
   synchronous `4xx`/`5xx` with a message (a remote branch that does not
   exist, a `git worktree add` error, a tmux failure) now arrives as an event,
   so it must carry the message: add `reason: Option<String>` to
   `SessionEvent::SpawnFailed` (domain) and `WireSessionEvent::SpawnFailed`
   (`delta-wire/src/session_event.rs`, then `make gen`), `None` on the
   existing watchdog / `SessionEnd` paths, `Some(<error display>)` here. The
   frontend `spawnsSlice` / `PendingQueue` failed chip shows the reason under
   the existing "failed to start" text when present (keep the `data-testid`s;
   extend `spawn-failure` specs to assert a reason renders when the event
   carries one). Document the field in `docs/guides/api/live-channels.md`.
4. **Runtime predicates.** While a launching entry exists: `has_live_pane()`
   is true (cold-start idempotency must not spawn a second session);
   `is_empty()` is false (the actor stays alive); the enqueue guard from the
   previous task (`has_pending_spawn()`) must also fire — rename or extend it
   (e.g. `is_launching_or_pending()`) so a send in the accept→launch window is
   `409 session_spawning` too, and update its doc; `take_stale_pending` /
   `take_unbound_pending` do NOT see a launching entry (the watchdog and
   `SessionEnd` only act on a recorded pane); `close_session` on a launching
   session is a no-op on the pane side (it has none yet — the navigator hides
   Close for a `spawning` row already) — document that in the method doc.
   `ensure_session` (cold start, `first_prompt: None`) goes through the same
   split; `SessionLifecycle::Starting` semantics are unchanged.
5. **Docs.** `docs/guides/api/sends.md` (`201` for a `new_session` send:
   the response returns before the launch preparation — worktree checkout,
   trust seeding, the agent launch — which now runs in the background; what
   stays synchronous and still yields a `400`; a preparation failure surfaces
   as `spawn_failed` with a `reason` and removes the row), `sessions.md`
   (`POST /api/sessions` same), `live-channels.md` (`spawn_failed`: the third
   producer — a failed launch preparation — and the `reason` field).
6. **Tests** (usecase, `lifecycle/tests/`, one test per file as the siblings;
   register in `tests/mod.rs`): extend `FakeGitWorktree` with a gate on
   `create_worktree` (e.g. an optional `tokio::sync::Notify`/oneshot the fake
   awaits before recording the call) so a test can hold the worktree build
   open, and extend `FakeTmux` similarly if needed:
   - `new_session_replies_before_the_worktree_is_built.rs` — with the gate
     held, `enqueue_send(NewSession{worktree: Head})` returns the `Send` with
     real ids, the row is listed as `spawning`, `FakeGitWorktree.created` and
     `FakeTmux.created` are still empty; release the gate; the worktree is
     created at the planned path, tmux launched in it, and a pending spawn is
     recorded (a subsequent `SessionStart(startup)` binds it as today).
   - `failed_launch_preparation_reaps_the_row_and_reports_spawn_failed.rs` —
     `fail_create` set: the POST still returns `201` with ids; then the
     `SpawnFailed { reason: Some(..) }` is observed on the async sink (see how
     existing tests observe the sink, e.g. the Codex pump tests), the row is
     gone, no tmux session was created.
   - a send to the session while the worktree build is held → `SessionSpawning`.
   - the launch watchdog does not reap a session whose build is held past
     `pending_spawn_deadline`; after `LaunchFinished(Ok)` the deadline counts
     from the launch (drive `reap_stale_spawns` with injected `now`s).
   - existing `new_session_with_worktree_*` / `*_seeds_trust*` /
     `*_launches_there` tests keep their assertions but must now await the
     launch (the fake gate defaults to open, so most only need to yield until
     `FakeTmux.created` is populated — add a small `await_launch` helper in
     `interactor/testing` rather than sleeping).
   Frontend: `spawnsSlice`/`PendingQueue` tests for the reason, mock
   `applyEvent` carries `reason` through. e2e-fake: no new spec (git
   preparation cannot be slowed from the harness); the existing
   `spawn-failure` and `slow-start` specs must stay green.

### Session-state coverage

Operation "send to a session" — the **spawning** state now spans two
sub-states, *launching* (accepted, no pane yet) and *pending* (pane launched,
awaiting the first hook); both answer `409 session_spawning`. The other
states are untouched. Operation "close a session" gains *launching*: a pane-side
no-op (documented). Operation "cold-start `POST /api/sessions`": idempotent
across a launching session.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A new-session send with a worktree returns `201` with real ids while the
      worktree build is still held open; the build, trust seed, settings and
      tmux launch complete afterwards at the planned path, and a pending spawn
      is recorded — pinned by
      `new_session_replies_before_the_worktree_is_built.rs` (gate appended).
- [x] A failed launch preparation deletes the row, launches nothing, and emits
      `spawn_failed` with a `reason` on the async seam — pinned by
      `failed_launch_preparation_reaps_the_row_and_reports_spawn_failed.rs`
      (gate appended); the wire event and `live-channels.md` carry `reason`
      (gates appended).
- [x] A send during the accept→launch window is `409 session_spawning`, the
      watchdog leaves a launching session alone, and the bind deadline counts
      from the launch — pinned by usecase tests.
- [x] `SessionInput::LaunchFinished` exists and the runtime models a
      `launching` entry (gates appended); `has_live_pane` / `is_empty` cover it
      (pinned by the cold-start idempotency test).
- [x] The failed chip renders the reason when present — pinned by a
      `PendingQueue` test and the `spawn-failure` e2e specs.
- [x] `sends.md` describes the `201` as returned before the launch preparation
      (gate appended).
- [x] `make check` is green (backend, generated bindings, frontend, both
      Playwright suites).

### Manual / on-hardware (verified by a human before merge)

- [ ] On the dogfooding machine, start a new session with a worktree from a
      remote branch of a large repository: the workspace switches to the new
      session within well under a second of pressing Send, the card reads
      `Starting` while the worktree is checked out and `claude` starts, then
      `Open`; a second new session can be started meanwhile; and a deliberately
      wrong remote branch produces the Retry / Dismiss row with the git error
      as its reason.

## Out of scope

- The adapter-backed (Codex) spawn path keeps its synchronous
  `resolve_worktree_launch_dir`; applying the same split there is a follow-up.
- Accepting a send to a spawning session as `queued` (unchanged from the
  previous task).
