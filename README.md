# Delta

A local browser tool that wraps a single Claude Code session with a thread
navigation layer. It lets you run multiple conversation threads in one session
and move freely between the main line and the branches that grow out of it.

The name comes from a river delta: the way a conversation forks from its main
channel into side branches.

## Status

Pre-1.0. Scaffolding stage — the build and CI come first, concrete features
follow.

## Architecture

- **`backend/`** — Rust workspace. Drives a single Claude Code TUI through
  tmux `send-keys`, reads the JSONL transcript, serves HTTP (Claude Code hooks)
  and WebSocket (browser), and persists a thread overlay in SQLite.
- **`frontend/`** — TypeScript (React + Vite) pnpm workspace. Renders the
  thread navigator, the active-thread transcript (drill-down), the composer,
  and an embedded terminal (xterm.js) for permission prompts.

The server binds to `127.0.0.1` only.

The browser↔server contract (REST, WebSocket, and hook endpoints) is documented
in [docs/guides/api.md](docs/guides/api.md).

## Development

Build, test, lint, and run commands are in
[docs/guides/development.md](docs/guides/development.md).
