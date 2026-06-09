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

Pre-1.0 and under active development. The core works end to end: multi-session
management (spawn / view / resume), the thread-navigation layer (branch and
return within a session), and a conversation viewer whose session list and
sub-thread trees are cursor-paginated and DOM-virtualized so they stay
responsive as history grows. An embedded terminal handles permission prompts.
Features are still being shaped as the product takes form.

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

Build, test, lint, and run commands are in
[docs/guides/development.md](docs/guides/development.md).
