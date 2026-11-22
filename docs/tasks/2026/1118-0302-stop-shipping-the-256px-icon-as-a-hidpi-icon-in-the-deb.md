---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1118-0302-stop-shipping-the-256px-icon-as-a-hidpi-icon-in-the-deb
created_at: 2026-11-17T18:02:31Z
updated_at: 2026-11-17T18:28:11Z
---

# fix(desktop): stop shipping the 256 px icon as a HiDPI icon in the .deb

## Overview

On Linux, the installed app's icon looks blurred in GNOME's Activities overview
and in the Ctrl+Alt+Tab switcher when the display is scaled to 1.25.

The cause is `bundle.icon` in
`backend/crates/apps/delta-desktop/tauri.conf.json`, which lists
`icons/128x128@2x.png` (a 256×256 PNG). Tauri's Debian bundler installs an
`@2x` file under the hicolor theme as `256x256@2`, i.e. as a 256-px icon for
scale 2, which GNOME Shell then treats as a 512-px-equivalent icon and scales
down from the wrong size. Removing that hicolor entry from an installed system
was confirmed by hand to make the icon sharp.

Change:

1. Remove `"icons/128x128@2x.png"` from `bundle.icon` in
   `backend/crates/apps/delta-desktop/tauri.conf.json`. Keep the file itself:
   `make icons` (`scripts/gen-icons.sh`) still generates it, and
   `tauri.macos.conf.json` lists it for the macOS bundle (Tauri replaces the
   whole `bundle.icon` array from the platform config, so the macOS list is
   unaffected).
2. Do **not** change `tauri.macos.conf.json`, `scripts/gen-icons.sh`'s output
   set, or the dev build's icon list in `scripts/dev.sh` (`DESKTOP_DEV_ICONS`):
   the dev build is never installed as a `.deb`, so its icons never land in
   hicolor.
3. Add a test in the `delta-desktop` crate (beside
   `backend/crates/apps/delta-desktop/tests/update_bundle.rs`, which already
   reads the bundle config, or in a new file under `tests/`) that parses
   `tauri.conf.json` and fails if any `bundle.icon` entry is an `@2x` PNG, with
   a comment saying why (the Debian bundler puts it under hicolor's
   `<size>@2`, which GNOME scales from the wrong size).
4. Update the documentation that describes which icons the installed app uses:
   the header comment of `scripts/gen-icons.sh` (the `icons/` line says
   "the installed app (tauri.conf.json)" for the whole set) and the icon
   paragraph in `docs/guides/development/README.md`, so they say that
   `128x128@2x.png` is used by the macOS bundle only and why the Linux bundle
   leaves it out.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `tauri.conf.json`'s `bundle.icon` no longer lists `icons/128x128@2x.png`; `tauri.macos.conf.json` still does, and `icons/128x128@2x.png` still exists
- [x] A `delta-desktop` test fails if `tauri.conf.json`'s `bundle.icon` lists an `@2x` PNG, and passes now
- [x] `scripts/gen-icons.sh`'s header and `docs/guides/development/README.md` state that the `@2x` icon is for the macOS bundle only and why the Linux bundle omits it
- [x] `make check` passes
