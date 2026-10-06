---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && test -f assets/icon/delta.svg && test -x scripts/gen-icons.sh && git grep -q 'icons:' -- Makefile && ! (cd backend/crates/apps/delta-desktop/icons && shasum -a 256 32x32.png 128x128.png 128x128@2x.png icon.icns icon.ico | grep -qE 'aaa96d0c53b7|eeebf55d8ffe|2998b41d0c92|1535d355994f|e38ca88e1d54') && test -f backend/crates/apps/delta-desktop/icons/icon.png && test -d backend/crates/apps/delta-desktop/icons-dev && file backend/crates/apps/delta-desktop/icons/128x128@2x.png | grep -q '256 x 256' && test -f frontend/packages/apps/web/public/favicon.svg && git grep -q 'favicon' -- frontend/packages/apps/web/index.html && git grep -q 'icons-dev' -- scripts/dev.sh"
assignee: null
branch: task/1006-2030-build-desktop-derive-the-app-icons-and-the-favicon-from-one-svg-with-a-badge-for-the-dev-build
created_at: 2026-10-06T12:22:15Z
updated_at: 2026-10-06T12:55:08Z
---

# build(desktop): derive the app icons and the favicon from one SVG, with a badge for the dev build

## Overview

The five files in `backend/crates/apps/delta-desktop/icons/` are Tauri's
sample icons: their SHA-256 sums match tauri-cli 2.12's
`templates/app/src-tauri/icons/` byte for byte. The repository has
derived files and no source, and the web app has no favicon at all
(`frontend/packages/apps/web/public/` holds only the mock service worker).

### The source

One SVG, `assets/icon/delta.svg`, at the repository root because it is the
source for the desktop crate and the web app alike. It is placed in this
branch (the task's PR commits it; it is the only file under `assets/`): a
512×512 square, a rounded light background (`rx=114`, 22 % of the side)
filling the whole canvas, two coloured triangles, no text, no external
references, 341 bytes. It reads at 16 px.

### Derivation

Everything else is generated from it by `scripts/gen-icons.sh`, run by a
new `make icons` target, and committed. The script needs only tauri-cli
(`cargo tauri icon` rasterises SVG itself and is already required for
`make desktop-build`) and the shell: no ImageMagick or rsvg on the
developer machine. Where a variant is needed, the script composes a
*derived SVG* (the source wrapped in a new `<svg>` with a transform or an
overlay) in a temp directory and feeds that to `cargo tauri icon`.

- **Installed app** → `icons/`: run `cargo tauri icon assets/icon/delta.svg -o <tmp>`,
  keep `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.ico` and add
  `icon.png` (the 512 px output), discard `Square*Logo.png`, `StoreLogo.png`,
  `android/`, `ios/`. List `icons/icon.png` first in `bundle.icon` so the
  embedded window icon (codegen takes the first PNG) is sharp on Linux.
- **macOS `.icns`**: the Dock convention is a rounded square at about 80 %
  of the canvas with transparent margins; a full-bleed icon looks larger
  than its neighbours. Compose a derived SVG that scales the source to 80 %
  and centres it on a transparent 512 canvas, run `cargo tauri icon` on it,
  and take only its `icon.icns`.
- **Dev build** → `icons-dev/`: the same set from a derived SVG that
  overlays a badge on the source — a small filled disc in a corner with a
  contrasting "dev" glyph drawn as paths, legible at 32 px. `scripts/dev.sh`
  adds `"bundle": {"icon": [..icons-dev paths..]}` to `DESKTOP_DEV_TAURI_CONFIG`,
  so the dev binary embeds the badged window icon (what GNOME's dock and
  Alt-Tab show for the `.dev` app ID). The installed app's `tauri.conf.json`
  is not touched by this.
- **Favicon**: copy the source to `frontend/packages/apps/web/public/favicon.svg`
  and generate `favicon-32.png` (`cargo tauri icon -p 32`); `index.html`
  links both (`<link rel="icon" type="image/svg+xml">` with the PNG as the
  fallback for browsers without SVG favicons). The dev server and the
  embedded server already serve `public/`.
- The script is idempotent and prints what it wrote. Put the three derived
  SVG compositions in the script as here-documents so the geometry is
  readable in one place; the `rx` and the 80 % and badge numbers are named
  variables at the top.

### Docs

`docs/guides/development/README.md` "Desktop shell": a short paragraph —
the source, `make icons`, that the outputs are committed, and the dev badge.
Mention the `.icns` inset rule in one sentence so nobody "fixes" it.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The source, the script and the `make icons` target exist; no icon
      file matches a Tauri sample; `icon.png` and `icons-dev/` exist;
      `128x128@2x.png` is 256×256; the favicon exists and is linked; the dev
      build references the badged set (gates in `check_command`).
- [x] `make check` passes, including `check-desktop-build`.

### Before merge (verified outside the check command)

- [x] On macOS: `make desktop` installs; the Dock, Finder, Cmd-Tab and the
      `.dmg` window show the new icon at Dock size with margins like the
      neighbouring apps; `make desktop-dev` shows the badged icon in the
      Dock, distinguishable from the installed one. The browser tab at
      `http://127.0.0.1:7878/` (`make dev`) shows the favicon.
- [ ] On Ubuntu (GNOME): the application grid, dock and Alt-Tab show the new
      icon for the installed app (`dpkg-deb -c` lists
      `usr/share/icons/hicolor/*/apps/delta-desktop.png`), and the badged
      one for `make desktop-dev`; both at small sizes in the light and dark
      dock.
