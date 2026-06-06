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
| `DELTA_TMUX_PANE` | `delta:0.0` | tmux pane to drive via `send-keys` |

To be useful the server needs a tmux session running `claude` and Claude Code
hooks pointed at it; that wiring is not part of running the server alone.

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

## Run the whole thing locally (walking skeleton)

This brings up the full loop end to end: type in the browser, a real `claude`
TUI (running in tmux) receives it via `send-keys`, and its response flows back
through the JSONL transcript and Claude Code's HTTP hooks to the browser. This
is the minimal wiring — permission prompts are answered in the TUI, and
robustness and edge-cases come later.

### Prerequisites

- `tmux` on your PATH (it hosts the `claude` session).
- An authenticated Claude Code (`claude`). Run `claude` once on its own to
  complete OAuth login if you have not already.
- The Rust toolchain (`cargo`) and pnpm (via `corepack enable`).

### Launch

```bash
scripts/dev.sh           # default session workdir: .tmp/session
scripts/dev.sh ~/scratch # or pass your own working directory for claude
```

`scripts/dev.sh`:

1. Creates the session working directory and copies
   `scripts/claude-settings.json` to `<workdir>/.claude/settings.json`, so the
   session's native HTTP hooks point at the local server
   (`http://127.0.0.1:7878/hooks/...`).
2. Starts a tmux session named `delta` with `claude` in pane `delta:0.0`.
3. Starts `delta-server` with `DELTA_TMUX_PANE=delta:0.0` and `DELTA_PORT=7878`
   (logging to `.tmp/delta-server.log`).
4. Prints the command to start the frontend against the real backend.

Then, in a second terminal, start the UI against the real backend:

```bash
cd frontend
pnpm install            # first run only
pnpm -r build           # build workspace libs first
pnpm --filter @delta/web dev
```

Open <http://localhost:5173>.

### First run: attach to the TUI

On the first run you usually need to attach to the `claude` pane to finish OAuth
login and to answer permission prompts as they appear:

```bash
tmux attach -t delta     # detach again with Ctrl-b then d
```

### Happy-path check

Type a message in the browser. It is dispatched into the tmux pane via
`send-keys`; `claude`'s reply is ingested from the transcript and surfaces in
the browser. When a tool needs permission, switch to the TUI
(`tmux attach -t delta`) and answer the prompt there.

### Shut down

```bash
scripts/dev.sh --down    # or: scripts/stop.sh
```

This stops `delta-server` and kills the `delta` tmux session. Stop the frontend
dev server with Ctrl-C in its terminal.
