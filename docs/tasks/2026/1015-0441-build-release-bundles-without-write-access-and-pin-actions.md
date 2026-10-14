---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && scripts/check-workflow-pins.sh && test -f .github/dependabot.yml"
assignee: null
branch: task/1015-0441-build-release-bundles-without-write-access-and-pin-actions
created_at: 2026-10-14T19:41:19Z
updated_at: 2026-10-14T20:21:33Z
---

# ci: build release bundles without write access, publish them from a separate job, and pin every action

## Overview

The release pipeline (`.github/workflows/release.yml` calling `bundle.yml`)
builds the desktop bundles in a job that holds `contents: write`, runs
third-party actions referenced by mutable tags (`@v4`, `@v1`, `@stable`), and
lets `tauri-apps/tauri-action` upload straight to a Release that is already
published. Whatever runs in that job — an action, a build script, a dev
dependency's install-time code — runs with a token that can change the
Release the desktop app updates from. Restructure the pipeline so that code
which builds never holds write access, and pin what runs.

### Release flow

1. **Release job** (`release.yml`): checks out the commit CI actually tested
   (`github.event.workflow_run.head_sha`, not the branch tip) and creates the
   Release as a **draft** whose tag targets that same SHA
   (`target_commitish`). Steps that need the tag to exist (rewriting the
   release PR's links) move after publishing.
2. **Build jobs** (`bundle.yml`, one per platform): `permissions: contents:
   read`, `actions/checkout` with `persist-credentials: false`, no
   `GITHUB_TOKEN` passed to `tauri-action`, no `releaseId`; they upload the
   bundles as workflow artifacts only. A release build restores no
   `backend/target` cache (a cache written by an ordinary `main` CI run must
   not feed a release binary); keeping the cargo registry cache is fine.
3. **Publish job** (new, in `release.yml`): `contents: write`, runs **no
   third-party action** besides the pinned `actions/download-artifact`, and no
   project code. It downloads the artifacts, uploads them to the draft with
   `gh release upload`, checks that every expected asset is present, then
   publishes the draft (`gh release edit --draft=false`). Then the PR-link
   rewrite runs.
4. A failed build leaves an unpublished draft that a re-run completes; update
   `docs/guides/release.md` ("Desktop bundles") to say so, replacing the
   current "the Release exists, re-run the bundles job" guidance.

Add a `workflow_dispatch` trigger to `release.yml` with a `dry_run` input for
verification: it builds from the dispatched ref, creates a draft under a tag
name that cannot collide with a real release (for example
`v0.0.0-dryrun.<run id>`), uploads the assets, and stops before publishing and
before the link rewrite. GitHub only offers `workflow_dispatch` for a workflow
whose default-branch copy declares it, so the dry run is exercised right after
this merges, before the next real release.

### Pinning and the release-PR job

- Every `uses:` in `.github/workflows/` references a full 40-character commit
  SHA with the version in a trailing comment (`# v4.2.2`). Add
  `scripts/check-workflow-pins.sh`, which fails naming each `uses:` that is not
  a SHA (local `./` reusable workflows are exempt), and run it in `make check`
  (`CHECK_STEPS`) and in CI.
- `create-release-pr.yml`: install `cargo-edit` at a fixed version
  (`cargo install cargo-edit@<version> --locked`).
- `dtolnay/rust-toolchain` is pinned like any other action; keep the toolchain
  input explicit (`toolchain: stable`) since the ref no longer names it.

### Dependency monitoring

- `.github/dependabot.yml`: weekly updates for `github-actions` (which keeps
  the pinned SHAs current), `cargo` (`/backend`), and `npm` (`/frontend`).
- A CI job `audit` runs `cargo deny check advisories` (with a `deny.toml` at
  `backend/` that states how unmaintained and yanked crates are treated, and
  ignores nothing silently — any ignore carries a reason) and
  `pnpm audit --prod`. Add it to `make check` as well if it runs offline-safe;
  otherwise CI only, and say so in `docs/guides/development/README.md`.
- Update `rustls` to a version without the current advisories
  (`cargo update -p rustls`), and run `cargo update` for the other crates with
  advisories (anyhow, yoke-derive) where a compatible fix exists.

### Docs

`docs/guides/release.md`: the new flow (draft → build → publish), the dry
run, and that the build jobs hold no write access.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `scripts/check-workflow-pins.sh` passes: every `uses:` under
      `.github/workflows/` is a 40-hex SHA (local reusable workflows exempt),
      and the script is part of `make check`.
- [x] A test for the pin checker (a temporary directory holding a workflow
      with a tag reference and one with a SHA) shows it fails on the first
      and passes on the second; it runs in `make check`.
- [x] `.github/dependabot.yml` exists and covers `github-actions`, `cargo`,
      and `npm`.
- [x] `make check` passes.

### Before merge (verified outside the check command)

- [ ] Reading the workflows: no job that runs a build step, `pnpm install`,
      or `cargo` holds `contents: write`; every `actions/checkout` sets
      `persist-credentials: false` except where a later step pushes with the
      checkout's credentials, which is named in a comment.
- [ ] The CI `audit` job passes on the PR.
