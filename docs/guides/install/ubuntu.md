# Install on Ubuntu

## Overview

The Linux bundle is a `.deb` for Ubuntu and other Debian-based distributions
(x86_64). This page covers installing it, rendering problems on WebKitGTK, and
updating — from the app itself (a password dialog, then a restart) or with
`sudo apt install` in a terminal. What the app needs on the host and where it
keeps its data are in the [install guide](README.md). For other
distributions, build from source with `make desktop-build` (see
[the development guide](../development/README.md)).

## Download and install

1. Install `tmux` if you do not have it: `sudo apt install tmux`.
2. Open the [latest Release](https://github.com/x7c1/delta/releases/latest)
   and download `delta-desktop_<version>_amd64.deb`.
3. Install it:

   ```bash
   sudo apt install ./delta-desktop_<version>_amd64.deb
   ```

`apt` pulls in the WebKitGTK and GTK libraries the app needs. Delta then
appears in the application menu, and the `delta-desktop` command starts it
from a terminal. To uninstall it, run `sudo apt remove delta-desktop`.

### Builds that installed as `delta`

Earlier builds installed a package named `delta`, which collides with
Ubuntu's own `delta` package (an unrelated test-case minimizer). The new
package does not replace such a build, so with both installed the application
menu shows Delta twice. If you installed one of those builds, remove it — but
only if it is Delta's: `dpkg -s delta` should show
`Description: Delta desktop shell: …`, not Ubuntu's `heuristic tool to
minimize failure-inducing files`.

```bash
dpkg -s delta        # check which `delta` is installed
sudo apt remove delta
```

Your data stays where it was: the data directory follows the app identifier,
which has not changed. The command those builds installed had a different
name; if you put it in a wrapper script or a desktop file (for example for the
EGL workaround below), change it to `delta-desktop`.

## Rendering on WebKitGTK

The app's window is WebKitGTK, not a desktop browser, and two things follow
from that.

- **Baseline speed.** Rendering is noticeably slower than the same UI in
  Chrome or Firefox. To keep scrolling usable, the app turns off decorative
  shadows and background washes on Linux. The visual effects control in
  Settings overrides that choice (`On` / `Off`; `Auto` is the platform
  default).
- **NVIDIA as the primary GPU.** WebKitGTK's hardware compositing can fail
  silently on these machines: nothing errors, but the window renders in
  software and is much slower. Pointing EGL at the Mesa vendor library before
  launching works around it:

  ```bash
  __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json delta-desktop
  ```

  The file's location can differ by distribution; look under
  `/usr/share/glvnd/egl_vendor.d/` for the Mesa entry. The app does not set
  this variable for you. To make it permanent, put it in a wrapper script or
  in the `Exec=` line of a desktop file through `env` (`Exec=` does not
  accept a bare `VAR=value` prefix):

  ```ini
  Exec=env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json delta-desktop
  ```

## Updating

The app updates itself from the footer, where a newer release is shown next to
the version:

1. **Update** downloads the new `.deb` into the data directory's `updates/`
   and checks its sha256 against the one the release states. The footer then
   says **Update ready** and offers **Install**.
2. **Install** opens the system's password dialog, which says that Delta
   wants to install its downloaded update. Enter an administrator's password.
   It asks every time; dismissing it leaves the update ready to install
   later. While it runs the footer says **Installing…**.
3. **Restart** appears once it is installed. The app closes and opens again
   as the new version. As when you quit the app, Claude Code sessions keep
   running in tmux across the restart and Codex sessions end (see
   [Quitting, relaunching and upgrading](README.md#quitting-relaunching-and-upgrading)).

Only a small helper the package installs
(`/usr/lib/delta-desktop/delta-update-helper`) runs as root, and it checks
the file again before installing it; the
[security guide](../security.md#updates) says what it checks and why Delta
never runs `sudo` in its own terminal.

If the app cannot install the update — no password dialog can be shown (for
example in a session without a polkit agent), you are not allowed to
authenticate as an administrator, or the install failed — the footer shows
the command to run in your own terminal, with a copy button and a link to the
release page, beside a retry:

```bash
sudo apt install /home/<you>/.local/share/io.github.x7c1.delta/updates/delta-desktop_<version>_amd64.deb
```

Then quit the app and start it again.

If the downloaded file fails the helper's check (it does not match the
release's digest, or is not `delta-desktop` at that version), the app removes
it and the footer says **Verification failed · Download again**, with a link
to the release page; no command is offered for that file. Download it again from
the footer, or get the `.deb` from the release page.

You can also install the new `.deb` from the Release with the same
`apt install` command as the first time. The
[install guide](README.md#updating) says what happens to your data. If the
build you are updating from installed as `delta`, the new package does not
replace it; see [Builds that installed as `delta`](#builds-that-installed-as-delta).
