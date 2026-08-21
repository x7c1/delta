# Development

## Overview

How to build, test, lint, and run Delta locally. Delta has two parts: a Rust
backend (`backend/`) and a TypeScript frontend (`frontend/`).

The unified entry point is `make`, run from the repo root: it wraps the
per-part commands and the `scripts/dev.sh` loop so you do not have to `cd` into
each part or remember the underlying `cargo`/`pnpm` invocations. Run `make help`
for the full target list. These guides name the relevant `make` target for each
task and keep the underlying commands only where there is no target (one-time
setup and env-overridden runs).

This file covers the platform baseline and the day-to-day commands for each
part. The larger workflows live in their own files:

- **[e2e.md](e2e.md)** — the headless Playwright suites: mock mode (`make e2e`)
  and fake mode against the real backend (`make e2e-fake`).
- **[canary.md](canary.md)** — the real-agent canary suites (`make e2e-real`,
  `make e2e-real-codex`), the drift runbook, and the automatic canary trigger.
- **[local-run.md](local-run.md)** — running the whole thing locally with
  `make dev`.
- **[release.md](../release.md)** — the release flow and its supporting
  automation.

## Supported platforms

Delta is officially supported on **Linux** and **macOS** for both development
and runtime. The dev scripts (`scripts/dev.sh`, `scripts/stop.sh`,
`scripts/reset.sh`) and the `Makefile` target the common shell baseline shared
by both — see "Portability conventions" below.

### Prerequisites by platform

| Platform | Prerequisites |
|----------|---------------|
| Linux | `tmux`, `lsof`, GNU `make`, `bash` — install via the system package manager (e.g. `apt install tmux lsof make`). |
| macOS | `tmux` via Homebrew (`brew install tmux`). `lsof`, `make` (GNU make 3.81), `awk`, `bash` 3.2, `date`, and `pkill` ship with the system. Installing the Xcode Command Line Tools (`xcode-select --install`) is the standard way to get `make`. |

In addition, both platforms need the Rust toolchain (`cargo`) and pnpm (via
`corepack enable`), plus the agent CLIs you plan to drive: an authenticated
Claude Code (`claude`) and/or Codex (`codex`) — see
[local-run.md](local-run.md).

### `lsof` is assumed

`scripts/dev.sh`'s `port_in_use` / `kill_port` helpers prefer `lsof` for port
probing and teardown. They fall back to `ss` or `fuser` (Linux-only) when
`lsof` is missing, gated by `command -v`, but a host without `lsof` is not a
supported configuration. macOS ships `lsof` by default, so this is only a
concern on stripped-down Linux images.

### Portability conventions

The dev scripts are written against a shell baseline that runs unmodified on
both platforms. Keep changes within this baseline so macOS support does not
regress:

- Stick to **bash 3.2 / POSIX-compatible** idioms — macOS still ships bash 3.2
  by default, and the scripts are run under that version.
- Avoid bash-4-only features: associative arrays (`declare -A`),
  `mapfile` / `readarray`, case-modifying expansions (`${var,,}` / `${var^^}`),
  and `&>>` redirection.
- Avoid GNU-only flags and tools: `readlink -f`, GNU-style `sed -i`,
  `date -d`, `grep -P`, GNU `stat`, `nproc`, and reads from `/proc`.
- When a feature genuinely needs a Linux-only tool (e.g. `ss`, `fuser`), gate
  it with `command -v` and provide an `lsof`-based path or a no-op fallback so
  the script still does the right thing on macOS.

## Backend (`backend/`)

Quality gate — `make build`, `make test`, and `make lint` each cover both
parts and stay fast, for the inner loop. `make check` is the pre-PR gate: it
runs the whole thing for both parts at once (build, test, lint, the frontend
typecheck, the generated-bindings freshness check) **plus both Playwright
suites**, so passing it means CI will pass. It needs tmux, because `make
e2e-fake` drives the real backend through one.

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
| `DELTA_WORKTREE_BASE` | `$HOME/.delta/worktrees` | base directory for per-session git worktrees (`<base>/delta-<session-id>`), deliberately outside any repo tree so the worktree does not inherit a surrounding `CLAUDE.md`/settings |
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

### Reading the SQLite schema

The schema has no single file to open: it is built by replaying the migration
ladder in `backend/crates/gateway/delta-sqlite/src/migrations/`, one module per
schema subject. To read the whole thing at once, dump it from a database the
ladder built:

```bash
sqlite3 delta.db .schema
```

That is true by construction — it is the schema delta is actually running
against, including every migration step this particular file has been through.
`sqlite3 delta.db 'PRAGMA user_version'` shows which generation it is stamped
at. For when a change needs a migration step and when it may ask for a reset,
see the [compatibility policy](../compatibility.md).

## Frontend (`frontend/`)

The `frontend/` directory is the pnpm workspace root. pnpm is provided by
corepack from the `packageManager` field — run `corepack enable` once if pnpm is
not on your PATH. Install dependencies once with `pnpm install` from `frontend/`.

The quality gate (build, typecheck, test, and `lint` = ESLint +
dependency-cruiser) is covered by `make check` — which also runs both Playwright
suites — or by the individual `make build` / `make test` / `make lint` targets
when a faster loop is wanted.

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

### Run the UI against the real backend

Start the server (see Backend), then:

```bash
pnpm --filter @delta/web dev
```

Vite proxies `/api`, `/ws`, `/pty`, and `/comms` to `127.0.0.1:7878` by default;
if the server runs elsewhere, start the dev server with the same `DELTA_PORT`
and the proxy follows it.

### Notes

- The web dev server resolves workspace libraries from their built output. After
  editing a library package (`@delta/model`, `@delta/ui-kit`, `@delta/api-client`)
  rebuild it, or run a watch in another terminal:
  `pnpm -r --parallel exec tsc -b --watch`. Editing `@delta/web` sources
  hot-reloads directly.
- `esbuild` and `msw` build scripts are allow-listed in `pnpm-workspace.yaml`
  (`allowBuilds`); pnpm does not run dependency build scripts by default.
