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
branch: task/0930-0400-build-build-the-desktop-bundles-in-actions-and-attach-them-to-the-github-release
created_at: 2026-09-29T17:50:00Z
updated_at: 2026-09-29T19:48:42Z
---

# build(release): build the desktop bundles in Actions and attach them to the GitHub Release

## Overview

The desktop shell (`backend/crates/apps/delta-app`, a Tauri v2 crate that
runs `delta-server` in-process; `make app` bundles it locally) is only
useful to people who never build Delta if the bundles come with each
release. The `Release` workflow (`.github/workflows/release.yml`) today
runs after a green CI on `main`, detects a workspace version bump, and
creates the `vX.Y.Z` tag and GitHub Release with the summary from the
release PR. This task adds the bundle builds to that flow and makes the
same builds runnable on a pull request, so a broken bundle is caught
before a release depends on it. The bundles are unsigned; signing and
notarization are deliberately not part of this.

### Change

1. **A reusable bundle workflow.** Add `.github/workflows/bundle.yml`
   with a matrix of three entries — `macos-14` for Apple silicon
   (`aarch64-apple-darwin`), `macos-14` cross-compiling for Intel
   (`x86_64-apple-darwin`; GitHub has retired its Intel macOS images) and
   `ubuntu-22.04` (`.deb` and `.AppImage`) — that checks out the
   requested ref, installs the Rust toolchain with the matrix target, sets
   up pnpm and Node the way `ci.yml` does, installs the frontend
   dependencies, runs `make web-dist`, installs the Linux system packages
   the shell needs on the Ubuntu runner (the same list `ci.yml` uses for
   the shell's build step), and runs `tauri-apps/tauri-action` with
   `projectPath` pointing at the shell crate and `args` selecting the
   matrix target. Cache cargo the way `ci.yml` does. Three triggers:
   - `workflow_call` with inputs `ref` (the tag to build) and
     `release_id`; when `release_id` is set, `tauri-action` uploads the
     bundles to that existing Release (do not let it create a Release or
     a tag of its own);
   - `pull_request` limited by `paths` to the shell crate, this workflow,
     the Makefile and the frontend — the run uploads the bundles as
     workflow artifacts (`actions/upload-artifact`, one artifact per
     matrix entry, using `tauri-action`'s `artifactPaths` output) so a
     reviewer can download and try them;
   - `workflow_dispatch` with the same `ref` input, uploading artifacts
     the same way.
2. **Hook it into the Release workflow.** The `release` job exposes
   `changed`, `version` and the Release's id (the `softprops/action-gh-release`
   step's `id` output) as job outputs; a second job `bundles` runs when
   `changed == 'true'`, `needs: release`, and calls `bundle.yml` with
   `ref: v<version>` and that `release_id`. Give it `contents: write`.
   The Release is created first and the bundles attach afterwards, so a
   bundle failure never blocks the tag; it shows as a failed run to rerun.
3. **Versioning.** The bundle file names carry the version Tauri reads
   from the crate (the workspace version), so they match the tag by
   construction. Add a check to `scripts/check-version-change.sh`'s test
   or a small script test under `scripts/tests/` only if the version
   flows through something new; otherwise no code change here.
4. **Docs.** `docs/guides/release.md`: a "Desktop bundles" section
   listing what a Release carries (the two macOS `.dmg`, the `.deb` and
   `.AppImage`), that they are unsigned, how to rerun the `bundles` job
   if it failed, and that a pull request touching the shell builds the
   same bundles as artifacts. Add `bundle.yml` to the "Workflows
   involved" list. Do not write the end-user install instructions here
   (a later step adds a guide for opening the unsigned app and rewrites
   the README).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make check` is green (this task adds workflows and docs; any
      script it touches keeps its stubbed tests passing).

### Manual / on-hardware (verified by a human before merge)

- [ ] The pull request's `bundle` run finishes green on all three runners
      and its artifacts contain an Apple-silicon `.dmg`, an Intel `.dmg`,
      a `.deb` and an `.AppImage` whose file names carry the workspace
      version.
- [ ] The Apple-silicon `.dmg` from those artifacts, after the quarantine
      workaround for an unsigned app, launches `Delta.app` on a Mac to the
      session list.

## Out of scope

- Code signing and notarization, auto-update, Windows.
- The end-user install guide and the README rewrite.
- Any change to how the release PR, the tag or the Release body are
  produced.
