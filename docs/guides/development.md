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
