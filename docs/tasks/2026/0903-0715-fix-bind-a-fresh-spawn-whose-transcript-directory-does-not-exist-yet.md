---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -q "accepts_a_jsonl_whose_parent_directory_does_not_exist_yet" backend/crates/domain/delta-usecase/src/interactor/hooks/validate_transcript_path.rs && grep -q "rejects_a_dotdot_escape_through_a_missing_parent" backend/crates/domain/delta-usecase/src/interactor/hooks/validate_transcript_path.rs && grep -rlq "leaves_the_spawn_pending" backend/crates/domain/delta-usecase/src/interactor/hooks/tests && ! grep -rq "Claude Code creates the per-project directory before writing into it" backend/crates/domain/delta-usecase/src/interactor/hooks/ && [ "$(cd backend && cargo test -q -p delta-usecase -- --list 2>/dev/null | grep -c '"'"': test$'"'"')" -ge 424 ]'
assignee: null
branch: task/0903-0715-fix-bind-a-fresh-spawn-whose-transcript-directory-does-not-exist-yet
created_at: 2026-09-03T07:15:00Z
updated_at: 2026-09-03T09:55:49Z
---

# fix(hooks): bind a fresh spawn whose transcript directory does not exist yet

## Overview

A new Delta session launched in a directory Claude Code has never run in
before never binds: its card stays `spawning` (orange) forever, the
conversation pane stays empty, and only the terminal pane works after a
reload. Every launch that creates a fresh per-session worktree
(`~/.delta/worktrees/<repo>-<session-id>`) hits this, deterministically. The
regression came in with the transcript-path confinement (PR #369, merged
2026-09-02); the first fresh-worktree launch after it (2026-09-03) exposed it.

### What happens

Claude Code writes the transcript to
`<claude-config-dir>/projects/<cwd-slug>/<session-id>.jsonl` and creates the
`<cwd-slug>/` directory **lazily, on the first transcript write — which comes
after the `SessionStart` hook has fired**. For a cwd that has hosted a Claude
Code session before, the directory already exists when the hook arrives; for a
first-ever cwd it does not (observed: directory birth time and the hook
failure share the same second, the hook first).

`validate_transcript_path`
(`backend/crates/domain/delta-usecase/src/interactor/hooks/register_session_row.rs`,
`fn validate_transcript_path`) canonicalizes the transcript's **parent
directory** to defeat `..` and symlinks, on the stated assumption that "it
exists — Claude Code creates the per-project directory before writing into
it". That assumption is false at `SessionStart` time for a fresh cwd, so the
check fails with
`invalid transcript path: …/<cwd-slug>/<id>.jsonl: parent directory is unresolvable: No such file or directory`
and `SessionStart(startup)` returns an error. The existing unit tests all use
an existing `tempdir()` as the parent, which is why this shape was not caught.

That alone would be a transient failure: the first `UserPromptSubmit` arrives
~70 ms later, by which time the directory exists. But
`SessionContext::bind_pending_spawn`
(`backend/crates/domain/delta-usecase/src/interactor/hooks/bind_pending_spawn.rs`)
performs the runtime transition **first** — `self.state.bind_pending_spawn()`
takes the `PendingSpawn` and binds the pane
(`session_actor/runtime/spawn.rs:196`) — and only **then** calls
`register_session_row` (validation + `spawning → active` row update). When
registration fails, the `?` propagates the error but nothing undoes the
runtime bind. Every later hook therefore sees no pending spawn, returns
`Ok(None)`, falls back to the stored `spawning` row (whose `transcript_path` is
still `NULL`), and proceeds; `ClaudeConversationSource::next_batch`
(`interactor/sync/conversation_source.rs:72`) sources nothing for a row without
a transcript path, so no message is ever ingested. The session is wedged with
no retry and no log line after the first error. The stale-pending sweep
(`take_stale_pending`) cannot rescue it either, because the spawn is no longer
pending.

### Fix — two parts of the same defect, one PR

**Part A — validate a transcript path whose parent does not exist yet.**
Rewrite `validate_transcript_path` so that it does not require the parent
directory to exist, while keeping every guarantee the current version gives:

- still requires the `.jsonl` extension and an absolute path;
- reject lexically any `..` (`Component::ParentDir`) or `.`
  (`Component::CurDir`) component in the incoming path, so a traversal can no
  longer hide behind a directory that does not exist yet;
- canonicalize `root` as today;
- walk up from the transcript path to its **deepest existing ancestor**,
  canonicalize that ancestor (this is what collapses a symlinked prefix such as
  macOS `/tmp` → `/private/tmp`), re-join the not-yet-existing tail
  components onto it, and require the result to `starts_with` the canonical
  root. A missing tail cannot contain a symlink, so resolving only the existing
  prefix is sound.

Correct the doc comment: drop the false claim about when Claude Code creates
the directory and state the real ordering (`SessionStart` fires before the
first transcript write, so for a first-ever cwd the per-project directory does
not exist yet). Add unit tests next to the existing ones (the validator and its tests live in
their own module, `hooks/validate_transcript_path.rs`, split out of
`register_session_row.rs` during refine):

- `accepts_a_jsonl_whose_parent_directory_does_not_exist_yet` — root exists,
  `<root>/new-project/<id>.jsonl` where `new-project/` is absent → `Ok`;
- `rejects_a_dotdot_escape_through_a_missing_parent` —
  `<root>/missing/../../secret.jsonl` → `Err` (and a sibling-of-root path via a
  missing parent is also refused);
- a symlinked root (a `tempdir()` symlink pointing at the real root) still
  accepts a not-yet-created transcript beneath it.

**Part B — keep the spawn pending until its registration succeeds.**
Reorder `SessionContext::bind_pending_spawn` so the runtime transition is the
last step: check whether a spawn is pending without consuming it
(`SessionRuntime::pending_spawn()` at `runtime/spawn.rs:413` already exists;
widen its visibility or add an `is_pending()`/`has_pending_spawn()` accessor
as fits), return `Ok(None)` if nothing is pending, call `register_session_row`
and propagate its error **before** touching the runtime, then take and bind
the pending spawn and post `FlushQueuedSend` as today. A failed registration
thus leaves the `PendingSpawn` in place, so the next hook for the id retries
the bind, and `take_stale_pending` still reports the launch as failed if no
hook ever succeeds. Update the doc comment on `bind_pending_spawn` (both the
`SessionContext` method and the runtime method's "whichever arrives first"
paragraph) to state this ordering and why.

Add a usecase test under
`backend/crates/domain/delta-usecase/src/interactor/hooks/tests/` (one test
per file, as the siblings do; see
`session_start_startup_binds_pending_spawn.rs` and
`session_start_then_user_prompt_bind_once.rs` for the harness) whose name
contains `leaves_the_spawn_pending`: build the interactor with a transcript
root set to a `tempdir()` (`Interactor::with_transcript_root`, called before
any actor is spawned — see `interactor/mod.rs:388`; add a `testing::factory`
constructor for it if the existing ones do not expose it), spawn a session,
send `SessionStart(startup)` with a `transcript_path` **outside** the root so
`register_session_row` rejects it, and assert that the call returns
`Err(Error::InvalidTranscriptPath(_))`, the id is still in
`pending_session_ids()`, `pane_for_session` is `None`, and the stored row is
still `spawning` with no transcript path. Then send a `UserPromptSubmit` (or a
second `SessionStart(startup)`) with a valid path under the root and assert
the session binds and registers normally — `SessionRegistered` emitted, pane
bound, row `active` with that transcript path. The hook builders in
`testing/hooks.rs` (`session_start`, `submit_for`) take the transcript path;
extend them if a `SessionStart` builder with an explicit path is missing.

Also add a second usecase test (or fold it into the same file if it stays
readable) that reproduces the reported shape end to end under the fixed
validator: with the root set, send `SessionStart(startup)` whose
`transcript_path` is `<root>/<never-created-dir>/<id>.jsonl` and assert it
binds and registers on the first call.

### Session-state coverage

This task changes no operation a user triggers; it changes how the first
hooks of a launch bind the session. The states the bind can meet are
enumerated by the tests above: pending spawn + hook accepted (existing
`session_start_startup_binds_pending_spawn`), pending spawn + hook rejected
then retried (new), already bound (existing
`session_start_then_user_prompt_bind_once`), and no pending spawn / external
id (existing `session_start_unknown_session_is_a_safe_noop`,
`unknown_session_without_pending_spawn_registers_external_closed`).

### Pipeline notes

- Backend only. `make check` is the canonical gate; the appended greps assert
  the new test names, the removal of the false doc-comment claim, and the
  `delta-usecase` test count (421 on `main`; at least three new tests are
  expected, so the gate is ≥ 424). All appended gates fail on `main`.
- The `fake-claude` `full_loop` test and the e2e-fake suite write transcripts
  under a pre-created `FAKE_CLAUDE_TRANSCRIPT_DIR`, so they exercised only the
  existing-parent path; they need no change, but they must stay green.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `validate_transcript_path` accepts a `.jsonl` path under the root whose
      parent directory does not exist yet, and still refuses `..` escapes
      (including through a missing parent), non-`.jsonl` targets, and paths
      resolving outside the root (unit tests
      `accepts_a_jsonl_whose_parent_directory_does_not_exist_yet` and
      `rejects_a_dotdot_escape_through_a_missing_parent`; grep gates in
      `check_command`).
- [x] The doc comment on `validate_transcript_path` no longer claims that the
      per-project directory exists when the hook fires (negative grep gate over
      `hooks/` in `check_command`).
- [x] A `SessionStart(startup)` / first `UserPromptSubmit` whose registration
      is rejected leaves the spawn pending — pane unbound, row still
      `spawning` — and a later hook with a valid transcript path binds and
      registers it (usecase test whose name contains
      `leaves_the_spawn_pending`; grep gate).
- [x] A `SessionStart(startup)` naming a transcript in a not-yet-created
      per-project directory binds and registers on the first call (usecase
      test; `delta-usecase` test count ≥ 424).

### Manual / on-hardware (verified by a human before merge)

- [ ] In a live `make dev` session with real `claude`, starting a new session
      in a **fresh** worktree (a cwd Claude Code has never run in on this
      machine) binds immediately: the card leaves the orange `spawning`
      state, the conversation pane shows the first turn without a reload, and
      the server log shows `SessionStart(startup): bound and registered a
      pending spawn` instead of `invalid transcript path`. (Non-blocking for
      merge under the CI-green autonomous policy; recorded for dogfooding.)

## Out of scope

- Repairing an already-wedged row left behind by the bug in a live database;
  that is an operator action, not a code change.
- A warning or watchdog for a bound pane whose row has no transcript path.
  With Part B a bound pane always has a registered row, so the state this
  would detect can no longer arise from the hook path.
- Any change to the hook secret, the transcript root derivation
  (`DELTA_TRANSCRIPT_ROOT`), or the nested-subagent transcript comparison in
  `hook_transcript_guard.rs`.
