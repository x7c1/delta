---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, error-type-design]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && ! grep -rq "Only a store error is an" backend/crates/domain/delta-usecase/src/ && cd backend && RUSTDOCFLAGS="-D warnings" cargo doc -p delta-server --no-deps --document-private-items'
assignee: null
branch: task/1019-0330-keep-one-session-transcript-failure-from-dropping-the-tick
created_at: 2026-10-18T18:30:40Z
updated_at: 2026-10-18T19:13:47Z
---

# fix(server): keep one session's transcript failure from flooding the log or dropping the tick

## Overview

Follow-ups to the change that reports a refused transcript once. Four small
corrections in the same area, bundled so they share one pipeline run.

**1. Passing transcript failures repeat every tick and drop other sessions'
results.** A refused transcript (too large, not a regular file) is now
reported once per session. A passing failure (I/O, parse, aborted read) is
not: a failure that keeps happening, such as `permission denied` on the
transcript file, is logged at `warn` on every 500 ms tail tick. Worse,
`collect_tick_replies` in
`backend/crates/domain/delta-usecase/src/interactor/routing/ticks.rs` does
`replies.push(result?)`, so one session's `Err` makes the whole tick return
`Err` and discards the replies already collected from, and still pending
for, every other session (their ingest itself still happens inside each
actor). See `interactor/sync/poll_transcript.rs` (`sync_tick`) for where the
refusal state lives today.

Fix:

- One session's error must not discard any other session's reply in the
  same tick. Report the failing session's error (naming the session and the
  stage, rendering the cause chain with `with_causes`) and keep collecting.
  `collect_tick_replies` is shared by the transcript poll and three other
  sweeps; check what each caller does with the `Err` today and keep their
  reporting at least as visible — no error may become silent.
- A passing transcript failure that repeats must not log once per tick.
  Report it when it starts (at `warn`, with the cause chain), stay quiet
  while the same failure repeats, report again if it changes, and log the
  recovery at `info`. Reuse or generalize the per-session state already kept
  for refusals rather than adding a second mechanism, and explain in a
  comment what counts as "the same failure".

**2. Parse failures do not say which transcript or line failed.** The
gateway's `Error::Parse`
(`backend/crates/gateway/delta-transcript/src/error.rs`) and the use-case
`TranscriptError::Parse`
(`backend/crates/domain/delta-usecase/src/ports/transcript_error.rs`) carry
only the `serde_json::Error`, so the log cannot lead an operator to the
broken transcript. Have the reader attach the transcript path and the line
number (1-based) where the parse failed, carry them through to the
use-case error, and show them in its message. Keep the `serde_json::Error`
as the `source`.

**3. A stale doc comment on `readopt_remembered_pane`.** In
`backend/crates/domain/delta-usecase/src/interactor/lifecycle/adopt_pane.rs`
the doc says "Only a store error is an `Err`". The `PortMoved` branch calls
`end_for_moved_port()?`, which goes through close, so tmux and transcript
failures also come back as `Err`. Make the doc state what can actually be an
`Err`.

**4. `cargo doc -p delta-server` fails without `embed-web`.** The module doc
of `backend/crates/apps/delta-server/src/app/mod.rs` links to
[`static_web`], but that module is only compiled under
`#[cfg(any(test, feature = "embed-web"))]`, so documenting the crate alone
without the feature fails on a broken intra-doc link (the gate's
`--workspace` build happens to enable the feature, so `make check` passes).
Make the doc resolve, or stop linking, in both configurations. The
`cargo doc -p delta-server` appended to `check_command` fails on the current
tree.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A test runs a tick over several sessions where one session's sync
      returns an error and asserts the other sessions' replies are still
      returned from that tick.
- [x] A test polls a session whose transcript fails with the same passing
      error several ticks in a row and asserts it is reported once, reported
      again after the session recovers and fails again, and that a recovery is
      reported.
- [x] A test feeds the reader a transcript with an unparsable line and asserts
      the resulting use-case error names the transcript path and the line
      number, and still exposes the `serde_json::Error` as its source.
- [x] `RUSTDOCFLAGS="-D warnings" cargo doc -p delta-server --no-deps
      --document-private-items` succeeds without `embed-web` (appended to
      `check_command`).
- [x] The doc of `readopt_remembered_pane` no longer claims only a store
      error is an `Err` (`! grep -rq "Only a store error is an"` over
      `delta-usecase/src/` is appended to `check_command`).
