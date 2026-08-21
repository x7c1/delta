# End-to-end UI tests

## Overview

Two headless Playwright suites drive the real browser DOM, cheapest first:
**mock mode** (`make e2e`) runs the UI against MSW mocks with no backend at
all, and **fake mode** (`make e2e-fake`) runs the real backend end to end with
scripted stand-ins for the agent CLIs (`claude` and `codex`). Contract
monitoring against the real agent CLIs is a third lane, documented in
[canary.md](canary.md). Day-to-day build and run commands are in
[README.md](README.md).

## Mock mode (`make e2e`)

A headless Playwright suite drives the real browser DOM against mock mode and
asserts functional, structural behavior (message send, branch drill-in, the
pending/running indicator lifecycle, layout restore after reload, terminal
resize). It lives in `@delta/web` (`packages/apps/web/e2e/`), separate from the
vitest unit tests.

Install the browser once:

```bash
pnpm --filter @delta/web exec playwright install --with-deps chromium
```

Build the workspace libraries (`make build`, since the dev server resolves them
from built output), then run the suite with `make e2e` — Playwright starts the
mock-mode dev server itself.

The suite puts the fake event source under manual control (no auto-replay) and
feeds events explicitly, so every run is fast and deterministic.

`make e2e` runs isolated: it pins a dedicated mock-server port (`E2E_PORT=5199`)
so it cannot collide with a dev server on the default 5173, and the suite starts
its own mock-mode build every run (it never reuses an already-running server).
That last part matters — a live `make dev` server (real backend, tmux +
`claude`) is indistinguishable from the suite's own mock build at the port, so
adopting it would drive that **live session, sending real prompts to your real
`claude`**. Hence the suite never reuses by default.

For fast iteration you can reuse an already-running mock server (e.g. `make
mock`) instead of spawning a fresh one per run: set `E2E_REUSE=1` and point the
suite at that server's port — only ever a mock server, never `make dev`:

```bash
E2E_REUSE=1 E2E_PORT=5173 make e2e
```

## Fake mode (`make e2e-fake`)

A second Playwright suite (`packages/apps/web/e2e-fake/`) drives the real
backend instead of mocks: a live `delta-server` on a temp database, real tmux
panes, real hooks, the real transcript tail, and the real WebSocket/PTY
channels. The only scripted part is the agent the server spawns: the
`fake-claude` binary (`backend/crates/apps/fake-claude`), a stand-in that
accepts `claude`'s CLI flags, fires the same HTTP hooks, and writes the same
transcript JSONL — but follows a deterministic scenario script instead of a
model. This is the suite that proves the full loop
(REST → spawn → tmux → hooks → transcript → tail → WS) end to end.

The adapter (terminal-less) path has its own stand-in: `fake-codex`
(`backend/crates/apps/fake-codex`), spawned as the session's `codex` binary,
speaking the real `codex app-server` JSON-RPC over stdio. A spec starts a session
on it with `startNewCodexSession` (the helper picks the provider in the
new-session form). It covers behavior a pane-backed provider cannot even
produce — notably several tool approvals outstanding at once, since Claude's
permission hook blocks its CLI until each dialog is answered.

Run it with:

```bash
make e2e-fake
```

Ownership is split. `scripts/e2e-fake.sh` is a thin wrapper: it only builds
the binaries (`delta-server`, `fake-claude`, `fake-codex`) and invokes the Playwright
suite. The **server lifecycle is owned by a worker-scoped Playwright fixture**
(`packages/apps/web/e2e-fake/support/server.ts`), which runs in the worker
process and holds the child-process handle — which is what makes the
server-restart coverage possible (kill the server, relaunch it against the
same database and tmux socket) and means a worker crash reboots the server.
The fixture owns the per-run temp database and tmux socket
(`delta-e2e-fake-<pid>`, killed on teardown), the scripted-claude and
scripted-codex wrappers, a shortened launch watchdog
(`DELTA_LAUNCH_DEADLINE_MS`), the dedicated backend
port (7899), and the `/health` readiness poll; Playwright starts the Vite dev
server (port 5198) proxied to that backend. A spec that needs a different
server-wide setting (e.g. a shortened echo watchdog, `DELTA_ECHO_DEADLINE_MS`)
passes it to `restart(env)` for its own server generation, and restores the
shared configuration with a bare `restart()` in an `afterEach`. Because a hard
kill (SIGKILL, Ctrl-C) can skip teardown, the fixture also **sweeps at
startup**: it kills any leftover `delta-e2e-fake-*` tmux server and removes any
`delta-e2e-fake.*` temp dir from a crashed run, so leaks are bounded to one
run. Each server generation logs to its own file under
`test-results/e2e-fake/` (`server.log`, `server.2.log`, …), all uploaded by CI
on failure. Nothing the e2e-fake run touches collides with `make dev` or the
mock suite. It needs tmux, the Playwright chromium browser (see above), and
built workspace libraries (`make build`).

**Writing a scenario.** Scenarios are JSON files in
`packages/apps/web/e2e-fake/scenarios/`, executed step by step by the fake:
`await_prompt`, `reply`, `tool_use`, `permission_request`, `tool_result`,
`stop`, `await_interrupt`, `write_queued_command`, `delay`, `hang`, plus
`session_start` timing (`immediate` / `skip` / `{ "delay_ms": N }`) and an
optional `loop`. The full vocabulary is documented in the fake's `scenario`
module (`backend/crates/apps/fake-claude/src/scenario.rs`). A spec selects its
scenario through the **first word of the first prompt it sends**: sending
`"first-send hold then answer"` makes the spawned fake load
`scenarios/first-send.json`. Keep specs structural — assert presence, absence,
and ordering of UI elements, never scripted reply text.

`fake-codex` scenarios live in the same directory but are selected differently:
the fake reads ONE file, named by `FAKE_CODEX_SCENARIO` at process start, so the
server fixture pins a single Codex scenario for the whole run
(`scenarios/codex-parallel-approvals.json`) and every Codex session plays it. Its
step vocabulary (including `await_approvals`, which suspends a turn until every
emitted approval has been answered) is documented in
`backend/crates/apps/fake-codex/src/scenario.rs`.

Both fakes also back backend-only integration tests (part of `cargo test`) that
prove the same loops without a browser:
`backend/crates/apps/fake-claude/tests/full_loop.rs`, which skips where tmux is
missing, and `backend/crates/apps/fake-codex/tests/full_loop/`, where the
adapter path's turn controls — the parallel approval fan-out included — are
pinned.
