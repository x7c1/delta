---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && test -x scripts/setup-system.sh"
assignee: null
branch: task/1109-1333-add-a-script-that-installs-the-system-packages-for-make-check
created_at: 2026-11-09T04:33:00Z
updated_at: 2026-11-09T04:47:41Z
---

# build: add a script that installs the system packages `make check` needs on Debian/Ubuntu

## Overview

`make check` needs system packages that a fresh Debian/Ubuntu machine does not have: the libraries Tauri v2 links against (`check-desktop-build`, `check-backend-doc`), `tmux` for the suite, and the Playwright Chromium for `check-e2e` and `check-e2e-fake`. Today they are listed in prose in `docs/guides/development/README.md` (the "Prerequisites by platform" table) and in the Makefile's `e2e` comment, so setting up a new machine, container or CI-like sandbox means copying commands out of the docs by hand. The Chromium in particular has to match the `@playwright/test` version pinned in `frontend/pnpm-lock.yaml`, or the e2e steps fail to find their browser.

What to do:

1. **Add `scripts/setup-system.sh`** — an executable Bash script that installs, on Debian/Ubuntu:
   - the apt packages: `tmux`, `lsof`, `jq`, `make`, and the Tauri v2 libraries listed in the docs (`libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev patchelf build-essential file`)
   - the Playwright Chromium (and the headless shell) at the version `frontend/pnpm-lock.yaml` pins for `@playwright/test`, read from the lockfile so the script stays correct when Playwright is bumped. It must work before `pnpm install` has run (for example with `npx -y playwright@<version> install chromium chromium-headless-shell`), and honour `PLAYWRIGHT_BROWSERS_PATH` when it is set
   - It runs with `sudo` when not root and without it when root; it is safe to run again (a second run changes nothing and exits 0); it fails with a clear message on a system without `apt-get`; it resolves paths from its own location, so it works from any working directory
2. **Make the docs point to it** — replace the hand-written `apt install` lists in the "Prerequisites by platform" table with a reference to the script (keep the table's per-platform explanation of what each group is for), and update the Makefile's `e2e` comment so the one-time browser install names the script. Follow the repository's DRY rule: the package list lives in the script only
3. **Keep CI unchanged** — `.github/workflows/` keeps its own `APT_PACKAGES` lists, which are part of its apt cache keys. Do not edit `.github/`

Out of scope:

- macOS setup (Homebrew) and the Rust and Node toolchains
- Changing CI to call the script

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make check` passes, including the repository's shell-script checks on the new script
- [x] `scripts/setup-system.sh` is executable

### Before merge (verified outside the check command)

- [x] In a fresh `ubuntu:24.04` container with only Node.js 22 added, running the script as root installs the packages and the Chromium that the lockfile's Playwright version expects (`pkg-config --modversion gdk-3.0` succeeds, and the browsers directory has the `chromium-<revision>` that `playwright-core`'s `browsers.json` names); a second run exits 0
