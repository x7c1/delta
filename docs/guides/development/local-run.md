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

`make desktop-dev` shows the same dev environment — data directory, tmux socket,
port — through the desktop shell instead of the browser; it and `make
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
make dev
```

`make dev` runs `scripts/dev.sh`, which:

1. Starts `delta-server` (`DELTA_PORT=7878`) under the dev identifier
   (`DELTA_IDENTIFIER=io.github.x7c1.delta.dev`). The identifier gives the
   server its own data directory — `~/.local/share/io.github.x7c1.delta.dev/`
   on Linux (`$XDG_DATA_HOME` when set), `~/Library/Application
   Support/io.github.x7c1.delta.dev/` on macOS — and its own tmux socket, so
   nothing it writes mixes with the installed app's. The layout of that
   directory is in [the development guide](README.md#backend-backend). The
   server owns the `claude` session lifecycle: when a session is spawned (first
   Send / New) it creates the tmux session and launches `claude --settings
   <file>` with Delta's rendered session settings (so the hooks point at
   `http://127.0.0.1:7878/hooks/...`); the settings file lives in the data
   directory, so a real project's own `.claude/settings.json` is never touched.
   A session started without a repository or a chosen directory runs in
   `sessions/<token>` there. Nothing is spawned on startup. The hook URLs also
   carry a hook secret, which the server keeps in `delta-hook-state.json` in the
   data directory (mode `0600`) and reuses on every start, so a session still
   running when the server restarts — after a crash, or when you restart only
   `delta-server` — still reaches it. `make down` ends the dev sessions along
   with the server, so none outlives that. Delete that file to rotate the
   secret; `DELTA_HOOK_SECRET` overrides it without replacing it.

   A dev environment from before the data directory kept its database in
   `backend/delta.db`. That file is no longer read: `make dev` starts with an
   empty session list in the new directory, and the old file stays where it is
   until you delete it.
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
DELTA_PORT=7878 DELTA_IDENTIFIER=io.github.x7c1.delta.dev \
  backend/target/release/delta-server
```

Then open <http://127.0.0.1:7878/>. The server hands out the page with the
per-run token injected, so `DELTA_AUTH_TOKEN` is optional (a bare run mints
one). Unlike `make dev`, nothing sets the other `DELTA_*` variables for you.
`DELTA_IDENTIFIER` points it at `make dev`'s data directory, so it shows the
same sessions, and at `make dev`'s tmux server; without it the server runs
under the installed app's identifier — its data directory and the tmux socket
it runs its sessions on — and `make down` would not end the sessions it
spawns.
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
| Identifier | `io.github.x7c1.delta` | `io.github.x7c1.delta.dev` |
| Data directory (database, settings, tmux config, session workdirs) | `<platform data dir>/io.github.x7c1.delta/` | `<platform data dir>/io.github.x7c1.delta.dev/` |
| Git worktrees | `~/.delta/worktrees` | `~/.delta-dev/worktrees` |
| tmux socket | `io.github.x7c1.delta` | `io.github.x7c1.delta.dev` |
| Port | the one recorded in the hook state file | 7878, pinned |
| Stopped by | closing its window | `make down` (`make reset` also deletes the database); closing the `make desktop-dev` window stops only its server |

`make dev` and `make desktop-dev` are two views of the **same** dev environment:
`scripts/dev.sh` holds the identifier (which names the data directory and the
tmux socket), the port and the worktree base for both, so a session spawned in the browser is listed in the dev desktop shell and
the other way round. Only the frontend differs — Vite on port 5173 for `make
dev`, the SPA embedded in the shell (built by `make web-dist`) for `make
desktop-dev`. They cannot run at the same time: both bind 127.0.0.1:7878, so
each refuses to start while the other (or anything else) holds that port, with a
message pointing at `make down`. `make down` stops whichever is running — it
frees port 7878, which ends a running `make desktop-dev` and closes its window
— and kills the dev tmux server; after it, either starts.

`make desktop-dev` is a debug build of the shell with its own identifier (see
[the development guide](README.md#desktop-shell-delta-desktop)), so its app data
directory, webview storage, single-instance lock and, on Linux, window identity
are separate from the installed app's: it opens its own window, under its own
dock icon, while the installed app runs, and never opens the installed app's
database or re-adopts its sessions.

## The desktop app

The installed app — the bundle `make desktop` builds and installs — runs the same server
inside a desktop window instead; `make desktop-dev` runs the dev environment in
that same shell (above). See
[the development guide](README.md#desktop-shell-delta-desktop) for the targets.
The installed app shares nothing with `make dev` (the table above), so the two
run side by side and `make down` and `make reset` leave the app's sessions
running. `DELTA_TMUX_SOCKET` overrides either tmux socket. The points below
describe the installed app; `make desktop-dev` runs under the dev identifier,
which names the dev environment's data directory and tmux socket, and sets
`DELTA_PORT` to 7878, so the data and port rules that follow from them apply
to it instead.

- **Startup.** The window opens at once on a page that says "Starting
  Delta…", and switches to Delta when the server is up: once your login shell
  has answered (below), the database is open and the sessions that survived
  the last quit are re-adopted. The app logs both moments at `info` (`window
  shown on the placeholder page`, then `window navigating to the server` with
  the port), so the time a launch spent on that page can be read from the log.
- **Window size.** The window opens at the size it was last left at, and
  maximized if it was. The first time — when nothing is remembered yet — it
  opens at 80 % of the screen's work area (the screen less the menu bar and
  Dock, or the panels), centred on macOS, not maximized, and at 1280×800 if no
  screen can be read. It cannot be made smaller than 1024×640, the smallest size at
  which the navigator, the conversation and the terminal still sit side by
  side. Only the size and the maximized state are remembered, not the
  position: Wayland does not let an app place its window. They are kept in
  `.window-state.json` in the app config directory, written as the app quits:
  `~/Library/Application Support/io.github.x7c1.delta/` on macOS (the data
  directory) and `~/.config/io.github.x7c1.delta/` on Linux. The dev
  identifier keeps its own file, so resizing the dev window leaves the
  installed app's size alone; delete the file to get the first-launch size
  again.
- **Data.** The app passes its bundle identifier to the server, which keeps
  everything it writes in the data directory that identifier names, created on
  first run: `~/Library/Application Support/io.github.x7c1.delta/` on macOS and
  `~/.local/share/io.github.x7c1.delta/` on Linux — the directory Tauri names
  the app data directory, so an install from before keeps its database. So the
  app starts with its own session list, not the one `make dev` shows. An
  explicit `DELTA_DATA_DIR` still wins, and the worktree base
  (`$HOME/.delta/worktrees`) and the transcript root (where Claude Code writes)
  keep their usual defaults.
- **Port.** The app has no fixed port, so it never collides with a `make dev`
  server on 7878. It records the port it took in the hook state file (below) and
  tries that one again on the next launch, falling back to a free loopback port
  — with a warning, and the new port recorded — when something else holds it.
  `DELTA_PORT` pins a port; that one is used as-is and never recorded. The dev
  environment pins 7878 this way, so the port recording is exercised only by
  the installed app.
- **Hook state file.** `delta-hook-state.json`, in the data directory, keeps the hook secret and the app's port so a
  session that outlives a restart still reaches the server with hook URLs it
  accepts. It is created `0600`; a file found readable by others is tightened
  to `0600` with a warning. `DELTA_HOOK_SECRET` overrides the recorded secret
  without replacing it. Delete the file to rotate the secret.
- **`PATH` and locale.** An app launched from Finder or a desktop file does not
  inherit your shell's environment, so at startup the app asks your login shell
  (`$SHELL`, or `/bin/sh`) for its `PATH`, `LANG` and every `LC_*`, and passes
  them to its server, which sets them on every command it starts (`tmux` and the
  agent in each pane, `codex`, `git`, `gh`, the editor opener) and looks the
  commands up on that `PATH`. They win over the environment the app was started with, so a launch from
  a terminal gets the login shell's values too. The variables are deliberately
  only these: anything else your rc files export (API keys, proxies) is not
  passed on. The server starts once the shell has answered, so a slow shell
  keeps the window on the starting page longer; if it does not answer within
  eight seconds, the commands keep the environment the app was started with
  and the app logs a warning.
  When neither the login shell nor the app's own environment sets `LC_ALL`,
  `LC_CTYPE` or `LANG`, the app passes `LANG=en_US.UTF-8` as well, so every
  command sees a UTF-8 locale. The browser version (`delta-server`, `make dev`)
  asks no shell: its commands inherit the terminal's environment.
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
  or written — are shown in a dialog over the starting page, and the app exits
  after it is dismissed. Any other startup failure shows a generic dialog
  ("Delta could not start. See the log for details.") over the same page, and
  the error itself is in the log.
- **Quitting.** Closing the window stops the server. The tmux server keeps
  running, so open sessions survive and are resumed on the next launch.

## Shut down

```bash
make down
```

This stops `delta-server` (or a running `make desktop-dev`, which holds the same
port 7878), the frontend dev server (port 5173), and every `delta-<n>` tmux
session the server spawned. To also delete the SQLite overlay in the dev data
directory so the next start recreates an empty schema, run `make reset`
instead.

**Settings → Storage → Erase everything** also stops the server, after closing
its sessions and removing their clean worktrees, and deletes its files in the
data directory, the directory itself only once nothing else is left in it
(see [the install guide](../install/README.md#removing-everything)). In
`make desktop-dev` the shell then shows what was kept, quits, and removes its
own directories under the dev identifier (`io.github.x7c1.delta.dev`), never
the installed app's. It is the user's way out, while `scripts/dev.sh --reset`
(`make reset`) remains the developer's quick reset.
