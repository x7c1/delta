---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && ! grep -rq "allow(dead_code)" backend/crates/apps/fake-codex/tests/full_loop backend/crates/apps/fake-codex/tests/adapter_contract && [ "$(find backend/crates/apps/fake-codex/tests/full_loop backend/crates/apps/fake-codex/tests/adapter_contract -name "*.rs" -exec wc -l {} \; | sort -rn | head -1 | cut -d" " -f1)" -lt 1000 ] && [ "$(ls backend/crates/apps/fake-codex/tests/*.rs | wc -l)" -eq 2 ] && [ "$(cd backend && cargo test -p fake-codex --test full_loop -- --list | grep -c ": test$")" -eq 22 ] && [ "$(cd backend && cargo test -p fake-codex --test adapter_contract -- --list | grep -c ": test$")" -eq 30 ]'
assignee: null
branch: task/0821-1244-refactor-split-fake-codex-test-suites
created_at: 2026-08-21T12:44:19Z
updated_at: 2026-08-21T14:12:00Z
---

# refactor(fake-codex): split the full-loop and adapter-contract suites into directory modules

## Overview

Two integration-test files in `backend/crates/apps/fake-codex/tests/` have grown
past the point where the whole picture is readable:

- `full_loop.rs` — 2807 lines, 22 tests
- `adapter_contract.rs` — 1219 lines, 30 tests

Both grow with every Codex feature, because both are the place a new browser →
server → app-server behaviour gets proved. The most recent addition alone put
+429 lines into `adapter_contract.rs`. Neither file is going to stop growing, so
the fix is the structural one: convert each into a **directory module** split by
responsibility, the shape a module that is known to keep growing should have
started in.

This is a **pure mechanical move**. No test is added, removed, renamed, or
re-asserted; no production code changes. The diff must read as "these lines
moved from here to there" so a reviewer can confirm that by inspection instead
of by re-reading every assertion. If a genuine bug in a test surfaces while
moving it, leave it alone and note it in the PR body rather than fixing it in
this diff — a behaviour change hidden inside a 4000-line move is exactly what
this task's shape is designed to prevent.

### Target shape

Use Cargo's directory form of an integration-test target — `tests/<name>/main.rs`
plus sibling submodules — **not** several new top-level `tests/*.rs` files. Each
top-level file is its own test binary and would recompile and re-link the whole
backend (`full_loop` wires the real `delta-server`, the real gateways and an
in-memory `SqliteStore`), and each binary would compile its own copy of the
shared helpers, forcing a blanket `#![allow(dead_code)]` the way
`crates/domain/delta-attribution/tests/support/mod.rs:5` has to. The directory
form keeps one binary per suite, so the helper module is used in full and needs
no such allow.

```
tests/full_loop/
  main.rs        # the file-level //! doc + `mod` declarations, nothing else
  support/       # or support.rs while it stays small
  <behaviour>.rs # one file per behaviour group
tests/adapter_contract/
  main.rs
  support/
  <behaviour>.rs
```

Group by the behaviour under test, following the seams that are already visible
in the current files:

`full_loop.rs` — streaming/reasoning of a plain turn; launch options (including
the delta-owned-field rejection); message metadata (model / branch / cwd);
second-message dispatch; branch-from-selected-text; resume across a restart;
interrupt; permissions (the allow/deny/allow-for-session matrix, the shared
`permission_full_loop` driver, parallel approvals, and the file-change approval
detail); app-server death and the send that follows it; the comms-log stream;
token usage and rate limits.

`adapter_contract.rs` — the shared `agent_contract` cases; session lifecycle
(thread-id adoption, resume, death, orderly close); turn translation (prompt,
turn start/completion, tool items, interrupt); permissions (allow/deny for both
approval kinds plus the shared `permission_case` driver); unsupported server
requests; token usage and rate limits; file-change approval detail (the
`file_change_item` / `change` / `approval_for` / `approval_granting_root` /
`approvals_of` helper family and the eight tests built on it).

These groupings are a starting point, not a contract — merge or split them where
the code says otherwise, as long as no resulting file is back in four-digit line
counts.

### Where things go

- `support` holds only what **more than one** submodule uses: the `ScenarioGuard`
  / `GitRepoGuard` / `DbGuard` fixtures, `build_app` / `build_app_with`, the
  `post_json` / `get` / `json_response` request helpers, the drain helpers, and
  the shared timeout/reply constants.
- A helper used by exactly one behaviour group moves **into that group's file**,
  next to its only caller — the scenario builders and step fragments in
  particular. Do not sweep every free function into `support` on the way past;
  a `support` module that everything reaches into is the same unbounded file
  under a new name.
- Keep visibility as tight as the split allows (`pub(crate)` / `pub(super)`
  rather than blanket `pub`), and do not paper over an unused item with an
  `#[allow(dead_code)]` — in a single-binary target an unused helper means the
  helper is misplaced, not that the lint is wrong.
- `main.rs` carries the existing file-level `//!` documentation (updated only
  where it describes the file layout itself) and the `mod` declarations. No test
  bodies, no helpers.

There is a direct precedent for the mechanics in this repo's frontend: the
`ThreadTimelineOverlay` suite (7,219 lines) was moved to a shared
`.testkit.tsx` fixture plus eight behaviour-named files with all 108 tests
preserved verbatim. Do the same thing here.

## Acceptance criteria

### Automated (pipeline-verified)

Each item below is backed by a gate appended to `check_command` after the
canonical `make check`; the gates are additions to the canonical gate, never a
hand-copy of it.

- [x] `backend/crates/apps/fake-codex/tests/full_loop.rs` and
      `tests/adapter_contract.rs` no longer exist as single files; the two
      suites are directory-module targets rooted at `tests/full_loop/main.rs`
      and `tests/adapter_contract/main.rs`, and both compile and pass under
      `cargo test`.
- [x] Both suites still run as exactly two test binaries: `tests/` holds
      exactly two top-level `*.rs` targets (`comms_log.rs`, `round_trip.rs`)
      plus the two directories, so the split added no new binary.
- [x] The suites hold the same number of tests as before the split — the
      `--list` count is 22 for `full_loop` and 30 for `adapter_contract` — so
      no test was dropped or silently renamed out of its target.
- [x] The other two suites in the crate (`tests/comms_log.rs`,
      `tests/round_trip.rs`) still compile and pass unchanged.
- [x] No file under `tests/full_loop/` or `tests/adapter_contract/` reaches
      1000 lines.
- [x] No `#[allow(dead_code)]` appears anywhere under `tests/full_loop/` or
      `tests/adapter_contract/` — in a single-binary target an unused helper
      means the helper is misplaced, so the lint must stay unmuted.
- [x] The crate builds clean under `cargo fmt --all -- --check` and
      `cargo clippy --all-targets -- -D warnings`.

### Manual / on-hardware (verified by a human before merge)

- [ ] Reading the diff confirms it is a move: no test body, assertion, or
      scenario JSON differs from the pre-split original. (Doc comments are the
      deliberate exception — five prose references that were valid inside a
      single file no longer resolved across module boundaries and were
      corrected; they are listed in the PR body.) This is the reviewer's
      judgement call and is the reason the whole change was scoped as a pure
      move.
- [ ] Each new file's name says which behaviour it covers, so a reader looking
      for one loop can pick the file without opening several.

## Out of scope

- `tests/comms_log.rs` (579 lines) and `tests/round_trip.rs` (200 lines) — both
  are within a readable size and are not part of this split.
- The sibling `fake-claude` crate's `tests/full_loop.rs`. It has the same
  pressure and should get the same treatment, but mixing two crates' moves into
  one diff destroys the "this is just a move" reading that makes this reviewable.
- Any change to what the tests assert, to the scenario JSON the fake replays, or
  to production code under `crates/gateway/codex-agent/`.
