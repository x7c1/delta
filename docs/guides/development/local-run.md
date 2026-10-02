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

`make desktop-dev` shows the same dev environment — database, sessions, tmux
socket, port — through the desktop shell instead of the browser; it and `make
dev` never run at once. The installed desktop app is a separate environment;
[Two environments](#two-environments-the-installed-app-and-the-dev-environment)
lays out which is which.

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
   touched. Nothing is spawned on startup. The hook URLs also carry a hook
   secret, which the server keeps in `backend/delta-hook-state.json` (beside
   `backend/delta.db`, mode `0600`) and reuses on every start, so a session
   still running when the server restarts — after a crash, or when you restart
   only `delta-server` — still reaches it. `make down` ends the dev sessions
   along with the server, so none outlives that.
   Delete that file to rotate the secret; `DELTA_HOOK_SECRET` overrides it
   without replacing it.
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
Send to it resumes it (`claude --resume`). After a server restart, a session
whose `claude` is still running in its tmux pane is re-adopted and shows as open
on that same pane; every other prior session shows as closed until it is
resumed via Send.

## First run / answering prompts

If `claude` is not yet authenticated, the first spawn will not become usable and
the UI shows an explicit error. Run `claude` once on its own to finish login, or
attach to a spawned pane to log in and to answer permission prompts as they
appear (each spawn is named `delta-<n>`; the first Send of a run spawns
`delta-1`):

```bash
tmux -L io.github.x7c1.delta.dev attach -t delta-1     # detach again with Ctrl-b then d
```

## Happy-path check

Type a message in the browser. It is dispatched into the tmux pane via
`send-keys`; `claude`'s reply is ingested from the transcript and surfaces in
the browser. When a tool needs permission, answer it in the embedded terminal or
in the TUI (`tmux -L io.github.x7c1.delta.dev attach -t delta-1`).

## Serving the built frontend

`make dev` serves the UI from Vite. To check the UI the way a packaged build
delivers it — the built SPA served by `delta-server` itself, no Vite running —
build the server with the frontend compiled in:

```bash
make server-embedded   # builds the SPA (make web-dist), then delta-server with --features embed-web
make down              # the binary takes the same port 7878 as make dev's server
DELTA_PORT=7878 DELTA_DB_PATH=backend/delta.db DELTA_TMUX_SOCKET=io.github.x7c1.delta.dev \
  backend/target/release/delta-server
```

Then open <http://127.0.0.1:7878/>. The server hands out the page with the
per-run token injected, so `DELTA_AUTH_TOKEN` is optional (a bare run mints
one). Unlike `make dev`, nothing sets the other `DELTA_*` variables for you, and
the server's defaults are relative to its working directory: `DELTA_DB_PATH`
defaults to `delta.db` there, so from the repository root pass
`backend/delta.db` (the database `make dev` uses) to see the same sessions.
`DELTA_TMUX_SOCKET` likewise points it at `make dev`'s tmux server; without it
the server uses the default socket, the one the desktop app runs its sessions
on, and `make down` would not end the sessions it spawns.
What is served, and how the page gets the token, is in
[the API guide](../api/README.md#the-built-frontend-embed-web). A plain
`cargo build` never needs `dist/`: only the `embed-web` feature compiles it in.

## Two environments: the installed app and the dev environment

A developer runs two Delta environments side by side. They share nothing of
Delta's own, so dogfooding the installed app and developing Delta never get in
each other's way. (Both read the agents' own records — Claude Code's
`~/.claude`, Codex's `~/.codex` — each by its own session ids.)

| | Installed app | Dev environment |
| --- | --- | --- |
| Started by | the bundle `make desktop` builds and installs (the `.deb`, `Delta.app`) | `make dev` (browser) or `make desktop-dev` (desktop shell) |
| Identifier | `io.github.x7c1.delta` | `io.github.x7c1.delta.dev` (`make desktop-dev` only) |
| Database | `delta.db` in the app data directory | `backend/delta.db` (or `DELTA_DB_PATH`) |
| Session workdirs | `sessions/` in the app data directory | `.tmp/session` (or `WORKDIR`) |
| Git worktrees | `~/.delta/worktrees` | `~/.delta-dev/worktrees` |
| tmux socket | `io.github.x7c1.delta` | `io.github.x7c1.delta.dev` |
| Port | the one recorded in the hook state file | 7878, pinned |
| Stopped by | closing its window | `make down` (`make reset` also deletes the database); closing the `make desktop-dev` window stops only its server |

`make dev` and `make desktop-dev` are two views of the **same** dev environment:
`scripts/dev.sh` holds the database, tmux socket, port, session workdir and
worktree base for both, so a session spawned in the browser is listed in the dev desktop shell and
the other way round. Only the frontend differs — Vite on port 5173 for `make
dev`, the SPA embedded in the shell (built by `make web-dist`) for `make
desktop-dev`. They cannot run at the same time: both bind 127.0.0.1:7878, so
each refuses to start while the other (or anything else) holds that port, with a
message pointing at `make down`. `make down` stops whichever is running — it
frees port 7878, which ends a running `make desktop-dev` and closes its window
— and kills the dev tmux server; after it, either starts.

`make desktop-dev` is a debug build of the shell with its own identifier (see
[the development guide](README.md#desktop-shell-delta-desktop)), so its app data
directory, webview storage and single-instance lock are separate from the
installed app's: it opens its own window while the installed app runs, and never
opens the installed app's database or re-adopts its sessions.

## The desktop app

The installed app — the bundle `make desktop` builds and installs — runs the same server
inside a desktop window instead; `make desktop-dev` runs the dev environment in
that same shell (above). See
[the development guide](README.md#desktop-shell-delta-desktop) for the targets.
The installed app shares nothing with `make dev` (the table above), so the two
run side by side and `make down` and `make reset` leave the app's sessions
running. `DELTA_TMUX_SOCKET` overrides either tmux socket. The points below
describe the installed app; `make desktop-dev` sets `DELTA_DB_PATH`,
`DELTA_SESSION_WORKDIR`, `DELTA_TMUX_SOCKET` and `DELTA_PORT` to the dev
environment's values, so the data and port rules that follow from them apply
to it instead.

- **Data.** The database and the per-spawn working directories live in the app
  data directory, created on first run: `delta.db` and `sessions/` under
  `~/Library/Application Support/io.github.x7c1.delta/` on macOS and
  `~/.local/share/io.github.x7c1.delta/` on Linux. So the app starts with its own
  session list, not the one `make dev` shows from `backend/delta.db`. An explicit
  `DELTA_DB_PATH` / `DELTA_SESSION_WORKDIR` still wins, and the worktree base
  (`$HOME/.delta/worktrees`) and the transcript root (where Claude Code writes)
  keep their usual defaults.
- **Port.** The app has no fixed port, so it never collides with a `make dev`
  server on 7878. It records the port it took in the hook state file (below) and
  tries that one again on the next launch, falling back to a free loopback port
  — with a warning, and the new port recorded — when something else holds it.
  `DELTA_PORT` pins a port; that one is used as-is and never recorded. The dev
  environment pins 7878 this way, so the port recording is exercised only by
  the installed app.
- **Hook state file.** `delta-hook-state.json`, next to the database (the
  directory of `DELTA_DB_PATH`), keeps the hook secret and the app's port so a
  session that outlives a restart still reaches the server with hook URLs it
  accepts. It is created `0600`; a file found readable by others is tightened
  to `0600` with a warning. `DELTA_HOOK_SECRET` overrides the recorded secret
  without replacing it. Delete the file to rotate the secret.
- **`PATH` and locale.** An app launched from Finder or a desktop file does not
  inherit your shell's environment, so at startup the app asks your login shell
  (`$SHELL`, or `/bin/sh`) for it and adopts its `PATH`, `LANG` and every
  `LC_*`: the `PATH` finds `tmux`, `claude` and `codex`, and the locale is what
  the sessions' commands run in. If the shell does not answer within a few
  seconds, the app keeps the environment it was started with and logs a warning.
  When neither the login shell nor the app's own environment sets `LC_ALL`,
  `LC_CTYPE` or `LANG`, the app sets `LANG=en_US.UTF-8`, so every command sees
  a UTF-8 locale.
- **Title bar (macOS).** The title bar is transparent and shows no title: the
  page paints the strip under the traffic lights in the active theme's
  background and lays itself out below it. Before the page's scripts run, the
  app marks `<html data-shell="tauri-macos">` and sets `--shell-top-inset` on
  `<html>` to the bar's height (28 px); the page renders the strip when it sees
  the attribute and sizes it from the variable. A transparent native view over
  the strip drags the window and handles double-click, so clicks there do not
  reach the page. In full screen, where macOS hides the bar, the app sets the
  variable to 0 and lets those clicks through, so the page fills the screen.
  The browser and the Linux app set neither and show no strip.
- **Links.** The window never leaves Delta's own page. A link that would open
  a new tab — every link in a message, including a footnote marker, and a
  session's pull-request number in the navigator — opens in a new tab of your
  default browser (through `open` on macOS, `xdg-open` on Linux); a relative or
  footnote link therefore shows Delta itself there. Any other `http` or `https`
  URL the page navigates to opens there too; only a navigation within Delta's
  own origin stays in the window. Clicking a link with any other scheme
  (`file:`, `javascript:`, `mailto:`, app schemes) does nothing, so text in a
  message cannot open local files or other applications; the app logs the
  refusal to its standard output. On Linux without `xdg-open`, web links do
  nothing either and the log says the opener could not start.
- **Startup errors.** The failures you have to act on — a database the binary
  refuses to open, a missing `tmux`, and a hook state file that cannot be read
  or written — are shown in a dialog, and the app exits after it is dismissed.
- **Quitting.** Closing the window stops the server. The tmux server keeps
  running, so open sessions survive and are resumed on the next launch.

## Shut down

```bash
make down
```

This stops `delta-server` (or a running `make desktop-dev`, which holds the same
port 7878), the frontend dev server (port 5173), and every `delta-<n>` tmux
session the server spawned. To also delete the SQLite overlay
so the next start recreates an empty schema, run `make reset` instead.
