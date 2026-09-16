---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -rq reaps_the_row backend/crates/domain/delta-usecase/ && ! grep -q dormant backend/crates/apps/delta-server/src/main.rs && ! grep -q dormant backend/crates/apps/delta-server/tests/end_to_end.rs && (cd backend && RUSTDOCFLAGS='-D warnings' cargo doc -p fake-claude --no-deps)"
assignee: null
branch: task/0916-2030-chore-name-the-failed-launch-tests-after-what-they-assert
created_at: 2026-09-16T20:30:00Z
updated_at: 2026-09-16T20:51:54Z
---

# chore(tests): name the failed-launch tests after what they assert, and fix stale doc wording

## Overview

Four places in the test and tooling code say something the code no longer
does. None of them changes behaviour; all four are names or comments that
contradict what sits next to them, so a reader who trusts the wording is
misled. This task applies one rule — wording must match the code it
describes — to every instance found on `main` today.

### 1. Three lifecycle tests claim to reap a row they actually keep

Under `backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests/`:

- `a_failed_codex_launch_reaps_the_row_and_reports_spawn_failed.rs`
- `failed_launch_preparation_reaps_the_row_and_reports_spawn_failed.rs`
- `tmux_failure_after_the_pending_spawn_is_recorded_reaps_the_row.rs`

Each file name and test function says the failed launch *reaps the row*,
but each one asserts the opposite, in these words:

```
"the eager row of a failed launch is marked failed, not deleted"
```

Since #402 a failed launch keeps its session row and marks it `failed`, so
the failure has a screen of its own. The assertions were updated then; the
names were not (the refine that found this in #402 could not apply it — a
`git mv` is outside the comment-only sweep).

Rename all three so the name states the behaviour the test pins:

| current | new |
| --- | --- |
| `a_failed_codex_launch_reaps_the_row_and_reports_spawn_failed` | `a_failed_codex_launch_marks_the_row_failed_and_reports_spawn_failed` |
| `failed_launch_preparation_reaps_the_row_and_reports_spawn_failed` | `failed_launch_preparation_marks_the_row_failed_and_reports_spawn_failed` |
| `tmux_failure_after_the_pending_spawn_is_recorded_reaps_the_row` | `tmux_failure_after_the_pending_spawn_is_recorded_marks_the_row_failed` |

For each: `git mv` the file, rename the `async fn`, update the `mod` line in
`tests/mod.rs` (lines 5, 41, 80 today), and update the two prose
cross-references that cite the old names — the doc comment of
`a_refused_codex_launch_option_fails_the_send_itself.rs` (line 13) and the
doc comment of the Codex test itself, which cites the Claude-path test by
name. Read each test's doc comment after renaming and correct any sentence
that still describes the row as removed or rolled back; the pending-spawn
*record* being removed (the tmux test's subject) is still true and stays.

### 2. Two comments still call the async event seam dormant

The seam has carried live traffic since #397 (the transcript tail emits its
per-session ingest announcements on it), and #398 rewrote the seam's own
docs to say so — see the "Live today:" list on
`Interactor::emit_async_event` in
`backend/crates/domain/delta-usecase/src/interactor/mod.rs`. Two comments
outside #398's scope still describe it as unused:

- `backend/crates/apps/delta-server/src/main.rs`, the comment above
  `state.spawn_async_event_drain()`: "Wired but dormant in this slice — no
  live path emits on the seam yet."
- `backend/crates/apps/delta-server/tests/end_to_end.rs`, the doc comment
  of `async_event_seam_reaches_the_broadcast`: "No Claude path emits on the
  seam (it is dormant), so this drives it directly through the interactor's
  public emit."

Rewrite both to match the seam's current state. The `main.rs` comment
should say what the drain is for and point at `emit_async_event` for who
emits; the test doc should say that the test drives the seam directly to
prove the plumbing in isolation from any real producer, not because there
is none. Do not touch `delta-server/src/comms.rs` or
`docs/guides/api/live-channels.md`: their "dormant" describes a session
with no live wire, which is a different and still-correct use of the word.
The `check_command` therefore greps the two files by name, not the
directory.

### 3. Two rustdoc links in `fake-claude` do not resolve

`RUSTDOCFLAGS='-D warnings' cargo doc -p fake-claude --no-deps` fails
today with two unresolved intra-doc links:

- `backend/crates/apps/fake-claude/src/args.rs`, `Args::parse`'s doc:
  `argv[0]` is read as a link to `0`. Put it in a code span.
- `backend/crates/apps/fake-claude/src/input.rs`, the doc of the
  byte-advance helper near line 249: `` [`decode_stream`] `` does not
  resolve to the free function at line 155. Fix the link path so it
  resolves, or demote it to a plain code span if the item cannot be linked
  from there — whichever `cargo doc` accepts without a warning.

`make check` does not run `cargo doc`, so the `check_command` appends that
exact invocation as the gate for this part.

### Session-state coverage

Not applicable: no operation against a session is added or changed.

### Pipeline notes

- Rust test files and comments only; no wire, schema or frontend change.
- The three grep gates and the `cargo doc` gate in `check_command` were
  negative-tested at authoring time: each fails on `main` today.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] No file, module or function under `delta-usecase/` is still named
      `reaps_the_row` (`! grep -rq reaps_the_row
      backend/crates/domain/delta-usecase/`, appended to `check_command`),
      and the three renamed tests still pass under `cargo test`.
- [x] The two cross-references that cited the old test names point at the
      new names (covered by the same grep: an un-updated citation would
      still contain `reaps_the_row`).
- [x] Neither `delta-server/src/main.rs` nor
      `delta-server/tests/end_to_end.rs` calls the async event seam
      dormant (`! grep -q dormant <file>` for each, appended to
      `check_command`).
- [x] `cargo doc -p fake-claude --no-deps` is warning-free under
      `RUSTDOCFLAGS='-D warnings'` (appended to `check_command`).

## Out of scope

- Renaming anything other than the three tests named above.
- The seam's own documentation in `delta-usecase` (already updated by #398).
- Wiring `cargo doc` into `make check` for every crate.
