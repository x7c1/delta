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

It holds the database (`delta.db`) and the per-session working directories
(`sessions/`). Conversation transcripts are not in it: they stay where
Claude Code and Codex write them.

Sessions run on Delta's own tmux server (socket `io.github.x7c1.delta`, i.e.
`tmux -L io.github.x7c1.delta`), not inside the app process. Closing the window stops the
Delta server but leaves the tmux server running, so open sessions survive and
are picked up again on the next launch.

### Removing everything

1. Quit the app, then end its sessions: `tmux -L io.github.x7c1.delta kill-server`.
2. Remove the app: delete `/Applications/Delta.app` on macOS, or run
   `sudo apt remove delta` on Ubuntu.
3. Delete the data directory above.
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
