# Release

How a Delta release is cut. Releases are driven by a single rolling pull
request kept in sync with `main`; marking it ready for review is the decision
to release, and merging it cuts the release.

## Overview

The mechanics of the release PR and of tagging and publishing are not in this
repository: they are the reusable workflows of
[colophon](https://github.com/x7c1/colophon), pinned to v0.3.0, which this
repository calls from three small workflows of its own (see
[Workflows involved](#workflows-involved)). colophon's
[usage guide](https://github.com/x7c1/colophon/blob/v0.3.0/docs/usage.md)
describes the flow those workflows run — the release PR's title and version,
its body with the summary above the `<!-- changelog:auto -->` marker, the
allowed title transitions, the draft, and what a merge does — and this guide
does not repeat it. What follows is what is particular to Delta.

The version lives in one place: `[workspace.package].version` in
`backend/Cargo.toml`, which every crate of the backend workspace inherits.
Versions are semver (`Release vX.Y.Z`, tag `vX.Y.Z`). Between colophon
deciding that a commit is a release and colophon publishing it, the `Release`
workflow builds the desktop bundles, signs them with a key only a dedicated
job can read, and attests their build provenance; the Release is published
with every bundle and its signature already attached. The jobs that build
hold no write access and never see the signing key; see
[Desktop bundles](#desktop-bundles) and [Signing](#signing).

## Normal flow

1. Every push to `main` opens or refreshes one PR titled `Release vX.Y.Z` on a
   `release/...` branch, labelled `release`. By default `X.Y.Z` is the last
   tag patch-bumped; to cut a minor or major release, edit the PR title
   (`Release v0.1.1` → `Release v0.2.0`), and the refresh that edit starts
   keeps it. The PR's one commit sets the workspace version to `X.Y.Z` in
   `backend/Cargo.toml` and `backend/Cargo.lock` (see
   [Setting the version](#setting-the-version)).
2. **The PR opens as a draft.** An open release PR does not mean anyone has
   decided to release it. While it is a draft, `validate / release-pr` passes
   without judging it, and CI and the bundles skip it (see
   [CI on the draft](#ci-on-the-draft)).
3. Write the release summary in the PR body, above the
   `<!-- changelog:auto -->` marker, and delete the
   `<!-- release-summary:todo -->` comment. The summary becomes the body of
   the GitHub Release; the per-commit changelog below the marker is
   regenerated on every refresh.
4. **When you decide to release, mark the PR ready for review.** That runs CI,
   the bundles and `validate / release-pr` on it; the last fails while the
   title is not an allowed next version or the summary is unwritten.
5. **Any refresh turns the PR back into a draft** — a push to `main`, a title
   edit, or a manual run — because the content you decided on has changed.
   Review the new content and mark it ready again.
6. Squash-merge the PR. When CI passes on the merge commit, the `Release`
   workflow builds, signs and attests the desktop bundles, then tags the
   commit `vX.Y.Z` and publishes the GitHub Release with the bundles and
   their signatures attached.

## Setting the version

colophon writes the version with its `command` writer, which runs
`scripts/set-workspace-version.sh` with the version the title names. The
script sets `[workspace.package].version` in `backend/Cargo.toml` and runs
`cargo update --workspace`, so that `backend/Cargo.lock` records every
workspace crate at the new version and a `--locked` build accepts it. It runs
on colophon's runner, which carries nothing beyond the runner image, so it
installs the toolchain `rust-toolchain.toml` names itself and does not use
cargo-edit; its header says how and why. `make set-workspace-version-test`
runs it against throwaway workspaces, in `make check` and in CI's backend
job.

The title is the single source of truth for the next version; you never edit
`Cargo.toml` by hand.

## CI on the draft

colophon cannot keep this repository's own CI off the draft release PR, so the
workflows here skip it themselves. In `ci.yml` and `bundle.yml`, every job but
`all-green` carries the condition

```yaml
if: ${{ !(github.event_name == 'pull_request' && github.event.pull_request.draft && startsWith(github.head_ref, 'release/')) }}
```

and both workflows run on `ready_for_review` besides the default
`pull_request` types, so marking the PR ready runs everything on it. When
`release.yml` calls `bundle.yml`, the event is `release.yml`'s own
(`workflow_run` or `workflow_dispatch`), so the condition never stops a
release build. `all-green` accepts skipped jobs, so the required checks pass
on the draft. Other draft PRs still run CI.

## Desktop bundles

Every Release carries bundles of the desktop shell
(`backend/crates/apps/delta-desktop`), built by `.github/workflows/bundle.yml`,
each with its minisign signature, `<file>.minisig`, beside it
([Signing](#signing)):

| File | Platform |
|---|---|
| `Delta_<version>_aarch64.dmg` | macOS, Apple silicon |
| `delta-desktop_<version>_amd64.deb` | Linux (Debian/Ubuntu), x86_64 |

The version in each file name is the workspace version Tauri reads from the
shell crate. The tag comes from the release PR's title instead, so `target`
refuses a release whose commit carries another version (see
[Recovery](#recovery)). The bundles are not
code-signed by Apple's or a distribution's scheme, nor notarized; what that
means for someone installing them is in [the install guide](install/README.md).

The `Release` workflow (`.github/workflows/release.yml`) runs as a chain of
jobs, each holding only the access its step needs:

1. **`plan`** (colophon's `release-plan.yml`; read-only) considers only a
   green CI run started by a push to `main`, and decides whether the commit
   that run tested (`workflow_run.head_sha`, not whatever `main` points at by
   then) is the squash of a merged PR labelled `release`; if so, it reads the
   version from the PR's title. It fails, before anything is built, when the
   PR carries no summary.
2. **`target`** (read-only) names the commit to build, its version and the
   bundle file names in the table above. On a release it fails, before
   anything is built, unless the workspace version at that commit is the
   version `plan` read from the title.
3. **`bundles`** (read-only) calls `bundle.yml`, which builds each platform
   from that commit and uploads the bundle as a workflow artifact. The build
   jobs get no write token and keep no git credentials, so nothing they run —
   an action, a build script, a dependency's install-time code — can change a
   Release or push. A release build restores no compiled `backend/target`
   cache, only the downloaded crate sources.
4. **`sign`** (no token; the `release-signing` environment) signs each file
   in the table above with the release key and uploads the signatures as a
   workflow artifact ([Signing](#signing)).
5. **`assets`** (`id-token: write` and `attestations: write` for the
   attestations, and no write access to the repository) downloads the
   bundles and the signatures, fails unless every file in the table above and
   its `.minisig` is there and nothing else is, records a GitHub artifact
   attestation of each bundle's build provenance, and uploads them all as the
   one artifact `release-assets`. It runs no project code and no third-party
   action besides `actions/download-artifact`,
   `actions/attest-build-provenance` and `actions/upload-artifact`.
6. **`publish`** (colophon's `release-publish.yml`; `contents: write`,
   `pull-requests: write`) tags the commit and creates the Release with every
   file of `release-assets` attached in one `gh release create`, so the
   Release is published only once every bundle and signature is on it; the
   desktop app's update check never sees one without them. It then points
   the release PR's compare link at the tag.

- **A failed build** stops the run before `publish`: no tag exists and the
  desktop app sees no new Release. Open the failed `Release` run in the
  Actions tab and use "Re-run failed jobs": the failed platforms build again,
  `sign` signs them, `assets` gathers and attests them, and `publish`
  publishes the Release. If a run fails after the Release was created, a
  re-run keeps the Release and replaces its assets. To rebuild a ref without
  any Release, run the `Bundle` workflow by hand (`workflow_dispatch`) with
  that ref; it uploads workflow artifacts only.
- **Dry run.** Run the `Release` workflow by hand (`workflow_dispatch`, with
  `dry_run` checked, which is the only accepted value) to exercise the build
  and signing from the dispatched ref without releasing anything. It builds
  that ref, signs the bundles with the release key under the dry run's own
  version (so `sign` is exercised end to end; dispatch it from a protected
  branch such as `main`, since only those may use the `release-signing`
  environment), and runs the check in `assets`. It skips the attestation and
  ends at workflow artifacts: the bundles (`delta-*`), their signatures
  (`bundle-signatures`) and both together (`release-assets`). It creates no
  Release, draft or otherwise, and never calls colophon.
- **Pull requests.** A pull request that touches the shell or its build
  inputs (the `paths` filter in `bundle.yml`) runs the same builds and uploads
  each platform's bundles as a workflow artifact (`delta-macos-aarch64`,
  `delta-linux-x86_64`), so a broken bundle is caught before a release
  depends on it and reviewers can download and try it. A draft release PR
  runs none of them until it is marked ready for review (see
  [CI on the draft](#ci-on-the-draft)).

## Signing

Every bundle is signed with [minisign](https://jedisct1.github.io/minisign/)
(Ed25519), and the desktop app and its Linux update helper refuse an update
whose signature does not verify ([security guide](security.md#signed-release-bundles)).
GitHub's sha256 digest of an asset only proves the download is what was
uploaded; the signature proves the release pipeline produced it, so the key
must be out of reach of everything that builds.

- **Where the key lives.** The secret key is the secret
  **`MINISIGN_SECRET_KEY`** of the GitHub Environment **`release-signing`**,
  which a maintainer created and restricted to protected branches; the
  workflow only references it. It holds the key file `minisign -G -W` writes
  (the bare key line is accepted too). The key has no password, since CI
  signs unattended; the environment is what guards it.
- **The `sign` job.** Only `sign` names the environment. It has
  `permissions: {}`, checks nothing out and runs no project code; it
  downloads the bundle artifacts, installs minisign from a pinned release
  archive whose sha256 is written in the workflow, and signs exactly the
  bundle names `target` lists, with the trusted comment
  `file:<bundle> version:<version>` (the app refuses a signature naming
  another file or version). On a dry run `<version>` is the dry run's own,
  `0.0.0-dryrun.<run id>`, not the version the bundles are built as: a
  dry-run bundle signed with the release key must not carry a signature the
  app accepts when it installs that version. It then verifies each signature against the
  public key written in the workflow — the one the app compiles in — so a
  secret that does not match the app's key fails the run instead of every
  user's update, and uploads the signatures as the `bundle-signatures`
  artifact. `assets` fails when a bundle lacks its signature, so none reaches
  the Release without one.
- **The public key** is `PUBLIC_KEY` in `backend/crates/libs/release-signature`
  (key id `C2B89994B1AA8DAF`), the only place the code holds it; the app and
  the helper both read it from there, and a test fails if it is written
  anywhere else under `backend/crates/`. The workflow and the install guide
  print it too.
- **Attestations.** `assets` records a GitHub artifact attestation of
  each bundle (`actions/attest-build-provenance`), which ties it to the
  workflow run and commit that built it; users can check it with
  `gh attestation verify <file> -R x7c1/delta`
  ([install guide](install/README.md#checking-a-download)). Only `assets`
  holds `id-token: write` and `attestations: write`, and it neither checks out
  nor runs project code. A dry run skips it.
- **The first signed release.** Releases published before signing began
  carry no `.minisig`. An app that checks signatures updates only to releases
  published after the change, all of which carry them; the release it would
  refuse — an older, unsigned one — is never newer than itself, so it is
  never offered. Apps built before the change keep checking only the digest
  until they update.

### Rotating the key

The app accepts only the key it was built with, so a new public key reaches
users inside an app release signed with the old key:

1. Generate the new key pair (`minisign -G -W`) and keep the new secret key
   out of the repository.
2. Change `PUBLIC_KEY` in `release-signature` to the new public key, and cut
   that release as usual. `sign` still signs it with the old key (and checks
   it against the old `MINISIGN_PUBLIC_KEY` in `release.yml`), so installed
   apps accept it; once they run it, they expect the new key. The guides
   keep printing the old key, which is still the one that verifies the
   bundles by hand.
3. Before the next release, in one change, replace `MINISIGN_SECRET_KEY` in
   the `release-signing` environment with the new secret key,
   `MINISIGN_PUBLIC_KEY` in `release.yml` with the new public key, and the
   key printed in the install and security guides with the new one, naming
   the old key as the one for releases up to the release of step 2.

An app still older than the release of step 2 refuses releases signed with
the new key and shows the reason with the release page; its user updates by
hand once. If the secret key is lost, or leaked and must not sign anything
more, there is no trusted old key to sign the hand-over release: do steps 2
and 3 together, so the release carrying the new key is signed with it, and
every user updates by hand once
([install guide](install/README.md#checking-a-download)); after a leak, say
so in the release notes.

## Workflows involved

- `.github/workflows/release-pr.yml` — calls colophon's `release-pr.yml`,
  which opens or refreshes the release PR as a draft on every push to `main`
  (but not the one that lands a release), when the release PR's title is
  edited, and on a manual run on `main`. It writes the version with
  `scripts/set-workspace-version.sh`.
- `.github/workflows/validate-release-pr.yml` — calls colophon's
  `validate-release-pr.yml` from its job `validate`, so the check reads
  `validate / release-pr`. It passes on every PR that is not a release PR and
  on a draft one; on a release PR marked ready for review it enforces the
  allowed title transitions and fails while the summary is unwritten.
- `.github/workflows/release.yml` — when CI completes on `main`, colophon's
  `release-plan.yml` (job `plan`) decides whether the commit CI tested is a
  release; if it is, the bundles are built, signed and attested, and
  colophon's `release-publish.yml` (job `publish`) tags the commit and
  publishes the Release with them. Run by hand, it performs a dry run. See
  [Desktop bundles](#desktop-bundles) and [Signing](#signing).
- `.github/workflows/bundle.yml` — builds the desktop bundles on macOS
  (Apple silicon only) and Linux with read-only access and uploads them as
  workflow artifacts, on pull requests, manual runs and for `release.yml`.

Every colophon call is pinned to the commit of colophon v0.3.0, as the other
actions here are pinned (`make check-workflow-pins`); Dependabot proposes
updates to it with the other GitHub Actions.

## Recovery

- **`Release PR` failed.** Re-run it from the Actions tab, or run the workflow
  by hand on `main`. Each run rebuilds the release branch from `main`.
- **Abandoning a release PR.** Close it and delete its branch; the next push
  to `main` opens a fresh one with the default patch-bumped title.
- **`Release` failed because the summary was missing.** `plan` fails before
  anything is built or tagged. Write the summary into the merged release PR's
  body (a merged PR's description is still editable) and re-run the failed
  run from the Actions tab. A re-run judges the same commit as the original
  run (`workflow_run.head_sha`), even after other commits have landed on
  `main`.
- **`Release` failed because the versions differ.** `target` fails when the
  merged commit's workspace version is not the one the release PR's title
  names, which happens when the refresh after a title edit failed and the PR
  was merged on its old commit. Nothing was built or tagged, and re-running
  does not help. Run `Release PR` by hand on `main` to open a new release PR,
  give it the version you meant, and merge it once the refresh that title
  edit starts has succeeded.
- **`Release` failed later.** Use "Re-run failed jobs" (see
  [Desktop bundles](#desktop-bundles)). colophon keeps a Release that already
  exists and replaces its assets.
- **CI was red on `main`.** The `Release` workflow considers only a green CI
  run, so nothing was tagged. CI runs again on the commit that fixes it, but
  that commit is not the squash of the release PR; re-run the failed CI run
  of the release commit instead. When that re-run turns green, its
  completion starts a new `Release` run. Re-running the `Release` run the red
  CI started does not help: a re-run replays the original event, which still
  says CI failed.

## Release automation setup

The release PR is opened as a **GitHub App**, so that CI and
`validate / release-pr` run on its refreshes too: GitHub starts no workflows
for events caused by the default `GITHUB_TOKEN`. The App's private key is
turned into a token for this repository only, valid for an hour, so there is
no long-lived token to rotate.

**One-time setup, by a person with admin access to the repository:**

1. Create the GitHub App and install it on this repository, as colophon's
   [usage guide](https://github.com/x7c1/colophon/blob/v0.3.0/docs/usage.md#tokens)
   describes (Contents and Pull requests: Read and write).
2. Add the App's Client ID as the Actions variable `RELEASE_APP_CLIENT_ID`,
   and its private key as the Actions secret `RELEASE_APP_PRIVATE_KEY`.
3. Allow squash merge, which the release relies on: the squash's default
   title (the PR title) is how a release landing is recognized.
4. Require the checks below in the ruleset of `main`.

Set both the variable and the secret: with the variable but no secret,
`Release PR` fails because colophon cannot create the App's token; with
neither, colophon falls back to `GITHUB_TOKEN`, which can open the release PR
only if the repository's Actions settings allow GitHub Actions to create pull
requests, and CI does not run on the PR's refreshes. Once the App is set up,
run the `Release PR` workflow by hand on `main` to open the release PR without
waiting for the next push.

The `Release` and `Bundle` workflows use the default `GITHUB_TOKEN`: they run
after a merge a person made, and no workflow here listens to tag pushes or
Release events, so nothing has to be triggered by what they create. Only
`publish` gets write access to the repository's contents (see
[Desktop bundles](#desktop-bundles)), and only `sign` may read the
`release-signing` environment's `MINISIGN_SECRET_KEY` ([Signing](#signing)).

## Required checks

| Check | Workflow | Passes when |
|---|---|---|
| `all-green` | `ci.yml` | every CI job passed or was skipped (all are skipped on a draft release PR) |
| `validate / release-pr` | `validate-release-pr.yml` | the title and summary are valid, or the pull request is not a release PR, or it is a draft |

A new CI job goes into the `needs` of `all-green` and carries the draft
release PR condition (see [CI on the draft](#ci-on-the-draft)); the required
checks stay as they are.
