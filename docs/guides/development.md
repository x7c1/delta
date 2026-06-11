# Development

How to build, test, lint, and run Delta locally. Delta has two parts: a Rust
backend (`backend/`) and a TypeScript frontend (`frontend/`).

The unified entry point is `make`, run from the repo root: it wraps the
per-part commands and the `scripts/dev.sh` loop so you do not have to `cd` into
each part or remember the underlying `cargo`/`pnpm` invocations. Run `make help`
for the full target list. This guide names the relevant `make` target for each
task and keeps the underlying commands only where there is no target (one-time
setup and env-overridden runs).

## Backend (`backend/`)

Quality gate — `make build`, `make test`, and `make lint` each cover both
parts; `make check` runs the whole gate (build, test, lint, plus the frontend
typecheck) for both parts at once, which is what to run before opening a PR.

Run the server (from `backend/`):

```bash
cargo run -p delta-server
```

It listens on `127.0.0.1` only (loopback). Configuration comes from environment
variables, all with local-friendly defaults:

| Variable | Default | Purpose |
|----------|---------|---------|
| `DELTA_PORT` | `7878` | TCP port |
| `DELTA_DB_PATH` | `delta.db` | SQLite overlay file |
| `DELTA_SESSION_WORKDIR` | `.tmp/session` | base directory for per-spawn working directories (`<base>/<token>`) |
| `DELTA_TMUX_SOCKET` | `delta` | dedicated tmux socket (`tmux -L <socket>`) for Delta's sessions, isolated from your default tmux server |

The server owns the `claude` session lifecycle: it boots fine with no tmux
session present and spawns nothing on startup or page load. A session is spawned
lazily when first needed — the composer's first Send, a New action, or
`POST /api/sessions`. Each spawn gets its own tmux session, named after a
Delta-minted token (`delta-<n>`), running `claude` in its own working directory
(`<base>/<token>`) with Claude Code hooks pointed back at this server. Naming the
tmux session after a Delta-owned token (never Claude's `session_id`) is what lets
a closed conversation be resumed (`claude --resume <id>`) under a fresh tmux
session without a name collision. Open/closed is in-memory only: after a restart
every persisted conversation is "closed" until it is resumed. Authentication is
assumed — the server relies on a cached Claude Code token (or
`CLAUDE_CODE_OAUTH_TOKEN`) and never runs interactive OAuth.

## Frontend (`frontend/`)

The `frontend/` directory is the pnpm workspace root. pnpm is provided by
corepack from the `packageManager` field — run `corepack enable` once if pnpm is
not on your PATH. Install dependencies once with `pnpm install` from `frontend/`.

The quality gate (build, typecheck, test, and `lint` = ESLint +
dependency-cruiser) is covered by `make check`, or the individual `make build` /
`make test` / `make lint` targets.

### Generated wire bindings (`@delta/wire-gen`)

`frontend/packages/gateway/wire-gen` contains TypeScript generated from the
backend's wire contract (the `delta-wire` crate): the REST request/response
shapes, the `SessionEvent` union, and the `EVENT_KINDS` const. Never edit the
files under `src/generated/` by hand —
change the Rust types and run `make gen` to regenerate, then commit the result.
`make check` (and CI) regenerates and fails on any diff, so stale bindings
cannot land.

### Run the UI against mocks (no backend needed)

MSW mocks the REST API and a fake event source replays the WebSocket stream, so
the full UI runs without the backend — no tmux or `claude` required:

```bash
make mock    # → http://localhost:5173
```

`make mock` builds the workspace libraries first (the dev server resolves them
from built output), then starts the mock-mode dev server with `--force` so the
freshly built libs are re-optimized and served.

### End-to-end UI tests (headless, mock mode)

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

### End-to-end UI tests (headless, fake mode)

A second Playwright suite (`packages/apps/web/e2e-fake/`) drives the real
backend instead of mocks: a live `delta-server` on a temp database, real tmux
panes, real hooks, the real transcript tail, and the real WebSocket/PTY
channels. The only scripted part is the "claude" the server spawns: the
`fake-claude` binary (`backend/crates/apps/fake-claude`), a stand-in that
accepts `claude`'s CLI flags, fires the same HTTP hooks, and writes the same
transcript JSONL — but follows a deterministic scenario script instead of a
model. This is the suite that proves the full loop
(REST → spawn → tmux → hooks → transcript → tail → WS) end to end.

Run it with:

```bash
make e2e-fake
```

`scripts/e2e-fake.sh` builds both binaries, boots the server on a dedicated
port (7899) with a per-run temp database and tmux socket
(`delta-e2e-<pid>`, killed on exit), shortens the launch watchdog via
`DELTA_LAUNCH_DEADLINE_MS`, and lets Playwright start the Vite dev server
(port 5198) proxied to that backend. Nothing it touches collides with
`make dev` or the mock suite. It needs tmux, the Playwright chromium browser
(see above), and built workspace libraries (`make build`).

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

The same fake also backs a backend-only integration test
(`backend/crates/apps/fake-claude/tests/full_loop.rs`, part of `cargo test`)
that proves the loop without a browser; it skips where tmux is missing.

### Run the UI against the real backend

Start the server (see Backend), then:

```bash
pnpm --filter @delta/web dev
```

Vite proxies `/api`, `/ws`, and `/pty` to `127.0.0.1:7878` by default; if the
server runs elsewhere, start the dev server with the same `DELTA_PORT` and the
proxy follows it.

### Notes

- The web dev server resolves workspace libraries from their built output. After
  editing a library package (`@delta/model`, `@delta/ui-kit`, `@delta/api-client`)
  rebuild it, or run a watch in another terminal:
  `pnpm -r --parallel exec tsc -b --watch`. Editing `@delta/web` sources
  hot-reloads directly.
- `esbuild` and `msw` build scripts are allow-listed in `pnpm-workspace.yaml`
  (`allowBuilds`); pnpm does not run dependency build scripts by default.

## Run the whole thing locally

This brings up the full loop end to end: type in the browser, a real `claude`
TUI (running in tmux) receives it via `send-keys`, and its response flows back
through the JSONL transcript and Claude Code's HTTP hooks to the browser.

Opening the browser is the only manual step. `make dev` starts both the
server and the frontend dev server. The server owns the `claude` session
lifecycle but does not spawn anything on startup or on page load: on load the UI
shows the session list (empty on a fresh database), and the first Send from the
composer (or a New action) spawns a session, so there is nothing else to launch.

### Prerequisites

- `tmux` on your PATH (it hosts the `claude` session the server creates).
- An authenticated Claude Code (`claude`). Authentication is assumed — the
  server relies on a cached token (or `CLAUDE_CODE_OAUTH_TOKEN`) and does not run
  interactive OAuth. If you have not logged in yet, run `claude` once on its own.
- The Rust toolchain (`cargo`) and pnpm (via `corepack enable`).

### Launch

```bash
make dev                 # default session workdir: .tmp/session
make dev WORKDIR=~/scratch # or pass your own working directory for claude
```

`make dev` runs `scripts/dev.sh`, which:

1. Starts `delta-server` (`DELTA_PORT=7878`), passing the session working
   directory. The server owns the `claude` session lifecycle: when a session is
   spawned (first Send / New) it creates the tmux session and launches `claude
   --settings <file>` with Delta's rendered session settings (so the hooks point
   at `http://127.0.0.1:7878/hooks/...`); the settings file lives outside the
   working directory, so a real project's own `.claude/settings.json` is never
   touched. Nothing is spawned on startup.
2. Installs and builds the frontend workspace libraries, then starts the web dev
   server against the real backend (port 5173).

Both run as managed background processes, logging to `.tmp/` (a stable
`delta-server.log` / `delta-frontend.log` symlink points at the latest run).

Then open <http://localhost:5173>. On load the UI fetches the session list and
shows it (empty on a fresh database); opening the browser does not spawn
anything. From the composer, the first Send (or a New action) spawns a fresh
`claude` session. Existing sessions show as open or closed: a closed session is
view-only — you can read its history with no process running — and the first
Send to it resumes it (`claude --resume`). After a server restart every prior
session shows as closed until it is resumed via Send.

### First run / answering prompts

If `claude` is not yet authenticated, the first spawn will not become usable and
the UI shows an explicit error. Run `claude` once on its own to finish login, or
attach to a spawned pane to log in and to answer permission prompts as they
appear (each spawn is named `delta-<n>`; the first Send of a run spawns
`delta-1`):

```bash
tmux -L delta attach -t delta-1     # detach again with Ctrl-b then d
```

### Happy-path check

Type a message in the browser. It is dispatched into the tmux pane via
`send-keys`; `claude`'s reply is ingested from the transcript and surfaces in
the browser. When a tool needs permission, answer it in the embedded terminal or
in the TUI (`tmux -L delta attach -t delta-1`).

### Shut down

```bash
make down
```

This stops `delta-server`, the frontend dev server (port 5173), and every
`delta-<n>` tmux session the server spawned. To also delete the SQLite overlay
so the next start recreates an empty schema, run `make reset` instead.
