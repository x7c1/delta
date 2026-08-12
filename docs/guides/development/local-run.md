# Run the whole thing locally

## Overview

This brings up the full loop end to end: type in the browser, a real `claude`
TUI (running in tmux) receives it via `send-keys`, and its response flows back
through the JSONL transcript and Claude Code's HTTP hooks to the browser.

Opening the browser is the only manual step. `make dev` starts both the
server and the frontend dev server. The server owns the `claude` session
lifecycle but does not spawn anything on startup or on page load: on load the UI
shows the session list (empty on a fresh database), and the first Send from the
composer (or a New action) spawns a session, so there is nothing else to launch.

## Prerequisites

- `tmux` on your PATH (it hosts the `claude` session the server creates).
- An authenticated Claude Code (`claude`). Authentication is assumed — the
  server relies on a cached token (or `CLAUDE_CODE_OAUTH_TOKEN`) and does not run
  interactive OAuth. If you have not logged in yet, run `claude` once on its own.
- To drive Codex sessions, an authenticated Codex CLI (`codex`) — the server
  spawns `codex app-server` on demand. Optional if you only use Claude Code.
- The Rust toolchain (`cargo`) and pnpm (via `corepack enable`).

## Launch

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
`make dev` does not return until both ports are actually listening, so a
completed command means the UI is openable right away — the frontend's
install+build finishes binding port 5173 before control returns. If either
process dies or is not listening within its budget, `make dev` tears the loop
back down and exits non-zero after printing the tail of the relevant log (the
budgets are overridable via `DELTA_DEV_SERVER_TIMEOUT` / `DELTA_DEV_FRONTEND_TIMEOUT`).

Then open <http://localhost:5173>. On load the UI fetches the session list and
shows it (empty on a fresh database); opening the browser does not spawn
anything. From the composer, the first Send (or a New action) spawns a fresh
`claude` session. Existing sessions show as open or closed: a closed session is
view-only — you can read its history with no process running — and the first
Send to it resumes it (`claude --resume`). After a server restart every prior
session shows as closed until it is resumed via Send.

## First run / answering prompts

If `claude` is not yet authenticated, the first spawn will not become usable and
the UI shows an explicit error. Run `claude` once on its own to finish login, or
attach to a spawned pane to log in and to answer permission prompts as they
appear (each spawn is named `delta-<n>`; the first Send of a run spawns
`delta-1`):

```bash
tmux -L delta attach -t delta-1     # detach again with Ctrl-b then d
```

## Happy-path check

Type a message in the browser. It is dispatched into the tmux pane via
`send-keys`; `claude`'s reply is ingested from the transcript and surfaces in
the browser. When a tool needs permission, answer it in the embedded terminal or
in the TUI (`tmux -L delta attach -t delta-1`).

## Shut down

```bash
make down
```

This stops `delta-server`, the frontend dev server (port 5173), and every
`delta-<n>` tmux session the server spawned. To also delete the SQLite overlay
so the next start recreates an empty schema, run `make reset` instead.
