# Install the desktop app

## Overview

How to download, open and run the Delta desktop app from a GitHub Release,
without building anything. Bundles are published for macOS (Apple silicon
only; Intel Macs are not supported) and for Ubuntu and other Debian-based
distributions (x86_64, as a `.deb`). The app does not bundle `tmux` or the
agent CLIs, and the macOS build is unsigned, so the first launch on a Mac
needs a one-time Gatekeeper workaround.

- **[macOS](macos.md)** — the `.dmg`, and getting past Gatekeeper
- **[Ubuntu](ubuntu.md)** — the `.deb`, and the WebKitGTK notes

To run Delta from source instead, see
[the development guide](../development/README.md).

## What the app needs on the host

The app contains the Delta server and UI, but not the tools it drives:

- **`tmux`.** Agent sessions run inside tmux. Without it the app shows an
  error dialog at startup and exits.
- **An agent CLI.** An authenticated Claude Code (`claude`) and/or Codex
  (`codex`). Log in once from a terminal; Delta reuses that login and never
  runs the sign-in flow itself.

An app launched from Finder or a desktop file does not inherit a terminal's
environment, so at launch the app asks your login shell for its `PATH` and
uses that to find `tmux`, `claude` and `codex`. If a command works in a new
terminal window, the app finds it too.

## Where the app keeps its data

The app creates its data directory on first launch:

| Platform | Directory |
|---|---|
| macOS | `~/Library/Application Support/io.github.x7c1.delta/` |
| Ubuntu | `~/.local/share/io.github.x7c1.delta/` |

It holds the database (`delta.db`), the per-session working directories
(`sessions/`), and the hook state file (`delta-hook-state.json`). Conversation
transcripts are not in it: they stay where Claude Code and Codex write them.

### The hook state file

Each Claude Code session calls back into Delta through hook URLs that carry
Delta's port and a hook secret, and it keeps calling the URLs it was launched
with until it exits — also after Delta itself has quit and started again. So
that those calls still reach Delta and are accepted, the app keeps both in
`delta-hook-state.json`, next to the database:

- `hook_secret` — minted on the first launch and reused on every launch after.
- `port` — the port the app took. The next launch tries it first; if another
  program holds it by then, the app takes a fresh port, records that one, and
  logs a warning that sessions which survived the restart cannot reach it
  (see [Quitting, relaunching and upgrading](#quitting-relaunching-and-upgrading)
  for what those sessions look like).

The file is readable by you only (mode `0600`). If it is ever found readable by
others, the app restricts it to `0600` on startup and logs a warning; delete
the file as well if you want the old secret gone. Deleting the file rotates the
secret on the next launch (and forgets the port). Sessions still running from
before then keep the old values and can no longer report to Delta, so end them
first (see below).

## Quitting, relaunching and upgrading

Only one copy of the app runs at a time. Launching it while it is already
running brings the open window forward and starts nothing else.

Closing the window quits the app and stops the Delta server. What happens to
the sessions that were open depends on the agent:

- **Claude Code sessions keep running.** They run on Delta's own tmux server
  (socket `io.github.x7c1.delta`), not inside the app process, so they carry on
  while the app is closed — a turn in progress finishes. You can watch or type
  into one meanwhile with `tmux -L io.github.x7c1.delta attach`. On the next
  launch the app finds them again before it shows anything: each is listed as
  open, its terminal attaches to the same pane, and whatever it wrote while the
  app was closed appears in the conversation. A session whose pane ended in the
  meantime is listed as closed, and sending to it resumes it as usual.
- **Codex sessions end.** Their `codex app-server` runs inside the app process
  and stops with it. They are listed as closed on the next launch, and the
  next send resumes the conversation where it left off.

A surviving Claude Code session reports back to Delta through the hook URLs it
was launched with. If the next launch could not take the same port, or the
hook secret changed (see the hook state file above), those calls no longer
reach Delta. Such a session is still re-adopted — its output and terminal work
— but prompt echoes, turn ends and permission dialogs do not arrive, and the
session shows the notice *"Delta lost contact with this session"*. Use its
terminal, or choose Close in the session's menu and send again: Close ends the
old process, and the send resumes the conversation with current settings, which
restores everything.

Upgrading works the same way: install the new version over the old one (see
[Updating](#updating)) while sessions are running, and the new version
re-adopts them on its first launch. A re-adopted session keeps running the
Claude Code it was started with until it is closed and resumed.

To stop everything instead, quit the app and then end its tmux server with
`tmux -L io.github.x7c1.delta kill-server`. That ends every Claude Code
session; their conversations stay in Delta and resume on the next send.

## Removing everything

1. Quit the app, then end its sessions: `tmux -L io.github.x7c1.delta kill-server`.
2. Remove the app: delete `/Applications/Delta.app` on macOS, or run
   `sudo apt remove delta` on Ubuntu.
3. Delete the data directory (see
   [Where the app keeps its data](#where-the-app-keeps-its-data)).
4. Optionally delete `~/.delta/worktrees/`, where Delta creates git worktrees
   for sessions that asked for one, then run `git worktree prune` in each
   repository they came from so git forgets the removed worktrees.

## Updating

Download the bundle from the new Release and install it over the old one the
same way as the first time. Your data directory is left as is.

On the first launch of the new version, the database is migrated forward
automatically, with a snapshot taken first when a step is destructive. Going
back is not supported: a database written by a newer version makes an older
one refuse to start. See
[the compatibility policy](../compatibility.md#operational-safety-net-the-startup-gate).
