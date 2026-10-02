---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! git grep -nE 'delta-app|make app([^-a-z]|$)|app-dev|check-app-build' -- ':!docs/tasks' ':!*.lock' && test -d backend/crates/apps/delta-desktop && make desktop && [ \"$(dpkg-deb -f backend/target/release/bundle/deb/*.deb Package)\" = delta-desktop ] && dpkg-deb -c backend/target/release/bundle/deb/*.deb | grep -q 'usr/bin/delta-desktop$'"
assignee: null
branch: task/1002-1500-refactor-rename-the-desktop-app-to-delta-desktop
created_at: 2026-10-02T06:07:26Z
updated_at: 2026-10-02T06:23:07Z
---

# refactor(desktop): rename the desktop app to delta-desktop

## Overview

The desktop app's Debian package is named `delta` (Tauri derives it from the
`productName`, `Delta`). Ubuntu's own archive already has a package called `delta`
(a test-case minimizer, version `2006.08.03`). Installing our `.deb` on a machine
that has it is treated as a *downgrade* that replaces that tool, and a later
`apt upgrade` "upgrades" back to Ubuntu's `delta` and silently removes the app. The
generic name also collides with other tools called `delta`.

At the same time, the desktop shell is called `delta-app` everywhere else (crate,
binary `/usr/bin/delta-app`, `make app` / `make app-dev`, the `check-app-build`
make target, docs). "app" will be ambiguous as soon as there is any other app (a
mobile one, say), and one thing should have one name.

Rename the desktop shell to **`delta-desktop`** throughout:

- The crate: move `backend/crates/apps/delta-app` to
  `backend/crates/apps/delta-desktop`; the Cargo package and binary become
  `delta-desktop` (update `backend/Cargo.toml`'s workspace comments and
  `Cargo.lock`).
- The bundle: the `.deb` package name and the installed command become
  `delta-desktop` (`/usr/bin/delta-desktop`). Find how Tauri v2 lets the package
  name differ from `productName` (for example `mainBinaryName`, or a deb-specific
  setting) without changing the user-visible name: `productName` stays `Delta`
  (window title, application menu entry, macOS `Delta.app`). If Tauri offers no way,
  rewrite the package name in the build step instead and explain why.
- Make targets: `make app` becomes `make desktop`, `make app-dev` becomes
  `make desktop-dev`, `check-app-build` becomes `check-desktop-build` (keep the
  help text accurate).
- CI: `.github/workflows/ci.yml` (the job step that builds/tests/lints the shell and
  its comment) and `.github/workflows/bundle.yml` (paths filter, `projectPath`,
  comments). Check the Release workflow and `docs/guides/release.md` for the bundle
  asset names they expect and keep them consistent with what the build now produces.
- Docs: `docs/guides/development/README.md` (the "Desktop shell" section and its
  anchor, which `local-run.md` links to), `docs/guides/development/local-run.md`,
  `docs/guides/install/ubuntu.md` (the command name, the EGL workaround lines, and
  the install/remove commands, which must now name `delta-desktop`),
  `docs/guides/install/README.md` (`sudo apt remove delta` becomes
  `sudo apt remove delta-desktop`), `docs/guides/release.md`.
- Code comments that name the shell: `delta-server`'s `config/mod.rs`, `lib.rs`,
  `serve.rs`, and `frontend/packages/apps/web/src/shell.ts`.

Do **not** change the app identifier `io.github.x7c1.delta`, the data directory,
the tmux socket name, or the display name `Delta`: they identify the product, not
the desktop flavour of it, and changing the identifier would move existing users'
data.

Add a short note to the install guide for anyone who installed an earlier build: the
old package was called `delta` (remove it with `sudo apt remove delta` only if it is
Delta's, i.e. `dpkg -s delta` shows the `io.github.x7c1.delta` app, not Ubuntu's
minimizer).

On this development machine `cc` is a Nix gcc wrapper; `make desktop` builds fine with
it (only the installed binary's loader is affected), so the check can bundle as is.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] No file outside `docs/tasks/` and lockfiles still says `delta-app`, `make app`,
      `app-dev` or `check-app-build` (the negative `git grep` gate).
- [x] The crate lives at `backend/crates/apps/delta-desktop` and `make check` passes
      (it builds, tests and lints the renamed crate via `check-desktop-build`).
- [x] `make desktop` produces a `.deb` whose package name is `delta-desktop` and which
      installs `usr/bin/delta-desktop` (the `dpkg-deb` gates).

### Manual / on-hardware (verified by a human before merge)

- [ ] The `.deb` installs with `sudo apt install ./…deb` next to Ubuntu's `delta`
      without touching it, the app starts from the application menu and as
      `delta-desktop`, and `sudo apt remove delta-desktop` removes it.
