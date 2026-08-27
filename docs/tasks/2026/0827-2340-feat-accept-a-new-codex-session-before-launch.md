---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && test -f backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests/a_codex_session_replies_before_the_worktree_is_built.rs && test -f backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests/a_failed_codex_launch_reaps_the_row_and_reports_spawn_failed.rs && test -f backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests/send_during_a_codex_launch_is_refused.rs && ! grep -q "still builds inside the request" backend/crates/domain/delta-usecase/src/interactor/lifecycle/worktree_launch_dir.rs && test -f frontend/packages/apps/web/e2e-fake/codex-slow-start.spec.ts'
assignee: null
branch: task/0827-2340-feat-accept-a-new-codex-session-before-launch
created_at: 2026-08-27T14:46:00Z
updated_at: 2026-08-27T19:30:24Z
---

# feat(sessions): accept a new Codex session before its worktree and agent are launched

## Overview

`POST /api/sends` for a **Claude** session with no `session_id` is split into
an accept phase and a launch phase: `spawn_fresh`
(`backend/crates/domain/delta-usecase/src/interactor/lifecycle/spawn_fresh.rs`)
does the cheap validation, plans the worktree path
(`plan_worktree_launch_dir`), inserts the `spawning` row, records a
`LaunchingSpawn`, spawns `spawn_launch_preparation` (`launch_prep.rs`) and
returns 201 in tens of milliseconds; the worktree fetch/checkout, trust seed,
settings and tmux launch run on a detached task under `launch_prep_deadline`
and come back through `SessionInput::LaunchPrepared` / `LaunchFinished`, with
failures rolled back by `finish_launch::roll_back_failed_launch` (row deleted,
`SessionEvent::SpawnFailed { reason }` emitted, which the browser shows as a
failed chip with Retry / Dismiss).

The **adapter-backed** (Codex) path never got the split.
`spawn_adapter_session` (`lifecycle/spawn_adapter_session.rs`) still does
everything inside the request: `resolve_worktree_launch_dir` (a `git fetch`
plus a full checkout), `factory.connect()` (spawns `codex app-server`),
`adapter.launch` (`thread/start`), `bind_agent`, `content_source`, the event
pump, `set_provider_ids`, and the first prompt's `dispatch_agent_turn` — then
201. A Codex session started from a PR therefore keeps the composer on the
new-session screen for the whole checkout, and any failure is a synchronous
500 with no `spawn_failed` event, no Retry chip, and a row rolled back by a
separate `rollback_adapter_spawn`. The module doc of
`spawn_adapter_session.rs` (~17-23), `worktree_launch_dir.rs` (~130-138) and
`reap_stale_spawns.rs` (~34-41) all carve the asymmetry out in prose.

Apply the same split to the adapter-backed path, reusing the Claude
machinery rather than growing a parallel one.

### Accept (inside the request, all 4xx-able)

Keep in the request: `resolve_launch_options`, the factory lookup,
`resolve_existing_dir`, the worktree gate (`repo_root`, `origin_url`,
`display_name`), **the adapter's launch-option validation** (a selected option
the adapter refuses — a Delta-owned `thread/start` field, a field selected
twice, a `config` merge conflict — must stay a synchronous `400
launch_option_rejected` from the POST, never a `spawn_failed`; expose the
pure `thread_start_params` check on the port so the accept phase can run it
without connecting), then **`plan_worktree_launch_dir`** in place of the in-request
`resolve_worktree_launch_dir` + `current_branch` (the planned worktree name
gives `branch_at_launch`, as it does for Claude; the bind re-observes the real
branch as `bind_adapter_agent` already does). Insert the `spawning` row, write
the first prompt as a **`queued`** send row on the main thread
(`store.enqueue_queued_send`; it cannot be `dispatched` — nothing has
received it yet, and unlike Claude there is no argv to ride on), record a
`LaunchingSpawn`, spawn the launch task, and return
`FreshSpawn { token, first_send }`.

### Launch (detached task, under `launch_prep_deadline`)

The task performs the slow, actor-independent steps and posts the result
back: `resolve_worktree_launch_dir` with the planned-path equality check
(reuse `Error::WorktreeLandedElsewhere`), then `factory.connect()` and
`adapter.launch(...)` (`thread/start`) and `observe_launch_branch`. Everything
that mutates `SessionRuntime` or needs `self_sender` — `bind_agent`,
`content_source`, `spawn_agent_event_pump`, `set_provider_ids`, and promoting
+ dispatching the queued first prompt through `dispatch_agent_turn` exactly
as the in-request path does today — runs **on the actor**, in the handler of
the message the task posts (a `LaunchPrepared`-shaped checkpoint carrying the
connected adapter and handle, or a new `SessionInput` variant; pick whichever
keeps `launch_prep.rs`'s `spawn_launch_preparation` shell — timeout,
`launches_in_flight`, `LaunchFinished` — shared between both providers rather
than duplicated). Do not block the actor mailbox on `connect` or
`thread/start`.

On success the handler must emit **`SessionEvent::SessionRegistered`** on the
async sink — today it is only emitted from the Claude hook path
(`hooks/register_session_row.rs`), and the browser releases its tracked spawn
and re-enables the composer only on that event (`spawnsSlice.ts`
`reduceSessionRegistered`); a Codex spawn entry is currently never released
at all, which this fixes as a side effect.

On failure at any step, route through `roll_back_failed_launch` and delete
`rollback_adapter_spawn`: for an adapter-backed session skip
`kill_pane_best_effort` (there is no pane) and instead make sure a
`codex app-server` process spawned by a `connect` that succeeded before a
later step failed is shut down (check what the adapter handle / factory does
on drop and close it explicitly if nothing does). The `SpawnFailed.reason`
text is what the user reads on the chip, so keep the existing error
`Display`s.

### The launch key

`LaunchingSpawn`, `finish_launch(&PaneToken, …)` and
`SessionEvent::SpawnFailed.pane_token` are keyed by `PaneToken`. An
adapter-backed session has no pane, and `mint_free_token` probes tmux, so do
not mint one. Give `PaneToken` a documented constructor for an adapter-backed
launch derived from the session id (never handed to tmux), or make the launch
registry's key a provider-neutral newtype if that stays inside the
delta-usecase launch modules — either way the wire `spawn_failed.pane_token`
becomes optional (`None` for adapter-backed sessions; the frontend keys the
event on `session_id` only — confirm with a grep), regenerated with
`make gen`, noted in `docs/guides/compatibility.md`.

### Sends during the launch window

`enqueue_send.rs` checks `is_launching_or_pending()` (→ `409
session_spawning`) **after** the adapter-resume block near line 58, so a send
to a launching Codex session would reach `resume_adapter_agent` with a NULL
`provider_session_id` and fail as a 500. Move the spawning gate above that
block so both providers answer `409 session_spawning` during the window (the
session-state coverage below pins it).

### Deadlines and the watchdog

`launch_prep_deadline` covers the whole adapter launch (worktree + connect +
`thread/start`); the bind is the last step, so an adapter-backed session
never becomes a `PendingSpawn` and `pending_spawn_deadline` does not apply.
Update the prose in `reap_stale_spawns.rs` (~34-41) and
`worktree_launch_dir.rs` (~130-138) so neither still says the adapter-backed
path builds inside the request (gate appended on the latter), and the module
doc of `spawn_adapter_session.rs`.

### The failed chip's Retry must keep the configuration

Every Codex launch failure now lands on the pending strip's failed chip, so
its Retry has to re-send what the user originally sent. Today the tracked
spawn (`store/live/spawnsSlice.ts` `SpawnItem`) and the retry body
(`features/composer/useNewSessionSend.ts`) carry only `text`, `workdir` and
`launch_option_ids` — a retry after a Codex + PR-origin failure would start a
*Claude* session in the plain workdir. Carry `provider` and the `worktree`
spec through the tracked spawn onto the retry body (the wire
`CreateSendRequest` already has both fields), with unit tests. While a
session is still spawning, its queued first prompt reads
`queued — sends when the session starts` instead of `… when idle`.

### Docs

`docs/guides/api/sends.md` — delete the `provider: "codex"` carve-out
(~164-167) and make the accept/launch description (~126-150) provider-neutral;
`docs/guides/api/sessions.md` (~63-74); `docs/guides/api/live-channels.md`
(~77-95: `spawn_failed` now also fires for Codex, `pane_token` optional,
`session_registered` fires for Codex at bind); `docs/guides/compatibility.md`.

### Tests

Usecase tests in `lifecycle/tests/`, one per file, registered in
`tests/mod.rs`, modelled on the Claude ones named below. Add a factory
`interactor_with_git_and_codex_factory_and_event_sink` to
`interactor/testing/factory.rs` (there is `interactor_with_git_and_codex_factory`
and `interactor_with_codex_factory_and_event_sink`, not the combination), and
give `FakeAgentFactory` a gate to hold `connect` (or `launch`) open the way
`WorktreeGate` holds the worktree build, documented on the fake.

- `a_codex_session_replies_before_the_worktree_is_built.rs` (model:
  `new_session_replies_before_the_worktree_is_built.rs`) — worktree gate
  closed, POST returns with the row `spawning`, the first send `queued`, no
  adapter connected; open the gate, `await_launch()`, then the row is
  `active` with `provider_session_id` set, `SessionRegistered` was emitted,
  and the first prompt reached the adapter's `turn/start`.
- `a_failed_codex_launch_reaps_the_row_and_reports_spawn_failed.rs` (model:
  `failed_launch_preparation_reaps_the_row_and_reports_spawn_failed.rs`) —
  a failing factory (`FakeAgentFactory::failing`): POST still 201; after
  `await_launch()` the row is gone, `SpawnFailed { reason: Some(..),
  pane_token: None }` was emitted, no launching entry remains, and the fake
  adapter's log shows no dangling handle.
- `send_during_a_codex_launch_is_refused.rs` (model:
  `send_during_the_launch_window_is_refused.rs`) — with the connect gate
  closed, a second send to the session is `Error::SessionSpawning`, not
  `Error::Agent`.
- Rewrite `codex_spawn_rolls_back_when_connect_fails.rs` (it asserts a
  synchronous `Err`) and `new_session_with_codex_provider_creates_a_terminal_less_session.rs`
  (it reads `Active` right after the POST — add `await_launch()`), and
  re-check `codex_turn_completing_does_not_cancel_its_send.rs`,
  `new_session_from_a_pr_with_codex_provider_uses_a_worktree.rs`,
  `codex_worktree_session_grants_its_repo_root_to_the_adapter.rs` and the
  other Codex spawn tests for the same assumption.
- A `WorktreeLandedElsewhere` case is now reachable for Codex too; cover it
  only if it does not fit in the existing Claude test's parameterisation.
- e2e-fake: add `frontend/packages/apps/web/e2e-fake/codex-slow-start.spec.ts`
  mirroring `slow-start.spec.ts` (the Claude spec: the session screen is
  focused with a `Starting` card while the launch is held, Send is disabled,
  the draft survives to the bound state). `fake-codex`
  (`backend/crates/apps/fake-codex`) is part of this repo: if its scenarios
  cannot yet delay the handshake or `thread/start`, add a scenario knob for
  it (a `codex-slow-start` scenario next to the existing ones) rather than
  dropping the spec. Keep `permission-queue.spec.ts`,
  `permission-allow-for-session.spec.ts` and `permission-file-change.spec.ts`
  green — they start a Codex session and wait for output at the default
  timeout, so the un-delayed launch must stay prompt.

Run `make check` and fix whatever it reports.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A new Codex session's POST returns 201 with the row `spawning` before
      the worktree is built or the adapter connected, and after the launch
      the row is `active`, `session_registered` is emitted and the first
      prompt reaches `turn/start` — pinned by
      `a_codex_session_replies_before_the_worktree_is_built.rs` (gate
      appended).
- [x] A Codex launch that fails (connect, `thread/start`, worktree, or the
      deadline) deletes the row, emits `spawn_failed` with a reason and no
      `pane_token`, leaves no launching entry and no live adapter process —
      pinned by `a_failed_codex_launch_reaps_the_row_and_reports_spawn_failed.rs`
      (gate appended); `rollback_adapter_spawn` is gone.
- [x] Session-state coverage for a send to a Codex session: **spawning** →
      `409 session_spawning` (pinned by `send_during_a_codex_launch_is_refused.rs`,
      gate appended); **open + idle** and **open + mid-turn** unchanged
      (existing `codex_*` enqueue tests stay green); **closed** and
      **resuming** unchanged (the adapter-resume block is only re-ordered,
      not changed — existing resume tests stay green).
- [x] The e2e-fake spec `codex-slow-start.spec.ts` (gate appended) shows the
      session screen with a `Starting` card during a held Codex launch and
      the bound session afterwards.
- [x] Retry on a failed spawn chip re-sends the original `provider` and
      `worktree` spec (frontend unit tests), and a spawning session's queued
      first prompt is labelled as waiting for the session to start.
- [x] No backend doc or guide still says the adapter-backed spawn builds
      inside the request (gate appended on `worktree_launch_dir.rs`);
      `sends.md`, `sessions.md`, `live-channels.md` and `compatibility.md`
      describe the provider-neutral accept/launch and the optional
      `pane_token`.

## Out of scope

- Accepting a send to a spawning session as `queued` (both providers still
  answer `409 session_spawning`; a follow-up task changes that).
- Codex resume (`resume_adapter_agent`) — it stays synchronous.
- Any change to the Claude launch path beyond sharing its shell.
