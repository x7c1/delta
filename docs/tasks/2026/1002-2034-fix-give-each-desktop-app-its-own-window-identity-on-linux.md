---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && [ \"$(jq -r '.app.enableGTKAppId' backend/crates/apps/delta-desktop/tauri.conf.json)\" = true ] && [ \"$(sed -n 's/^StartupWMClass=//p' backend/crates/apps/delta-desktop/linux/delta-desktop.desktop)\" = \"$(jq -r .identifier backend/crates/apps/delta-desktop/tauri.conf.json)\" ] && make desktop-build && dpkg-deb --fsys-tarfile \"$(ls -t backend/target/release/bundle/deb/delta-desktop_*.deb | head -n 1)\" | tar -xO usr/share/applications/delta-desktop.desktop | grep -qx 'StartupWMClass=io.github.x7c1.delta'"
assignee: null
branch: task/1002-2034-fix-give-each-desktop-app-its-own-window-identity-on-linux
created_at: 2026-10-02T11:34:47Z
updated_at: 2026-10-02T11:52:37Z
---

# fix(desktop): give each desktop app its own window identity on Linux

## Overview

On GNOME (Wayland), the installed app (`make desktop`) and the dev
environment's app (`make desktop-dev`) are treated as one application while
both are running. The dock shows a single Delta icon carrying both windows, and
choosing "Delta" from the Activities search raises whichever of the two windows
had focus last, so the installed app cannot be reached reliably while the dev
one is open.

The cause is the window's app ID. Tauri hands GTK an application ID only when
`app.enableGTKAppId` is set (`tauri` 2.12 `src/app.rs`, the `enable_gtk_app_id`
branch); it defaults to `false`, so GTK falls back to the program name, and
both builds are an executable named `delta-desktop`. Both windows therefore
carry the app ID `delta-desktop`, which GNOME matches to the installed
`delta-desktop.desktop` (`StartupWMClass=delta-desktop`, from
`StartupWMClass={{exec}}` in
`backend/crates/apps/delta-desktop/linux/delta-desktop.desktop`). The
identifiers already differ — `io.github.x7c1.delta` in `tauri.conf.json` and
`io.github.x7c1.delta.dev` through the `TAURI_CONFIG` override in
`scripts/dev.sh` — but today they separate only the app data directory and the
single-instance scope, not the window.

Make the identifier the window's identity as well:

- Set `app.enableGTKAppId: true` in
  `backend/crates/apps/delta-desktop/tauri.conf.json`, so each build's GTK
  application ID (and so its Wayland app ID) is its identifier. The dev build
  picks this up with no change of its own, since its override replaces only
  the identifier.
- Change `StartupWMClass` in the Linux desktop entry template to the installed
  app's identifier, `io.github.x7c1.delta`, so GNOME keeps associating the
  installed app's window with its desktop entry (name and icon). The template
  has no variable for the identifier, so the value is written literally; the
  check command pins it to `tauri.conf.json`'s `identifier` so the two cannot
  drift.
- No desktop entry is added for the dev build. Its window matches none, so
  GNOME shows it as a separate application and never raises it for "Delta".

Tauri documents the cost of `enableGTKAppId`: registering an application ID
also registers the app on the session bus under that ID, which prevents
running a second instance. The desktop app is already single-instance by
design (`tauri-plugin-single-instance` in `src/main.rs`, see
`focus_running_window`), and the plugin's D-Bus name is
`<identifier>.SingleInstance`, distinct from the GTK one. What must be kept is
the existing behaviour of a second launch: it focuses the running window and
exits. If GTK's own single-instance handling intercepts the second launch
before the plugin does (the second process hanging, exiting without focusing,
or the first instance re-running its startup), fix that within this task
rather than dropping the setting.

macOS is out of scope: `enableGTKAppId` is ignored there, and the bundle is
identified by its bundle identifier already.

Update the desktop-shell section of `docs/guides/development/README.md` (the
paragraph that lists what the dev identifier separates) to say it also
separates the window's identity on Linux, and mention the
`StartupWMClass`/identifier coupling where the desktop entry template is
described, if anywhere.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `tauri.conf.json` sets `app.enableGTKAppId` to `true` (jq gate in
      `check_command`).
- [x] The desktop entry template's `StartupWMClass` equals
      `tauri.conf.json`'s `identifier` (gate in `check_command`).
- [x] The `.deb` built by `make desktop-build` installs
      `/usr/share/applications/delta-desktop.desktop` with
      `StartupWMClass=io.github.x7c1.delta` (gate in `check_command`).

### Manual / on-hardware (verified by a human before merge)

- [ ] With the installed app and `make desktop-dev` both running on GNOME
      (Wayland), the dock shows them as two separate icons.
- [ ] With both running, choosing "Delta" from the Activities search always
      raises the installed app's window, whichever window had focus last.
- [ ] The installed app's window shows the Delta name and icon in the dock and
      the app switcher (it is still matched to its desktop entry).
- [ ] Launching the installed app again while it is running focuses the
      running window and exits, without starting a second server.
