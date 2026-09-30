# Install on Ubuntu

The Linux bundle is a `.deb` for Ubuntu and other Debian-based distributions
(x86_64). What the app needs on the host, where it keeps its data and how
updates behave are in the [install guide](README.md). Other distributions are
not covered by a bundle; build from source with `make app` (see
[the development guide](../development/README.md)).

## Download and install

1. Install `tmux` if you do not have it: `sudo apt install tmux`.
2. Open the [latest Release](https://github.com/x7c1/delta/releases/latest)
   and download `Delta_<version>_amd64.deb`.
3. Install it:

   ```bash
   sudo apt install ./Delta_<version>_amd64.deb
   ```

`apt` pulls in the WebKitGTK and GTK libraries the app needs. Delta then
appears in the application menu, and the `delta-app` command starts it from
a terminal.

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
  __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json delta-app
  ```

  The file's location can differ by distribution; look under
  `/usr/share/glvnd/egl_vendor.d/` for the Mesa entry. The app does not set
  this variable for you, so to make it permanent put it in a wrapper script,
  or in the `Exec=` line of a desktop file through `env`, since `Exec=` does
  not accept a bare `VAR=value` prefix:

  ```ini
  Exec=env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json delta-app
  ```

## Updating

Install the new `.deb` with the same `apt install` command. The
[install guide](README.md#updating) says what happens to your data.
