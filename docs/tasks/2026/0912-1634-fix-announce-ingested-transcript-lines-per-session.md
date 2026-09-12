---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -rq "TranscriptUpdated" backend/crates/domain/delta-usecase/src/interactor/sync/ && ! grep -q "SessionEvent::TranscriptUpdated" backend/crates/apps/delta-server/src/state.rs'
assignee: null
branch: task/0912-1634-fix-announce-ingested-transcript-lines-per-session
created_at: 2026-09-12T16:34:07Z
updated_at: 2026-09-12T17:43:19Z
---

# fix(server): announce ingested transcript lines per session, not after every session's tick

## Overview

The background transcript tail announces new messages to the browser only
after **every** session has answered the tick. `poll_transcript`
(`backend/crates/domain/delta-usecase/src/interactor/routing.rs`, around
line 735) posts a `SyncTick` to each session actor, then awaits all the
replies in a loop, and only then returns the per-session groups; the server
loop (`backend/crates/apps/delta-server/src/state.rs`,
`spawn_transcript_tail`) turns each group into a
`SessionEvent::TranscriptUpdated` broadcast after the whole fan-out has
returned. One actor that is slow to answer therefore delays the
`TranscriptUpdated` of every other session — messages already persisted by
their actors sit unannounced, and a browser that relies on that event to
refetch shows nothing. The three sibling fan-outs in the same tick
(`dispatch_ready_resumes`, `reap_stale_spawns`, `sweep_echo_deadlines`)
have the same shape and run before the transcript poll, so a stall in any of
them also holds the poll back.

This is the shape of the failure the fake-mode e2e suite hit in CI on
2026-09-12 (`branch-defer.spec.ts:29`: `message-item` count stayed 0 for
15 s while the fake had written both transcript lines and the server log
shows the session bound and its `UserPromptSubmit` accepted, then nothing
for the rest of the wait). Which actor stalled is not in the evidence; what
is, is that a single stall can silence every session's announcements, and
that nothing in the logs says so.

Make each session announce its own ingested lines the moment they are
persisted, bound how long a tick waits on any one actor, and log when a tick
runs long.

### Design

1. **Announce from the actor.** In `sync_tick`
   (`backend/crates/domain/delta-usecase/src/interactor/sync/poll_transcript.rs`),
   after `sync_transcript` returns a non-empty batch, build the
   `SessionEvent::TranscriptUpdated { session_id, thread_ids }` (the
   distinct thread ids of the batch, exactly as the server loop computes it
   today) and push it — together with the batch's other `SessionEvent`s —
   onto the interactor's async event seam via `emit_async_event`
   (`interactor/mod.rs`, around line 658; the sink is wired in production by
   `AppState::build` in `state.rs` around line 105 and drained by
   `spawn_async_event_drain`). The actor already runs inside the mailbox
   that serializes the ingest, so the event leaves at the same moment the
   rows land. Keep `sync_tick`'s return value so existing callers and tests
   still see the batch.
2. **Stop announcing from the loop.** In `spawn_transcript_tail`, drop the
   `TranscriptUpdated` construction and the re-broadcast of the poll's
   returned events — they now arrive through the drain, and broadcasting them
   twice would make the browser refetch twice. Leave `poll_transcript`'s
   return type alone (tests and the interrupt path use it). The gate appended
   to `check_command` pins both halves: the event is built under
   `interactor/sync/`, and `state.rs` no longer names it.
3. **Bound the fan-out.** In `routing.rs`, make the four tick fan-outs await
   each actor's reply with a timeout instead of indefinitely. Take the bound
   as a `Duration` parameter from the caller (the server passes its poll
   interval, `TRANSCRIPT_POLL_INTERVAL`; tests pass what they need) rather
   than a constant buried in the use-case crate. On a timeout, `warn!` with
   the session id and the stage name and continue with the remaining replies;
   the late actor's work is not lost — its rows are persisted and, with step
   1, its announcement is on the seam — only its contribution to that tick's
   return value is skipped. Extract the shared "post to every actor, collect
   bounded replies" shape into one helper the four call sites use, so the
   bound is applied once.
4. **Say when a tick runs long.** In `spawn_transcript_tail`, measure the
   tick and `warn!` when it exceeds the poll interval, naming the stage
   durations (resume dispatch, reap, echo sweep, transcript poll). At
   `debug`, log each session's ingested message count from `sync_tick`
   when it is non-zero, so a future log shows which sessions ingested what and
   when — the absence that made this incident undiagnosable.
5. **Do not touch** the attribution fold, the transcript reader, the
   per-session cursor, the echo watchdog's deadlines, or the frontend's
   handling of `TranscriptUpdated`.

### Tests

- `FakeTranscript::gate_reads` (`interactor/testing/fake_transcript.rs`)
  already parks a session's read on a `Barrier`; the existing
  `session_actor/tests/sessions_ingest_concurrently_while_a_third_handles_a_hook.rs`
  shows the pattern. Use it to hold session A's read open while session B
  ingests, and assert that B's `TranscriptUpdated` reaches an
  `AsyncEventSink` wired with `with_event_sink` **before** A's barrier is
  released.
- With A still held and a short bound, assert `poll_transcript` returns B's
  group within the bound rather than waiting on A, and that a later tick
  after A's release still surfaces A's rows to the store (nothing is lost).
- A tick whose every actor answers promptly behaves exactly as before: one
  group per session that ingested, no timeout branch taken.

### Pipeline notes

- Rust only (`delta-usecase`, `delta-server`). No wire change (the event
  shape is unchanged), so no `make gen`.
- `make check` takes over ten minutes and needs tmux on the host; the check
  phase is expected to run it through the driver's long-running path rather
  than a capped worker call.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `TranscriptUpdated` is built and emitted from the ingest path under
      `interactor/sync/`, and the server's tail loop no longer constructs it
      (the two gates appended to `check_command`).
- [x] With one session's transcript read held on a barrier, another session's
      `TranscriptUpdated` arrives on the async event sink before the barrier
      is released (cargo test, `delta-usecase`).
- [x] With one session's read held and a short bound, `poll_transcript`
      returns the other session's group within the bound, and the held
      session's rows are ingested by a later tick once released (cargo test,
      `delta-usecase`).
- [x] The four tick fan-outs share one bounded-reply helper and accept the
      bound from the caller; `cargo clippy` stays clean under `make check`.
- [x] The fake-mode suite passes inside `make check`, including
      `branch-defer.spec.ts`, `ws-reconnect.spec.ts`, and
      `server-restart.spec.ts` (the paths most sensitive to how
      `TranscriptUpdated` reaches the browser).

### Manual / on-hardware (verified by a human before merge)

- [ ] On a live `make dev` server with two open sessions, sending in one and
      leaving the other idle shows the reply in the browser without a reload,
      and `RUST_LOG=delta_usecase=debug` shows one ingested-count line per
      batch.

## Out of scope

- Finding which actor stalled in the 2026-09-12 CI run; the evidence needed
  for that (the tick-duration warning and per-session ingest logs) is what
  this change adds.
- Any change to the browser's refetch behaviour or to the echo/spawn
  watchdog deadlines.
