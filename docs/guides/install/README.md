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

It holds:

- `delta.db`, the database, with its `delta.db-wal` and `delta.db-shm` files
  (SQLite runs in WAL mode). The database shrinks as sessions are deleted
  (`auto_vacuum = FULL`), and `delta.db-wal` is cut back to 4 MiB after each
  checkpoint.
- `delta.db.bak-v<N>`, a snapshot of the database taken before an upgrade step
  that rewrites data (see [Updating](#updating)). Snapshots are kept until you
  delete them from Settings → Storage.
- `delta-hook-state.json`, the hook state file (see below).
- `sessions/`, the working directories of sessions started without a
  repository or a chosen folder.
- `settings/<port>.json`, the settings a session is launched with. There is one
  file for every port the app has used.
- `tmux.conf`, the configuration of Delta's tmux server.

The directory is readable by you only (mode `0700`) when the app creates it. A
directory an earlier version created keeps its mode; the hook state file and the
settings files are readable by you only either way.

Conversation transcripts are not in it: they stay where Claude Code and Codex
write them.

The webview that draws the UI keeps its own storage:

- **macOS.** Outside the data directory, in
  `~/Library/WebKit/io.github.x7c1.delta/` and
  `~/Library/Caches/io.github.x7c1.delta/`.
- **Ubuntu.** Inside the data directory, in `localstorage/`, `storage/`,
  `CacheStorage/` and `WebKitCache/`.

tmux keeps the socket of Delta's tmux server, `io.github.x7c1.delta`, in
`/tmp/tmux-<uid>/` on both platforms (under `$TMUX_TMPDIR` instead, if you set
it). The socket file stays there after the server ends.

### The hook state file

Each Claude Code session calls back into Delta through hook URLs that carry
Delta's port and a hook secret, and it keeps calling the URLs it was launched
with until it exits — also after Delta itself has quit and started again. So
that those calls still reach Delta and are accepted, the app keeps both in
`delta-hook-state.json`, in the data directory:

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

Follow these steps in order. Steps 1 to 4 delete everything listed in
[Where the app keeps its data](#where-the-app-keeps-its-data).

1. Quit the app, then end its sessions and delete the tmux socket, which
   `kill-server` leaves behind:

   ```sh
   tmux -L io.github.x7c1.delta kill-server
   rm -f "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)/io.github.x7c1.delta"
   ```

2. Remove the app: delete `/Applications/Delta.app` on macOS, or run
   `sudo apt remove delta-desktop` on Ubuntu.
3. Delete the data directory. This also removes the
   [hook state file](#the-hook-state-file), and on Ubuntu the webview storage.
4. On macOS, delete the webview storage:

   ```sh
   rm -rf ~/Library/WebKit/io.github.x7c1.delta ~/Library/Caches/io.github.x7c1.delta
   ```

5. If you ever ran a version from before the settings and the tmux
   configuration moved into the data directory, delete what it left in the
   system temp directory — `/tmp` on Ubuntu, `$TMPDIR` on macOS (a per-user
   directory under `/var/folders/`; run `echo $TMPDIR` to see yours). On macOS:

   ```sh
   rm -rf "$TMPDIR"/delta-[0-9]* "$TMPDIR"/delta-tmux-io.github.x7c1.delta.conf
   ```

   On Ubuntu:

   ```sh
   rm -rf /tmp/delta-[0-9]* /tmp/delta-tmux-io.github.x7c1.delta.conf
   ```

6. Optionally remove the git worktrees. For a session that asked for one, Delta
   creates a worktree under `~/.delta/worktrees/` on a new branch named
   `delta-<session id>` in the repository it came from. Removing a session in
   Delta already removes its worktree and that branch when they hold no work,
   so what is left either holds work, belongs to a session still listed, or
   belongs to a session removed by an earlier version that did not clean up.
   Delete `~/.delta/worktrees/`, then in each of those repositories run
   `git worktree prune` so git forgets the removed worktrees, and delete the
   branches you no longer want. `git branch --list 'delta-*'` lists them, and
   `git branch -D <branch>` deletes one. A session started from an existing
   branch, such as a pull request's, uses that branch instead; if the
   repository had no local branch of that name, Delta created one, and it does
   not start with `delta-`.
7. Optionally remove the trust entries. For each worktree, Delta adds an entry
   keyed by the worktree's path under `projects` in `~/.claude.json`, so Claude
   Code does not ask whether to trust the folder. The file belongs to Claude
   Code and holds your other settings too, so do not delete it. Look for the
   keys under `projects` that start with your `~/.delta/worktrees/` path and
   remove the ones you no longer want.

If you ran a version before v0.5.0, it used older names that the current app
never touches, and these may also exist: `~/Library/WebKit/delta-app/` and
`~/Library/Caches/delta-app/` on macOS, `delta-tmux-delta.conf` in the temp
directory, and the tmux socket `delta` in `/tmp/tmux-<uid>/`.
End that version's tmux server with `tmux -L delta kill-server` before
deleting them.

## Updating

Download the bundle from the new Release and install it over the old one the
same way as the first time. Your data directory is left as is.

On the first launch of the new version, the database is migrated forward
automatically, with a snapshot taken first when a step is destructive. Going
back is not supported: a database written by a newer version makes an older
one refuse to start. See
[the compatibility policy](../compatibility.md#operational-safety-net-the-startup-gate).
