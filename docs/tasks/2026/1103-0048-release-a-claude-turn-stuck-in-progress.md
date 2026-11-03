---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure, error-type-design, user-experience]
max_refine_rounds: 3
retries_remaining: 0
check_command: "make check && grep -rq 'stop-turn' backend/crates/apps/delta-server/src/ && grep -rq 'stop-turn' docs/guides/api/ && test -n \"$(grep -rl 'stop-turn' frontend/packages/apps/web/e2e-fake/)\""
assignee: null
branch: task/1103-0048-release-a-claude-turn-stuck-in-progress-retry-1
created_at: 2026-11-02T15:48:04Z
updated_at: 2026-11-03T01:36:18Z
---

# feat: stop a running Claude turn from the In progress strip

## Overview

A Claude session can stay "In progress" forever, and every later send to it
then waits as `queued` behind a turn that no longer exists.

Reproduced while dogfooding with Claude Code 2.1.316: a send was dispatched,
its `UserPromptSubmit` arrived (the turn machine went `InFlight`), and the
user pressed Escape in the attached pane before any assistant output
streamed. Claude Code then drops the prompt from its screen and leaves no
trace of the cancel:

- the transcript ends at the user line and its `api-request` record — no
  assistant line and no `[Request interrupted by user]` marker, so the
  existing marker path (`delta_attribution::claude_format::is_interrupt_marker`
  → `TurnInput::Interrupt`) never runs;
- no `Stop` hook fires (Claude Code does not fire `Stop` on a user interrupt,
  and `StopFailure` covers API errors only — its `error` enum has no
  user-cancel value). No hook signals the cancel.

So nothing ever moves the session's turn machine
(`backend/crates/domain/delta-usecase/src/turn/`) out of `InFlight`, and
`dispatch_queued_send` keeps deferring. The send typed afterwards stayed
`queued` indefinitely.

There is no reliable automatic signal for this, and scraping the TUI was
rejected as too brittle. The fix is a **Stop** control: the browser asks
Delta to stop the running turn, and Delta both stops Claude (by injecting
Escape into the pane, as the dispatched-send Cancel already does) and ends
the turn on its own side without waiting for a marker that may never come.
One control then covers both cases with the same meaning — when Claude is
really working it is interrupted; when Delta has merely lost track of a turn
Claude already abandoned, the strip recovers.

### Starting point

An earlier iteration of this task implemented a *release-only* variant (end
Delta's turn without touching the pane, `POST /api/sessions/{id}/release-turn`,
a "Mark as ended" control) and was refined before the design changed. Its
full diff against `main` is at `.tmp/release-turn-v1.patch` in the
repository root (git-ignored; it applies cleanly with `git apply`). Apply it
first and rework it into the Stop design below — the actor routing, the
state checks, the dedicated refusal error mapped to 409, the API test, the
client/mock wiring, the component-test split, and the e2e scenario are all
still wanted. Rename the release / "Mark as ended" vocabulary to stop /
"Stop" throughout (module, actor input, error variant and its refusal enum,
endpoint, error code, client call, hook, tests, docs) so no `release-turn`
or "Mark as ended" remains.

### The change

1. **A use-case operation that stops a Claude session's in-flight turn.**
   Routed through the session's actor (like `cancel_send` / `release_send`
   in `backend/crates/domain/delta-usecase/src/interactor/`), so it is
   ordered against hooks and dispatch. When the turn machine is `InFlight`:
   inject Escape into the session's pane through the `TmuxDriver` (the same
   keystroke the dispatched-send cancel injects — reuse that path), then
   resolve the in-progress turn's thread (the way `on_stop` resolves it,
   before the transition), feed `TurnInput::Interrupt` (`InFlight` → `Idle`,
   with its `SettleIfUnmatched` orphan handling), and emit
   `SessionEvent::TurnInterrupted`. If the Escape injection fails, return the
   error without touching the turn machine, so the turn is not reported as
   stopped when nothing reached Claude. A marker that Claude Code writes later
   for the same interrupt lands on `(Idle, Interrupt)`, which is already a
   no-op for the machine; make sure it does not emit a second
   `TurnInterrupted` that would confuse the browser, or document why a
   duplicate is harmless. Log the stop at `info`.
2. **The queued send behind the stopped turn.** Claude Code (2.1.316)
   restores a prompt it dropped before any reply streamed into its composer,
   and the Escape does not clear it, so a queued send typed at once would be
   spliced into it (seen before merge: Claude received
   "Write a 400-Reply with the single word PONG."). After the Escape, inject
   `Ctrl+C` as a separate keystroke: it empties the whole composer (multi-line
   and wherever the cursor is, unlike `C-u`), and on an empty composer only
   shows Claude Code's two-second exit hint, which the next typed send
   dismisses. An `ESC` immediately followed by `0x03` is read as `Meta+C`, so
   the two keys go through `send_keys`, one invocation each. Then dispatch the
   next queued send at once, the same way an ingested interrupt marker does.
   Teach fake-claude's line editor that `0x03` clears its buffer.
3. **States the operation must handle** (each one gets a usecase test):
   - `InFlight { send_id: Some(_) }` and `InFlight { send_id: None }` →
     Escape injected, turn stopped as above.
   - `Idle` → refused with a conflict (nothing to stop); no keystroke, no
     event, no dispatch.
   - `AwaitingEcho` → refused with a conflict: the dispatched send's own
     Cancel covers that state, and stopping there would race the echo.
   - A session with no live pane (closed / ended) → refused; nothing runs.
   - A Codex (terminal-less) session → refused: Codex is stopped through the
     existing `/interrupt` endpoint.
   - The Escape injection fails → the error surfaces, the turn stays
     `InFlight`, no event.
   Use a dedicated error variant for the refusal, mapped to `409 Conflict`
   at the API boundary.
4. **`POST /api/sessions/{id}/stop-turn`** in `delta-server`
   (`src/api/sessions.rs` and the router), broadcasting the returned events
   the way the other actor-routed endpoints do. Document it in
   `docs/guides/api/sessions.md` and the API index, including that it sends
   Escape to the pane and ends Delta's turn even when Claude Code leaves no
   interrupt marker. Leave the existing `/interrupt` endpoint unchanged. Also
   point the `send_not_cancellable` 409 description in
   `docs/guides/api/sends.md` (which tells the user to press Escape in the
   pane) at the new endpoint.
5. **A "Stop" control in the "In progress" strip**
   (`frontend/packages/apps/web/src/features/composer/SendActivityStrip.tsx`)
   shown for a Claude session whose turn is running (the running state that
   drives the strip's spinner) and that has no dispatched send still
   awaiting its echo — that row keeps its own Cancel. Clicking it calls the
   new endpoint; on success the `turn_interrupted` event clears the
   spinner; on a refusal show the app-wide error snackbar the strip already
   uses for a failed cancel, then reconcile by refetch. Label it "Stop";
   the tooltip says it interrupts Claude, like pressing Escape in the
   terminal. Add the client call to the gateway package and update the
   api-mocks so the dev/mock server supports it.
6. **An e2e-fake spec** (`frontend/packages/apps/web/e2e-fake/`, mentioning
   the endpoint path `stop-turn` so the check gate finds it) that drives a
   fake-claude scenario into a turn that never ends on its own — a prompt is
   submitted and no `Stop` and no interrupt marker follow — queues a second
   send behind it, clicks Stop, and asserts the spinner clears and the queued
   send leaves `queued`. Make the fake consume the injected Escape the way
   the real TUI does when it writes nothing (see the `AwaitEscape` /
   `AwaitInterrupt` steps in `backend/crates/apps/fake-claude/src/scenario/step.rs`).

Out of scope: detecting the silent cancel automatically (hooks cannot see
it; TUI scraping was rejected), and changing the Codex interrupt path.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Usecase tests cover the stop operation in every state listed in
      "The change" step 3: both `InFlight` variants inject Escape, end the
      turn, and emit `TurnInterrupted`; `Idle`, `AwaitingEcho`, a closed
      session, and a Codex session are refused with no keystroke, no event,
      and no dispatch; a failed Escape injection leaves the turn `InFlight`.
- [x] A later interrupt marker for the same turn does not produce a second
      `TurnInterrupted`, or a test pins the documented harmless duplicate.
- [x] The refusal is a dedicated error variant mapped to `409 Conflict` by
      `delta-server`, covered by a test at the API layer.
- [x] `POST /api/sessions/{id}/stop-turn` exists in `delta-server` and is
      documented under `docs/guides/api/` (both grepped by the check
      command).
- [x] The strip shows "Stop" only for a Claude session with a running turn
      and no dispatched send awaiting its echo, and hides it otherwise
      (component tests in `check-frontend-test`).
- [x] An e2e-fake spec drives a turn that never ends on its own, clicks
      Stop, and sees the spinner clear and the queued send leave `queued`
      (runs in `check-e2e-fake`; the check command asserts a spec mentions
      `stop-turn`).

### Before merge (verified outside the check command)

- [x] In `make dev` with a real Claude Code session, the stuck case: dispatch
      a send, press Escape in the pane before any reply streams, confirm the
      strip stays "In progress", click Stop, and confirm the strip clears
      and a send queued behind it is typed into an empty composer (not
      concatenated with the cancelled prompt) and answered.
- [x] In `make dev`, the running case: while Claude is streaming a reply,
      click Stop and confirm Claude is interrupted, the strip clears, and a
      queued send is delivered once and answered.

## Last subagent report

Before-merge verification (stuck case) found the restored-composer splice
described in step 2. Fixed by the `Ctrl+C` after the Escape; the key choice
was verified against Claude Code 2.1.316 in a scratch tmux pane (Escape,
then `C-c` 100 ms or more later, clears a restored multi-line prompt and the
next paste lands alone).
