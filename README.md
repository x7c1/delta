# Delta

A local browser tool that wraps Claude Code with a thread-navigation layer and
a readable conversation viewer. Within one session you can branch off a past
message into a side thread, dig in, and return to the main line without losing
your place. It also lays out all of your sessions side by side, so you can
browse and resume any past conversation far more comfortably than scrolling a
terminal.

The name comes from a river delta: the way a conversation forks from its main
channel into side branches.

## Status

v0.1.0 is the first tagged release and is alpha quality. Delta adopts
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) from this version
onward, but while it stays on `0.x` **no compatibility is guaranteed**: the
SQLite schema, the browser↔server wire contract, and the supported Claude CLI
version range may all change in any `0.x` bump. Supported platform is **Linux
only** for now.

## Install

Delta is distributed as source only — there are no prebuilt binaries yet.

```
git clone https://github.com/x7c1/delta.git
cd delta
make dev
```

`make dev` runs the local development loop (backend + frontend). See
[docs/guides/development.md](docs/guides/development.md) for prerequisites
(tmux, claude, cargo, pnpm) and the rest of the workflow.

## Architecture

- **`backend/`** — Rust workspace. Manages multiple Claude Code sessions, each
  driven through its own tmux pane via `send-keys`. It reads the JSONL
  transcripts, serves HTTP (Claude Code hooks) and WebSocket (browser), and
  persists a thread overlay in SQLite.
- **`frontend/`** — TypeScript (React + Vite) pnpm workspace. Renders the
  session list with its expanded sub-thread trees, the active-thread transcript
  (drill-down), the composer, and an embedded terminal (xterm.js) for
  permission prompts.

The server binds to `127.0.0.1` only.

The browser↔server contract (REST, WebSocket, and hook endpoints) is documented
in [docs/guides/api.md](docs/guides/api.md).

## Development

Common tasks run through `make` from the repo root — `make help` lists the
targets (`dev`, `mock`, `build`, `test`, `lint`, `check`, `e2e`, …). Build,
test, lint, and run details are in
[docs/guides/development.md](docs/guides/development.md).
