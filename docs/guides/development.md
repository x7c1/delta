# Development

How to build, test, lint, and run Delta locally. Delta has two parts: a Rust
backend (`backend/`) and a TypeScript frontend (`frontend/`). Run each part's
commands from that part's directory.

## Backend (`backend/`)

Quality gate — run after changing backend code:

```bash
cd backend
cargo build && cargo test && cargo clippy --all-targets -- -D warnings
```

Run the server:

```bash
cargo run -p delta-server
```

It listens on `127.0.0.1` only (loopback). Configuration comes from environment
variables, all with local-friendly defaults:

| Variable | Default | Purpose |
|----------|---------|---------|
| `DELTA_PORT` | `7878` | TCP port |
| `DELTA_DB_PATH` | `delta.db` | SQLite overlay file |
| `DELTA_TMUX_SESSION` | `delta` | tmux session name; the driven pane is `<session>:0.0` |
| `DELTA_SESSION_WORKDIR` | `.tmp/session` | working directory the `claude` session runs in |

The server owns the `claude` session lifecycle: it boots fine with no tmux
session present and lazily creates one (running `claude` in the working
directory, with Claude Code hooks pointed back at it) the first time the browser
calls `POST /api/session`. Authentication is assumed — the server relies on a
cached Claude Code token (or `CLAUDE_CODE_OAUTH_TOKEN`) and never runs
interactive OAuth.

## Frontend (`frontend/`)

All `pnpm` commands run from `frontend/` (the workspace root). pnpm is provided
by corepack from the `packageManager` field — run `corepack enable` once if pnpm
is not on your PATH.

Install:

```bash
cd frontend
pnpm install
```

Quality gate — run after changing frontend code (`lint` is ESLint +
dependency-cruiser):

```bash
pnpm -r build && pnpm -r typecheck && pnpm -r test && pnpm -r lint
```

### Run the UI against mocks (no backend needed)

MSW mocks the REST API and a fake event source replays the WebSocket stream, so
the full UI runs without the backend:

```bash
pnpm -r build                                   # build workspace libs first
VITE_API_MOCK=1 pnpm --filter @delta/web dev    # → http://localhost:5173
```

### End-to-end UI tests (headless, mock mode)

A headless Playwright suite drives the real browser DOM against mock mode and
asserts functional, structural behavior (message send, branch drill-in, the
pending/running indicator lifecycle, layout restore after reload, terminal
resize). It lives in `@delta/web` (`packages/apps/web/e2e/`), separate from the
vitest unit tests.

Build the workspace libraries first (the dev server resolves them from built
output), install the browser once, then run the suite — Playwright starts the
mock-mode dev server itself:

```bash
pnpm -r build
pnpm --filter @delta/web exec playwright install --with-deps chromium
pnpm --filter @delta/web e2e
```

The suite puts the fake event source under manual control (no auto-replay) and
feeds events explicitly, so every run is fast and deterministic. If the bundled
Chromium does not target your OS, run against a locally installed Google Chrome
instead: `E2E_CHROME_CHANNEL=chrome pnpm --filter @delta/web e2e`.

### Run the UI against the real backend

Start the server (see Backend), then:

```bash
pnpm --filter @delta/web dev
```

Vite proxies `/api`, `/ws`, and `/pty` to `127.0.0.1:7878` (the server's default
port — keep them in sync if you set `DELTA_PORT`).

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

Opening the browser is the only manual step. `scripts/dev.sh` starts both the
server and the frontend dev server; the server then starts the `claude` session
lazily the first time the UI loads, so there is nothing else to launch.

### Prerequisites

- `tmux` on your PATH (it hosts the `claude` session the server creates).
- An authenticated Claude Code (`claude`). Authentication is assumed — the
  server relies on a cached token (or `CLAUDE_CODE_OAUTH_TOKEN`) and does not run
  interactive OAuth. If you have not logged in yet, run `claude` once on its own.
- The Rust toolchain (`cargo`) and pnpm (via `corepack enable`).

### Launch

```bash
scripts/dev.sh           # default session workdir: .tmp/session
scripts/dev.sh ~/scratch # or pass your own working directory for claude
```

`scripts/dev.sh`:

1. Starts `delta-server` (`DELTA_PORT=7878`), passing the session working
   directory. The server owns the `claude` session: it lazily creates the tmux
   session and writes `<workdir>/.claude/settings.json` (so the hooks point at
   `http://127.0.0.1:7878/hooks/...`) the first time the browser asks for it.
2. Installs and builds the frontend workspace libraries, then starts the web dev
   server against the real backend (port 5173).

Both run as managed background processes, logging to `.tmp/` (a stable
`delta-server.log` / `delta-frontend.log` symlink points at the latest run).

Then open <http://localhost:5173>. On load the UI calls `POST /api/session`,
which brings the `claude` session up if it is not already running.

### First run / answering prompts

If `claude` is not yet authenticated, the session will not become usable and the
UI shows an explicit error. Run `claude` once on its own to finish login, or
attach to the pane to log in and to answer permission prompts as they appear:

```bash
tmux attach -t delta     # detach again with Ctrl-b then d
```

### Happy-path check

Type a message in the browser. It is dispatched into the tmux pane via
`send-keys`; `claude`'s reply is ingested from the transcript and surfaces in
the browser. When a tool needs permission, answer it in the embedded terminal or
in the TUI (`tmux attach -t delta`).

### Shut down

```bash
scripts/dev.sh --down    # or: scripts/stop.sh
```

This stops `delta-server`, the frontend dev server (port 5173), and the `delta`
tmux session.
