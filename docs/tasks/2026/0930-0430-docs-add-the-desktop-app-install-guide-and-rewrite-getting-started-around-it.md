---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0930-0430-docs-add-the-desktop-app-install-guide-and-rewrite-getting-started-around-it
created_at: 2026-09-29T18:10:00Z
updated_at: 2026-09-29T20:19:11Z
---

# docs: add the desktop app install guide and rewrite Getting started around it

## Overview

Each GitHub Release now carries unsigned desktop bundles (two macOS
`.dmg`, a `.deb` and an `.AppImage`, built by `.github/workflows/bundle.yml`
and attached by the `Release` workflow), but `README.md` still says Delta
is distributed as source only and points every reader at `make dev`. This
task writes the end-user path: a guide for installing and opening the
unsigned app, and a `Getting started` that leads with the download.

### Change

1. **`docs/guides/install.md`** (new, with an Overview section). In order:
   - what to download from the latest Release for each platform (Apple
     silicon `.dmg`, Intel `.dmg`, `.deb`, `.AppImage`) and what the app
     needs on the host: `tmux`, and an authenticated `claude` and/or
     `codex` on the login shell's `PATH` (the app imports that `PATH` at
     launch, so a CLI that works in the terminal works in the app);
   - macOS: the bundles are not signed or notarized, so Gatekeeper reports
     the app as damaged or blocks it. Give both workarounds: right-click
     → Open (and Privacy & Security → Open Anyway on recent macOS), and
     `xattr -d com.apple.quarantine /Applications/Delta.app`. State plainly
     why (no Apple developer account for the project), so the builds are
     unsigned;
   - Linux: installing the `.deb` (`apt install ./Delta_<version>_amd64.deb`)
     or running the `.AppImage` (`chmod +x`, then run; FUSE note), and the
     two known WebKitGTK items: the baseline rendering is slower than a
     desktop browser and the app already turns off shadows and animations
     on Linux WebKit; on machines whose primary GPU is NVIDIA, WebKitGTK's
     hardware compositing can silently fail, and setting
     `__EGL_VENDOR_LIBRARY_FILENAMES` to the Mesa vendor file before
     launching works around it — give the exact line and say the app does
     not set it for you;
   - where the app keeps its data (`~/Library/Application Support/
     io.github.x7c1.delta/` on macOS, `~/.local/share/io.github.x7c1.delta/`
     on Linux; `delta.db` and `sessions/`), that sessions live on Delta's
     own tmux socket and survive an app restart, and how to remove
     everything;
   - a short "Updating" note: download the new bundle and replace the app;
     the database is migrated forward on first launch, and a database
     written by a newer version refuses to open with an older one (link
     the existing text in `docs/guides/development/local-run.md` or
     `compatibility.md` rather than restating it).
2. **`README.md`.** Rewrite `Getting started` to lead with the app:
   download from the latest Release, open it (link `docs/guides/install.md`
   for the unsigned-app steps and the Linux notes), start a session.
   Keep the source path as a second, developer-oriented paragraph
   (`git clone`, `make dev`, link to `docs/guides/development/README.md`).
   Remove the "distributed as source only — there are no prebuilt
   binaries yet" sentence. Keep the file to overview and command
   reference, per the repository's documentation rules; details go in the
   guide.
3. **Cross-links.** `docs/guides/development/README.md` and
   `docs/guides/release.md` link to the install guide where they mention
   the app or the bundles; nothing is duplicated between them.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make check` is green (documentation-only change; no code).

### Manual / on-hardware (verified by a human before merge)

- [ ] Following `docs/guides/install.md` on a Mac with a fresh download of
      the Apple-silicon `.dmg` gets from "damaged / blocked" to a running
      `Delta.app` using each of the two documented workarounds.
- [ ] Following the Linux section on a Linux machine with the `.deb` or
      the `.AppImage` launches the app; on an NVIDIA-primary machine the
      documented environment variable restores hardware compositing.

## Out of scope

- Signing and notarization; auto-update.
- Any change to the bundles, the workflows or the shell.
