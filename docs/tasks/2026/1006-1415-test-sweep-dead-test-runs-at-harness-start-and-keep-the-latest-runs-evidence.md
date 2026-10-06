---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && test -x scripts/sweep-test-residue.sh && test -f scripts/tests/sweep-test-residue.test.sh && git grep -q 'sweep-test-residue-test' -- Makefile && git grep -q 'sweep-test-residue' -- frontend/packages/apps/web/e2e-fake/support/globalSetup.ts && ! git grep -q 'fs.rmSync(runDir' -- frontend/packages/apps/web/e2e-fake/support/server.ts && ! git grep -q 'rm -rf \"\\$RUN_DIR\"' -- scripts/e2e-real-claude.sh"
assignee: null
branch: task/1006-1415-test-sweep-dead-test-runs-at-harness-start-and-keep-the-latest-runs-evidence
created_at: 2026-10-06T05:16:44Z
updated_at: 2026-10-06T05:58:20Z
---

# test: sweep dead test runs at harness start and keep the latest run's evidence

## Overview

Every test harness that boots a real server leaves something behind, and
each one cleans up differently. Since the server moved its files into one
data directory, a test run's files all sit in one run directory and only
the tmux socket lives outside it, so the residue has two shapes: run
directories and tmux sockets. Today:

- **e2e-fake** (`frontend/packages/apps/web/e2e-fake/support/server.ts`):
  `sweepStaleRuns()` runs from `bootServer()` — once per Playwright
  *worker*, not once per invocation — and kills every `delta-e2e-fake-*`
  socket and removes every `delta-e2e-fake.*` run directory it finds
  without checking whether the owning run is still alive. With `workers: 1`
  and the fixed backend port nothing collides today, but a second worker or
  a run from another checkout would have its server killed from under it.
  `teardown()` then deletes the run directory, so the most recent run's
  database and server log — the evidence a failure investigation needs —
  are gone before anyone reads them. `globalSetup.ts` already wipes the
  artifact directory once per invocation and explains why that cannot live
  in the per-worker fixture; the sweep belongs beside it.
- **real-claude smoke** (`scripts/e2e-real-claude.sh`): no sweep at start;
  `teardown()` removes `$RUN_DIR` with the server log in it.
- **fake-claude `full_loop`** (`backend/crates/apps/fake-claude/tests/full_loop.rs`)
  and the **real-claude canary**
  (`backend/crates/apps/delta-server/tests/real_claude_canary.rs`): their
  run directories are `tempfile` directories cleaned on drop, so a
  crash (panic before the kill, SIGKILL) leaves only the tmux server and
  its socket `delta-fake-test-<pid>` / `delta-canary-<name>-<pid>`.
- In every context, `tmux kill-server` leaves the socket file in
  `${TMUX_TMPDIR:-/tmp}/tmux-<uid>/` (checked on tmux 3.6a), so even a
  clean run leaves one file per run.

The rule this task installs, once, for every harness: **at the start of a
run, clean up the dead runs of the same context, keep the newest dead
run's directory as evidence, and always remove dead tmux servers and
their socket files. A run whose owner is alive is never touched.**

### Change

- **One script, `scripts/sweep-test-residue.sh <context>`**, called by
  every harness, so the rule is written once. Contexts and what names
  them:

  | context | socket name | run directory |
  |---|---|---|
  | `e2e-fake` | `delta-e2e-fake-<pid>` | `$TMPDIR/delta-e2e-fake.*` |
  | `e2e-real` | `delta-e2e-real-<pid>` | `$TMPDIR/delta-e2e-real.*` |
  | `fake-test` | `delta-fake-test-<pid>` | — (tempfile, cleaned on drop) |
  | `canary` | `delta-canary-<name>-<pid>` | — (under `CARGO_TARGET_TMPDIR`) |

  - A socket's owner is the pid at the end of its name; a run directory's
    owner is read from an `owner.pid` file the harness writes into it at
    creation (add that write to `server.ts` and `e2e-real-claude.sh`). A
    directory without `owner.pid` counts as dead (pre-change leftovers).
  - Owner liveness is `kill -0 <pid>`. Pid reuse can only make the sweep
    skip something, never kill a live run.
  - Dead socket: `tmux -L <name> kill-server` (ignore failure — the server
    may already be gone), then unlink the socket file.
  - Dead run directories: sort by modification time, keep the newest,
    remove the rest. The kept one is evidence, not a running server, so
    its socket is still killed.
  - Only the prefixes above are ever matched. `io.github.x7c1.delta*`,
    `default` and anything else are never touched. The socket directory is
    `${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)`; the temp directory is `$TMPDIR`
    (`/tmp` when unset). Both are overridable for the test below.
  - Print one line per action; exit 0 even when nothing is found.
- **Harnesses call it at start and stop deleting the latest run**:
  - `server.ts`: delete `sweepStaleRuns()`; `globalSetup.ts` runs
    `scripts/sweep-test-residue.sh e2e-fake` (via `execFileSync`) once per
    invocation next to the artifact wipe; `bootServer()` writes
    `owner.pid` (`process.pid`) into the run directory; `teardown()` keeps
    killing the server process and the tmux server but no longer
    `rmSync`s the run directory. Update the module doc that describes the
    teardown.
  - `scripts/e2e-real-claude.sh`: after the lock is taken, run the script
    with `e2e-real`; write `owner.pid` (`$$`) into `$RUN_DIR`; stop
    `rm -rf "$RUN_DIR"` in `teardown()` (keep removing `$WORKDIR` and the
    transcript as today); keep `kill-server`.
  - `full_loop.rs` and `real_claude_canary.rs`: at the start of the test,
    run the script with `fake-test` / `canary` through `std::process::Command`
    (path relative to `CARGO_MANIFEST_DIR`); a missing `tmux` already
    skips these tests, so the sweep runs only when tmux is present.
- **Test the script, in `make check`**:
  `scripts/tests/sweep-test-residue.test.sh`, in the style of
  `scripts/tests/e2e-real-gate.test.sh`: point `TMUX_TMPDIR` and `TMPDIR`
  at a `mktemp -d`, plant a dead-owner socket file and a live-owner
  (`$$`) socket file for `e2e-fake`, an `io.github.x7c1.delta.dev` socket,
  an older and a newer dead run directory (with `owner.pid` of a dead
  pid) and one without `owner.pid`, and a live-owner run directory; run
  the script; assert that only the dead, older, prefixed entries are gone,
  that the newest dead directory, the live entries and the dev socket
  survive, and that the dev socket was never passed to `tmux`. Stale
  socket files can be plain files (the `kill-server` failure path).
  Register it as `sweep-test-residue-test` in `CHECK_STEPS` like
  `e2e-real-gate-test`.
- **Docs**: `docs/guides/development/e2e.md` (the evidence-retention
  section, lines ~97–110) and `docs/guides/development/canary.md`: one
  paragraph on the rule — what is swept, what is kept, that a run's
  directory survives until the next run of the same context.

### Out of scope

The production and dev sockets, and a developer's live server, are never
candidates; this task adds no cleanup for them.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `scripts/sweep-test-residue.sh` exists and is executable, its test
      exists and is a `CHECK_STEPS` entry, `globalSetup.ts` calls the
      sweep, and neither `server.ts`'s teardown nor the smoke script's
      teardown deletes the run directory any more (gates in `check_command`).
- [x] The script test proves: dead-owner socket killed and unlinked;
      live-owner socket untouched; dev socket untouched and never passed
      to `tmux`; of two dead run directories only the newer survives; a
      directory without `owner.pid` counts as dead; a live-owner directory
      survives.
- [x] `make check` passes, including `check-e2e-fake` booting through the
      moved sweep.

### Before merge (verified outside the check command)

- [x] On the development machine, run `make e2e-fake` three times: after
      the second run the first and second runs' `delta-e2e-fake.*`
      directories remain in `$TMPDIR` (the sweep keeps the newest dead run,
      and the live run adds its own); after the third run exactly two
      remain, the second and third, and the first is gone. After each run
      the only `delta-e2e-fake-*` file in the tmux socket directory is the
      finished run's own socket (which `kill-server` leaves behind), and the
      next run's sweep removes it. A tmux server on another socket running
      alongside (the `make dev` one, or a scratch one) is still alive
      afterwards.
