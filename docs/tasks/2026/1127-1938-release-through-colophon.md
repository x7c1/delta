---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && ! git grep -q -e RELEASE_PAT -e check-version-change -e "\.github/scripts" -e create-release-pr -- ":!docs/tasks/"'
assignee: null
branch: task/1127-1938-release-through-colophon
created_at: 2026-11-27T10:38:33Z
updated_at: 2026-11-27T11:00:53Z
---

# ci: release through the shared colophon workflows

## Overview

Delta's release automation is a private copy of what the reusable workflows in
[x7c1/colophon](https://github.com/x7c1/colophon) now provide:
`create-release-pr.yml`, `validate-release-pr.yml`, the release-deciding part
of `release.yml`, and the scripts under `.github/scripts/`. Replace the copy
with calls to colophon v0.3.0, so that release mechanics are maintained in one
place. Read colophon's `docs/usage.md` at v0.3.0 first; the callers below
follow its examples. A sibling repository, x7c1/coffret, made the same move in
its PR #511 (`ci: release through the shared colophon workflows`); its
`scripts/set-workspace-version.sh`, its test and its workflows are a working
reference for the parts delta shares with it (a cargo workspace under
`backend/`, desktop bundles attached after the Release).

Pin every colophon call to the v0.3.0 commit
`a4d0af8d30a36d9978d393e28b540bdb3c641c6f` with a `# v0.3.0` comment, as the
repository pins other actions (`scripts/check-workflow-pins.sh`).

With colophon, the release PR opens as a draft and every refresh turns it back
into one. Marking it ready for review is the decision to release. While it is
a draft, validate passes without judging it, and delta's CI skips it.

What must survive the move is delta's release security model, described in
`docs/guides/release.md` ("Desktop bundles", "Signing") and the header of
`release.yml`: jobs that build hold read access only; only the `sign` job can
read `MINISIGN_SECRET_KEY` (the `release-signing` environment), and it holds
no token and runs no project code; no bundle reaches a published Release
without its signature; the bundles' build provenance is attested; and the
desktop app's update check never sees a published Release that lacks a bundle
or a signature.

### Release PR (`release-pr.yml` replaces `create-release-pr.yml`)

- Call `x7c1/colophon/.github/workflows/release-pr.yml` on the same triggers as
  today (`push` to `main`, `pull_request` `edited`, `workflow_dispatch`) with
  `version-scheme: semver`.
- Open the PR as the GitHub App: `app-client-id: ${{ vars.RELEASE_APP_CLIENT_ID }}`
  and the `app-private-key` secret from `secrets.RELEASE_APP_PRIVATE_KEY`.
  `RELEASE_PAT` is no longer used; remove every reference to it.
- Write the version with colophon's `command` writer. The command runs a new
  `scripts/set-workspace-version.sh`, which sets the workspace version in
  `backend/Cargo.toml` (`[workspace.package]`) and keeps `backend/Cargo.lock`
  consistent with it, as `cargo set-version --workspace` plus the lock update
  in `.github/scripts/create-release-pr.sh` do today. The command runs inside
  colophon's job on a fresh `ubuntu-latest` runner, with the repository checked
  out at its root and nothing installed beyond the runner image; there is no
  step before it to install a toolchain or a cargo subcommand. So the script
  must work there: if it needs the toolchain named by `rust-toolchain.toml` it
  installs it itself (`rustup` does not install it on first use), and it must
  not depend on `cargo-edit`. Say in the script's header why it takes the
  approach it does.
- Add `make set-workspace-version-test`, in `scripts/tests/` beside the other
  script tests, and add it to `CHECK_STEPS` (and the step graph comment above
  it) so `make check` runs it. It runs the script against throwaway workspaces
  and checks that the workspace version, the version of `delta-server` as
  `cargo metadata` reports it, and the `Cargo.lock` entries of the workspace
  crates all become the new version, and that `cargo metadata --locked
  --offline` then succeeds. Run it from CI wherever
  `make check-version-change-test` runs today.

### Validate (`validate-release-pr.yml`)

- Call `x7c1/colophon/.github/workflows/validate-release-pr.yml` with
  `version-scheme: semver`, on `pull_request` types `[opened, edited,
  synchronize, reopened, ready_for_review]`. Name the calling job `validate`,
  so the check reads `validate / release-pr`. It replaces today's
  `validate-title` check.

### Release (`release.yml`)

Keep the `workflow_run` trigger on CI for `main`. Colophon decides the release
and publishes it; delta keeps building, signing and attesting the bundles in
between:

1. `plan` — colophon's `release-plan.yml` with `version-scheme: semver`. A
   commit is a release when it is the squash of a merged PR labelled
   `release`; this replaces `scripts/check-version-change.sh` (the workspace
   version went up). Check colophon's gating against what `plan` does today
   (a green CI run, the commit CI tested rather than the branch tip) and keep
   anything a caller still needs.
2. `bundles` — `./.github/workflows/bundle.yml` at the planned `sha` with
   `release: true`, read access only, as today.
3. `sign` — unchanged in what it may touch: the `release-signing` environment,
   `permissions: {}`, no checkout, the pinned minisign archive, the check
   against the app's public key, and the trusted comment
   `file:<name> version:<version>`. It signs exactly the bundle names derived
   from the version (`Delta_<v>_aarch64.dmg`, `delta-desktop_<v>_amd64.deb`),
   as `plan`'s "Name the bundles" step does today.
4. A job that downloads the bundles and their signatures, fails unless every
   expected bundle and its `.minisig` are present, attests the bundles' build
   provenance (`id-token: write`, `attestations: write`, no `contents: write`),
   and uploads all of them as one artifact for colophon.
5. `publish` — colophon's `release-publish.yml` with `tag`, `sha` and
   `pr-number` from `plan`, and `artifact-name` naming that artifact. Colophon
   creates the Release with its assets in one `gh release create`, so the
   Release is published only once every file is attached. It also rewrites
   the release PR's compare link to the tag, which replaces the
   `rewrite-links` job and `.github/scripts/update-release-pr-links.sh`.

Keep the dry run on `workflow_dispatch` (still refusing `dry_run: false`), but
stop it at artifacts: it builds the bundles of the dispatched ref, signs them
with the dry run's own version `0.0.0-dryrun.<run id>` exactly as today (a
dry-run bundle signed with the release key must never carry a signature the
app would accept as a real release), runs the presence check, skips the
attestation, and leaves the bundles and signatures as workflow artifacts. It
creates no Release, draft or otherwise, and does not call colophon. Today's
`draft` job, and the draft Release a dry run leaves behind, go away.

Remove what nothing uses any more: `scripts/check-version-change.sh`,
`scripts/tests/check-version-change.test.sh`, the
`check-version-change-test` Makefile target and its place in `CHECK_STEPS`
and CI, and every file under `.github/scripts/` that colophon replaces
(including `release-summary.test.sh` and any CI step that runs it).

### CI on a draft release PR

- In `ci.yml` and `bundle.yml`, add `ready_for_review` to the `pull_request`
  types (keeping the existing `branches` and `paths` filters and the default
  types `opened, synchronize, reopened`). Skip every job except `all-green` on
  a draft PR whose branch starts with `release/`, with the condition
  colophon's usage shows:
  `!(github.event_name == 'pull_request' && github.event.pull_request.draft && startsWith(github.head_ref, 'release/'))`.
  Where a job already has an `if:`, combine the two. In `bundle.yml` the
  condition must not stop the job when `release.yml` calls it
  (`workflow_call`). `all-green` must keep passing when the jobs it needs are
  skipped; confirm its condition still holds.

### Docs

- Rewrite `docs/guides/release.md` for the new flow: colophon decides and
  publishes the release; the release PR opens as a draft; mark it ready for
  review when you decide to release, which runs CI and validate; a refresh
  turns it back into a draft; the App setup (`RELEASE_APP_CLIENT_ID`
  variable, `RELEASE_APP_PRIVATE_KEY` secret, the App installed on the
  repository) replaces the `RELEASE_PAT` setup; the required checks are
  `all-green` and `validate / release-pr`; the dry run now ends at workflow
  artifacts. Keep the signing and provenance sections accurate to the new job
  layout. Point to colophon's `docs/usage.md` instead of repeating it. Update
  any other document, comment or Makefile help line that names a removed file,
  the PAT, the `draft` job or `validate-title`.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make set-workspace-version-test` is part of `make check` and passes: the script sets the workspace version and `Cargo.lock` consistently, and `cargo metadata --locked --offline` succeeds afterwards
- [x] `make check-workflow-pins` passes, so every action and colophon call is pinned to a commit SHA
- [x] No file outside `docs/tasks/` mentions `RELEASE_PAT`, `check-version-change`, `.github/scripts` or `create-release-pr` (the `git grep` gate in `check_command`)

### Before merge (verified outside the check command)

- [x] `actionlint` (for example `docker run --rm -v "$PWD:/repo" -w /repo rhysd/actionlint:latest`) reports nothing on `.github/workflows/`
- [x] Every colophon call is pinned to `a4d0af8d30a36d9978d393e28b540bdb3c641c6f` with a `# v0.3.0` comment, and passes the inputs and secrets its v0.3.0 `docs/usage.md` documents
- [x] Reading `release.yml`: only `sign` names `release-signing` and it has `permissions: {}` and no checkout; no job that builds or checks out project code holds `contents: write`, `id-token: write` or `attestations: write`; the artifact handed to colophon holds every bundle and its `.minisig`; a dispatched run never reaches colophon's `publish` and signs with `0.0.0-dryrun.<run id>`
- [x] In `ci.yml` and `bundle.yml`, every job other than `all-green` carries the draft-release skip, both `pull_request` triggers list `ready_for_review`, the skip does not apply when `release.yml` calls `bundle.yml`, and `all-green` still passes when the jobs it needs are skipped
- [x] `docs/guides/release.md` describes the draft flow, the App setup, the required checks, the artifact-only dry run and the new signing job layout, with no remaining description of the PAT, the removed scripts or the draft Release
- [x] The GitHub App `x7c1-release-bot` is installed on x7c1/delta, the variable `RELEASE_APP_CLIENT_ID` and the secret `RELEASE_APP_PRIVATE_KEY` are set, and the ruleset of `main` requires `validate / release-pr` instead of `validate-title` (needs a person: the App installation and the secrets need the account owner)
