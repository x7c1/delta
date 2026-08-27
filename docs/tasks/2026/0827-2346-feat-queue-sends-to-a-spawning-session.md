---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && test -f backend/crates/domain/delta-usecase/src/interactor/enqueue/tests/send_to_a_still_spawning_session_is_queued.rs && test -f backend/crates/domain/delta-usecase/src/interactor/enqueue/tests/queued_sends_flush_once_the_spawn_binds.rs && test -f backend/crates/domain/delta-usecase/src/interactor/enqueue/tests/a_failed_launch_reports_its_unsent_queued_text.rs && grep -q "unsent" backend/crates/gateway/delta-wire/src/session_event.rs && grep -q "session_spawning" backend/crates/apps/delta-server/src/api/api_error.rs'
assignee: null
branch: task/0827-2346-feat-queue-sends-to-a-spawning-session
created_at: 2026-08-27T14:46:00Z
updated_at: 2026-08-27T22:03:16Z
---

# feat(sends): queue a send to a still-spawning session and hand its text back if the launch fails

## Overview

A session accepted by `POST /api/sends` is `spawning` until its launch binds
(`is_launching_or_pending()` in `session_actor/runtime/spawn.rs`: a
`LaunchingSpawn` or `PendingSpawn` is registered). During that window a
second send is refused with `409 session_spawning`
(`interactor/enqueue/enqueue_send.rs`, the check in front of `ensure_open()`
— it exists so a spawning session never takes the `claude --resume` path and
launches a second agent). The browser never even sends it: the composer
disables Send while the focused session is spawning
(`features/composer/Composer.tsx`, `submitDisabled` includes `spawning`) with
the placeholder "This session is starting…". With the accept/launch split a
PR-origin session can spend a minute checking out, and the user who already
knows their next message has to wait at a disabled button.

Accept the send as a **`queued`** row instead, flush it once the session
binds, and — because a failed launch deletes the session row and the `send`
rows cascade with it (`send.session_id … ON DELETE CASCADE`) — put the text
of every queued send that never reached the agent on the `spawn_failed`
event so the browser can put it back in the composer. **No automatic
re-send**: the Retry chip keeps re-sending the first prompt as today, and the
restored text waits in the composer for the user. (This mirrors the decision
already taken for restored sends: an automatic re-send after a failure was
tried and reverted in favour of an explicit user action.) This task assumes
the adapter-backed (Codex) spawn has already been split into accept and
launch, so "spawning" means the same thing for both providers.

### Backend: accept as `queued`

In `enqueue_send.rs`, when `is_launching_or_pending()`:

- a **plain** send (no `branch_from`) is written with
  `store.enqueue_queued_send(...)` on the main thread and returned as the 201
  body with `status: "queued"` — no event (there is no `send_queued` kind;
  the body plus `GET /api/sessions/{id}/sends` is how a queued row reaches
  clients today), and this branch must be explicit and must not fall through
  to `ensure_open()`;
- a **branch** send (`branch_from` set) stays `409 session_spawning` — there
  is no message to branch from yet. Keep `Error::SessionSpawning` and
  `SESSION_SPAWNING_CODE` for it (gate appended) and narrow their docs.

### Backend: flush after bind

- **Claude**: the bind runs inside a blocking hook
  (`hooks/bind_pending_spawn.rs` → `register_session_row`), and keystrokes
  must not be typed from inside a hook (`hooks/on_session_start.rs` ~73-85
  explains why). With a first prompt the turn is already `AwaitingEcho` at
  bind, so `dispatch_queued_send` is a no-op there and `hooks/on_stop.rs`
  flushes the queue at the first turn end — nothing to add. For the
  prompt-less spawn (`ensure_session` / `new_session()` in `routing.rs`; the
  turn is `Idle` at bind and no `Stop` is coming) post a flush to the actor
  itself through `self_sender` from the bind handler so the dispatch runs on
  the next mailbox iteration, after the hook has returned — do not call
  `dispatch_queued_send` inline in the hook.
- **Codex**: the bind handler already promotes and dispatches the queued
  first prompt; any further queued rows follow the existing turn-end flush
  for adapter-backed sessions. Confirm with a test rather than assuming.

### Backend: the failed-launch handoff

In `finish_launch::roll_back_failed_launch` (and the other two producers of
the same rollback: the watchdog in `reap_stale_spawns.rs` and
`hooks/on_session_end.rs`), read the session's `queued` send rows in id
order **before** `clean_up_failed_spawn_row` deletes them, and carry them on
`SessionEvent::SpawnFailed` as `unsent: Vec<UnsentSend { send_id, text }>`
(precedent for text on an event: `SendParked { text }`). Include every row
that never reached the agent, the first prompt included — the client decides
what it already holds. Wire twin in `delta-wire/src/session_event.rs` (gate
appended), `make gen`, `docs/guides/api/live-channels.md`,
`docs/guides/compatibility.md`. Update the stale claim in
`reap_stale_spawns.rs` (~126-129) that the browser holds all the text.

### Frontend

- Enable Send while the focused session is spawning: drop `spawning` from
  `submitDisabled`, change the placeholder to say the message will be sent
  when the session is ready, and update the doc comments at
  `Composer.tsx` ~38-44 and ~286-296. The pending strip already shows a
  spawning session's sends on the new-session surface
  (`usePendingSends.ts`); the row renders `queued — sends when idle`, which
  is accurate enough — adjust the wording only if it misleads.
- On `spawn_failed`: in `store/live/spawnsSlice.ts` `reduceSpawnFailed` (and
  the buffered branch of `trackSpawn`) and `data/applySessionEvent.ts`, take
  `event.unsent`, drop the entry whose `send_id` is the spawn's own first
  send (the Retry chip holds that text — record the first send id on the
  `SpawnItem` at `trackSpawn` time if it is not there yet), and **append**
  the remaining texts, oldest first, separated by a blank line, to the
  new-session composer draft (`useComposerStore` `setDraft(NEW_SESSION_DRAFT_KEY, …)`)
  after whatever the user has typed there — never clobber a draft — before
  `reconcileFocusedSession(NEW_SESSION_FOCUS)` runs so the composer mounts
  with the text in place. Drop the `session_spawning` special case from the
  MSW mock (`testing/api-mocks/src/handlers.ts` ~578-592, `fixtures.ts`
  ~582) for plain sends; keep it for branch sends.
- `gateway/api-client/src/http.ts` keeps `session_spawning` in the code
  union (branch sends).
- The spawn registry is in-memory, so a `spawn_failed` that arrives after a
  reload has no tracked spawn to attach to. It must still return the text:
  append **every** `unsent` text (first prompt included) to the new-session
  draft and raise a notice with the failure reason, so nothing is lost even
  when no chip can be shown. The tracked path's failed chip additionally says
  how many later messages were returned to the composer, so the user knows
  where the second message went and that Retry re-sends only the first.

### Docs

`docs/guides/api/sends.md` (~136, ~181-187: the 409 is now branch-only, plain
sends queue), `docs/guides/api/sessions.md` (~71),
`docs/guides/api/live-channels.md` (`spawn_failed.unsent`),
`docs/guides/compatibility.md`.

### Tests

Usecase tests under `interactor/enqueue/tests/` (one per file, registered
in `tests/mod.rs`), driving the fakes in `interactor/testing/` with
`interactor_with_git_and_event_sink` and the `WorktreeGate`, modelled on
`send_to_a_still_spawning_session_is_refused.rs` and
`composer_first_send_rolls_back_a_failed_launch.rs`:

- `send_to_a_still_spawning_session_is_queued.rs` — worktree gate closed;
  second plain send returns a `queued` row, no event, no keystrokes, no
  second launch; a branch send in the same state is `Error::SessionSpawning`.
- `queued_sends_flush_once_the_spawn_binds.rs` — two cases in one file or
  two files: (1) with a first prompt: after bind and the first turn's `Stop`,
  the queued send is dispatched in order; (2) prompt-less spawn: after bind
  the queued send is dispatched without any `Stop`, and no keystroke was
  typed from inside the hook (assert the fake tmux saw the line only after
  the hook returned — the fake records call order).
- `a_failed_launch_reports_its_unsent_queued_text.rs` — first prompt plus
  one queued send, launch fails: `SpawnFailed.unsent` lists both rows' ids
  and texts in order; the row and its sends are gone afterwards.
- Cover the adapter-backed provider for the queue + flush case with the
  Codex fakes (a send while the connect gate is held is `queued`; after bind
  it reaches `turn/start` after the first prompt).
- Rewrite `send_to_a_still_spawning_session_is_refused.rs` and
  `lifecycle/tests/send_during_the_launch_window_is_refused.rs` (and the
  Codex sibling) to the branch-send case, or fold them into the new tests.
- Frontend unit: `spawnsSlice` / `applySessionEvent` tests asserting the
  draft is appended (not replaced) with the non-first unsent texts;
  `Composer.test.tsx` ~119-148 (starting → Send now enabled, new
  placeholder); `liveStore.test.ts` spawn_failed cases; mocks tests.
- e2e-fake: extend `slow-start.spec.ts` — type and Send while the launch is
  held → the strip shows the queued row → after the launch binds and the
  first turn ends it is dispatched and answered; and a failing-launch
  scenario (the real-backend `e2e/spawn-failure.spec.ts` or a fake scenario,
  whichever can fail a launch deterministically) asserting the composer holds
  the second message's text after the failed chip appears and nothing was
  re-sent.

Run `make check` and fix whatever it reports.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Session-state coverage for a plain send: **spawning** → accepted as a
      `queued` row (pinned by `send_to_a_still_spawning_session_is_queued.rs`,
      gate appended) and dispatched in order once the session binds, for a
      spawn with a first prompt and for a prompt-less spawn, without typing
      from inside a hook (pinned by `queued_sends_flush_once_the_spawn_binds.rs`,
      gate appended); **closed**, **open + idle**, **open + mid-turn**,
      **resuming** unchanged (existing enqueue tests stay green). A branch
      send to a spawning session is still `409 session_spawning` (gate
      appended).
- [x] A failed launch's `spawn_failed` carries `unsent` (gate appended on
      the wire type) with every send that never reached the agent, in order
      — pinned by `a_failed_launch_reports_its_unsent_queued_text.rs` (gate
      appended) — and the browser appends the non-first texts to the
      new-session composer draft without clobbering it and without
      re-sending, tells the user on the failed chip how many were returned,
      and after a reload (no tracked spawn) returns every text with a
      notice (frontend unit tests).
- [x] Send is enabled while the focused session is spawning, with a
      placeholder that says the message waits for the session (Composer
      unit tests); the e2e-fake `slow-start` spec covers a send during the
      held launch reaching the agent afterwards.
- [x] `sends.md`, `sessions.md`, `live-channels.md` and `compatibility.md`
      describe the queued acceptance, the branch-only 409 and
      `spawn_failed.unsent`.

## Out of scope

- Editing a queued send's text before it is sent.
- Cross-client draft sync: every connected client that receives
  `spawn_failed` restores the text into its own local composer draft; that
  is intended.
- Any change to what the Retry chip re-sends (the first prompt only).
