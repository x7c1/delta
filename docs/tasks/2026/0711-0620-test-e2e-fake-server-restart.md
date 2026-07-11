---
status: completed
pipeline_phase: null
plan: null
base_ref: null
retries_remaining: 1
check_command: "make check && make e2e-fake && ! grep -qE 'SERVER_PID' scripts/e2e-fake.sh"
assignee: null
branch: task/0711-0620-test-e2e-fake-server-restart
created_at: 2026-07-11T06:20:09Z
updated_at: 2026-07-11T07:53:15Z
---

# test(e2e-fake): own the server lifecycle from Playwright and cover server restart

## Overview

The server-restart semantics shipped in delta#222 — the boot sweep that
turns orphaned `dispatched` sends back into `queued` rows with
`restored_at` set (`delta_bootstrap::build` →
`SessionStore::restore_all_dispatched`), the "Restored after restart"
badge with the explicit Send/Cancel row
(`frontend/packages/apps/web/src/features/composer/PendingQueue.tsx`),
and the `POST /api/sends/{id}/release` endpoint — have unit coverage but
no e2e coverage. The e2e-fake harness cannot express a scenario that
crosses a server-process death, because the server lifecycle is owned
by `scripts/e2e-fake.sh` (bash spawns `delta-server` as a background
job, keeps `SERVER_PID` / `RUN_DIR` / the tmux socket name as shell
locals, and exports only two port numbers to Playwright). A spec has no
handle to kill the server and no configuration to relaunch it with.
Process death and rebirth are now part of the behaviour under test, so
the test process must own the server lifecycle.

Restructure the harness and add the missing spec:

1. **Move server ownership into a worker-scoped Playwright fixture**
   (new `frontend/packages/apps/web/e2e-fake/support/server.ts`, wired
   into the specs via a fixtures module). Use a worker-scoped fixture,
   NOT `globalSetup`: `globalSetup` runs in a separate process and
   cannot hand a live child-process handle to specs, while a
   worker-scoped fixture (the suite already runs `workers: 1`,
   `fullyParallel: false` per `playwright.fake.config.ts`) holds the
   handle in the worker process, and a worker crash automatically
   re-runs the fixture and reboots the server. The fixture owns
   everything `scripts/e2e-fake.sh` owns today: a per-run temp dir
   (SQLite `delta.db` via `DELTA_DB_PATH`, `workdir/`, `transcripts/`,
   the `claude-bin.sh` wrapper pinning `FAKE_CLAUDE_SCENARIO_DIR` /
   `FAKE_CLAUDE_TRANSCRIPT_DIR` and exec-ing the `fake-claude` binary),
   a per-run tmux socket (`DELTA_TMUX_SOCKET`), the server env
   (`DELTA_PORT`, `DELTA_CLAUDE_BIN`, `DELTA_LAUNCH_DEADLINE_MS`,
   `DELTA_PERMISSION_DECISION_TIMEOUT_MS`, `RUST_LOG` — see
   `config_from_env` in `backend/crates/apps/delta-server/src/main.rs`
   for the full set the bash script populates today), the spawn, the
   `GET /health` readiness poll, and the teardown (kill server, kill
   the tmux server, remove the temp dir).
2. **Shrink `scripts/e2e-fake.sh` to a thin wrapper**: build the
   binaries (`cargo build -p delta-server -p fake-claude`) and invoke
   the Playwright suite. No server spawn, no PID bookkeeping, no
   health poll, no RUN_DIR — a single boot implementation lives in
   Node so the two cannot drift.
3. **Sweep stale runs at fixture startup.** The bash EXIT trap was
   robust against interruption; a Node teardown is not guaranteed to
   run when the Playwright process dies hard (SIGKILL, Ctrl-C storms),
   which would leak tmux servers and temp dirs. Compensate at startup:
   before booting, detect and kill leftover tmux servers from previous
   e2e-fake runs (keep a recognisable socket-name prefix) and remove
   their temp dirs. Leaks must be bounded to one crashed run, cleaned
   by the next run.
4. **Preserve failure artifacts across generations.** The CI job
   (`.github/workflows/ci.yml`, `e2e-fake` job) uploads the preserved
   `server.log` on failure. With restarts, one run can produce several
   server generations — log to `server.log`, `server.2.log`, … (append
   or per-generation files), preserve ALL generations on failure, and
   update the CI upload path to match.
5. **Expose `restartServer()` from the fixture**: SIGKILL the server
   child (hard death — the production incident was not a graceful
   shutdown), then relaunch against the SAME `DELTA_DB_PATH`, tmux
   socket, and claude wrapper, and poll `/health` until ready. The
   relaunched generation logs to the next log file.
6. **Author a scenario that leaves a `dispatched` row at kill time**
   (new `e2e-fake/scenarios/*.json`). The zombie shape is: the send was
   dispatched to the pane but its transcript echo never arrived, so the
   DB row stays `dispatched`. Model it on the existing hold-open
   scenarios (`ws-reconnect-busy.json`, `interrupt-hold.json`) and
   verify the precondition over REST (`support/rest.ts` `fetchSends`)
   before killing. Note `fake-claude` already fully supports
   `--resume <id>` with transcript replay
   (`backend/crates/apps/fake-claude/src/args.rs`, `run.rs`), which the
   post-restart reopen path requires.
7. **Add `e2e-fake/server-restart.spec.ts`** covering the full saga:
   - start a session and get a send into `dispatched` with its echo
     swallowed (assert the row state over REST);
   - `restartServer()`;
   - the client reconnects (reuse the `connection-indicator` /
     `data-connection="open"` wait from `ws-reconnect.spec.ts`) and the
     session reads as closed (turn state is runtime-only and rebuilds
     `Idle`; see the module docs in
     `backend/crates/domain/delta-usecase/src/turn.rs`);
   - the swallowed send is NOT auto-resent: it shows as queued with the
     "Restored after restart" badge and the explicit Send/Cancel row;
   - pressing Send releases it (`POST /api/sends/{id}/release`), the
     session reopens through a fresh pane (`claude --resume`, a new
     pane token — surviving panes are never re-attached, see
     `lifecycle/open_session.rs` and `mint_free_token.rs`), and the
     message is delivered and the turn completes.
   - The spec must leave the (relaunched) server healthy so later specs
     in the shared serial suite are unaffected.

While the server is down, REST helpers in `support/rest.ts` throw on
non-ok responses; the restart window needs a poll-until-healthy probe
(e.g. `expect.poll` against `/health`) rather than the fault-injection
helpers in `support/liveSocket.ts`, which only fake a socket drop while
the server stays alive.

Also update the e2e-fake section of `docs/guides/development.md` to
describe the new ownership split (bash builds, the fixture boots).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `e2e-fake/server-restart.spec.ts` passes under `make e2e-fake`:
      with a `dispatched` send row present (asserted over REST), the
      server is SIGKILLed and relaunched against the same DB and tmux
      socket, the UI reconnects, the send surfaces as restored
      (badge + explicit Send/Cancel, no auto-resend), and an explicit
      Send releases it through a freshly resumed pane to completion.
- [x] The entire pre-existing e2e-fake suite passes with the server
      booted and torn down by the worker-scoped fixture instead of
      `scripts/e2e-fake.sh` (`make e2e-fake` is part of
      `check_command`).
- [x] `scripts/e2e-fake.sh` no longer spawns or tracks the server:
      `! grep -qE 'SERVER_PID' scripts/e2e-fake.sh` (appended to
      `check_command`).

### Manual / on-hardware (verified by a human before merge)

- [x] Kill a run mid-suite (Ctrl-C and/or `kill -9` the Playwright
      process), then start a new run: the startup sweep removes the
      leaked tmux server and temp dir, and no `delta-e2e` tmux servers
      accumulate across repeated interrupted runs.
      (Verified by killing playwright mid-suite; the first attempt
      exposed a real gap — the leaked Vite webServer blocked the next
      run on the port check before the sweep could run — fixed by
      adopting the existing server locally and also sweeping the
      per-socket tmux conf files; the rerun then reclaimed every leak
      and passed 27/27.)
- [x] Force a spec failure after a restart (temporarily break an
      assertion in `server-restart.spec.ts`): every server log
      generation (`server.log`, `server.2.log`, …) is preserved
      locally, and the paths match what the updated
      `.github/workflows/ci.yml` upload step collects.
      (Verified by breaking the restored-badge assertion: both
      generations landed under `test-results/e2e-fake/`, covered by
      the workflow's `test-results/` upload glob.)

## Out of scope

- Per-spec isolated server instances (each spec on its own port) — the
  fixture makes this a natural future extension, but this task keeps
  the shared serial-suite model.
- Any change to the product-side restore/release code
  (`restore_all_dispatched`, the release endpoint, `PendingQueue.tsx`)
  — this task only adds coverage for it.
- An invariant watchdog inside the product, and a nightly suite driven
  by the real `claude` CLI — tracked separately.
- Porting the other stuck-send scenarios (auto-compact, ESC cancel) to
  restart specs — candidates once this harness step exists.
