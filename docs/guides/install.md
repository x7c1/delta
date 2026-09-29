# Install the desktop app

## Overview

How to download, open and run the Delta desktop app from a GitHub Release,
without building anything. Bundles are published for macOS (Apple silicon and
Intel) and Linux (x86_64). The app does not bundle `tmux` or the agent
CLIs. The macOS build is unsigned, so the first launch on a Mac needs a
one-time Gatekeeper workaround.

To run Delta from source instead, see
[the development guide](development/README.md).

## Download

Open the [latest Release](https://github.com/x7c1/delta/releases/latest) and
download the file for your machine:

| Machine | File |
|---|---|
| Mac with Apple silicon (M1 and later) | `Delta_<version>_aarch64.dmg` |
| Mac with an Intel processor | `Delta_<version>_x64.dmg` |
| Linux, Debian/Ubuntu (x86_64) | `Delta_<version>_amd64.deb` |
| Linux, other distributions (x86_64) | `Delta_<version>_amd64.AppImage` |

## What the app needs on the host

The app contains the Delta server and UI, but not the tools it drives:

- **`tmux`.** Agent sessions run inside tmux. Install it with
  `brew install tmux` on macOS or your package manager on Linux
  (e.g. `apt install tmux`). Without it the app shows an error dialog at
  startup and exits.
- **An agent CLI.** An authenticated Claude Code (`claude`) and/or Codex
  (`codex`). Log in once from a terminal; Delta reuses that login and never
  runs the sign-in flow itself.

An app launched from Finder or a desktop file does not inherit a terminal's
environment, so at launch the app asks your login shell for its `PATH` and
uses that to find `tmux`, `claude` and `codex`. If a command works in a new
terminal window, the app finds it too. The details are in
[local-run.md](development/local-run.md#the-desktop-app).

## macOS

1. Open the `.dmg` and drag `Delta.app` into `Applications`.
2. Open `Delta.app`. The first time, macOS refuses: it reports that the app
   "is damaged and can't be opened" or that Apple "could not verify" it.

This happens because the project has no Apple developer account, so the app
is neither signed by an identified developer nor notarized by Apple, and
Gatekeeper, the macOS check for downloaded apps, blocks it. Either of the
following gets past the block; you only need it once per downloaded copy.

**Allow it in System Settings.** After the first refusal, dismiss the dialog
(**Done**), go to **System Settings → Privacy & Security**, find the message
that Delta was blocked near the bottom and click **Open Anyway**, then open
the app again and confirm **Open Anyway** with your password. This is the
path on macOS 15 (Sequoia) and later, where right-click (or Control-click) →
**Open** no longer gets past the block; on macOS 14 and earlier, right-click
`Delta.app` in `Applications`, choose **Open** and confirm **Open** in the
dialog. When macOS says the app is damaged, no Open Anyway button appears; use
the next workaround.

**Remove the quarantine flag.** macOS marks downloaded files with a quarantine
attribute, which is what triggers the check. Clearing it from the installed
app lets it open normally. This is the reliable fix when macOS says the app is
damaged:

```bash
xattr -d com.apple.quarantine /Applications/Delta.app
```

If you put the app somewhere other than `Applications`, pass that path
instead (for example `~/Applications/Delta.app`).

## Linux

### `.deb` (Debian, Ubuntu and derivatives)

```bash
sudo apt install ./Delta_<version>_amd64.deb
```

`apt` pulls in the WebKitGTK and GTK libraries the app needs. Delta then
appears in the application menu, and the `delta-app` command starts it from
a terminal.

### `.AppImage` (other distributions)

```bash
chmod +x Delta_<version>_amd64.AppImage
./Delta_<version>_amd64.AppImage
```

AppImages need FUSE 2 to mount themselves. If the run fails with an error
about `libfuse.so.2`, install it (`libfuse2`, or `libfuse2t64` on Ubuntu
24.04 and later), or run it without mounting via
`./Delta_<version>_amd64.AppImage --appimage-extract-and-run`.

### Rendering on WebKitGTK

On Linux the app's window is WebKitGTK, not a desktop browser, and two things
follow from that.

- **Baseline speed.** Rendering is noticeably slower than the same UI in
  Chrome or Firefox. To keep scrolling usable, the app turns off decorative
  shadows and background washes on Linux. The visual
  effects control in Settings overrides that choice (`On` / `Off`; `Auto` is
  the platform default).
- **NVIDIA as the primary GPU.** WebKitGTK's hardware compositing can fail
  silently on these machines: nothing errors, but the window renders in
  software and is much slower. Pointing EGL at the Mesa vendor library before
  launching works around it:

  ```bash
  __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json delta-app
  ```

  For the AppImage, put the same variable in front of the AppImage path. The
  file's location can differ by distribution; look under
  `/usr/share/glvnd/egl_vendor.d/` for the Mesa entry. The app does not set
  this variable for you, so to make it permanent put it in a wrapper script,
  or in the `Exec=` line of a desktop file through `env`, since `Exec=` does
  not accept a bare `VAR=value` prefix:

  ```ini
  Exec=env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json delta-app
  ```

## Where the app keeps its data

The app creates its data directory on first launch:

| Platform | Directory |
|---|---|
| macOS | `~/Library/Application Support/io.github.x7c1.delta/` |
| Linux | `~/.local/share/io.github.x7c1.delta/` |

It holds the database (`delta.db`) and the per-session working directories
(`sessions/`). Conversation transcripts are not in it: they stay where
Claude Code and Codex write them.

Sessions run on Delta's own tmux server (socket `delta`, i.e.
`tmux -L delta`), not inside the app process. Closing the window stops the
Delta server but leaves the tmux server running, so open sessions survive and
are picked up again on the next launch.

### Removing everything

1. Quit the app, then end its sessions: `tmux -L delta kill-server`.
2. Remove the app: delete `/Applications/Delta.app` on macOS, run
   `sudo apt remove delta` for the `.deb`, or delete the `.AppImage` file.
3. Delete the data directory above.
4. Optionally delete `~/.delta/worktrees/`, where Delta creates git worktrees
   for sessions that asked for one, then run `git worktree prune` in each
   repository they came from so git forgets the removed worktrees.

## Updating

Download the bundle from the new Release and replace the app: drag the new
`Delta.app` over the old one on macOS (and clear the quarantine flag again),
install the new `.deb` with the same `apt install` command, or swap the
`.AppImage` file. Your data directory is left as is.

On the first launch of the new version, the database is migrated forward
automatically, with a snapshot taken first when a step is destructive. Going
back is not supported: a database written by a newer version makes an older
one refuse to start. See
[the compatibility policy](compatibility.md#operational-safety-net-the-startup-gate).
