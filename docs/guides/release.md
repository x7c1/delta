# Release

How to cut a Delta release. Releases are driven by a single rolling pull
request that the bot keeps in sync with `main`; cutting a release is just
merging that PR.

## Overview

Delta uses a "merge the PR" release model. A bot opens and updates one
rolling release PR against `main`; merging it triggers the `Release`
workflow, which creates a draft GitHub Release for the commit CI tested,
builds the desktop bundles, signs them with a key only a dedicated job can
read, attaches them with their signatures, and only then publishes the
Release, which creates the matching `vX.Y.Z` tag. The Release body is a
summary written by hand in the release PR; the generated per-commit changelog
stays on the PR. The jobs that build hold no write access and never see the
signing key; see [Desktop bundles](#desktop-bundles) and
[Signing](#signing).

## Normal flow (patch bump)

1. The `Create Release PR` workflow opens or refreshes a single open PR
   titled `Release vX.Y.Z` on every push to `main`. By default `X.Y.Z` is
   the last tag patch-bumped (e.g. `v0.1.0` → `Release v0.1.1`).
2. The PR body has two parts: a summary region you write by hand, and the
   changelog since the previous tag below it (auto-generated from `git log`
   via `generate-changelog.sh`). See [Release summary](#release-summary).
3. Write the summary. The PR cannot be merged while it is unwritten.
4. Merge the PR when you want to cut the release. The `Release` workflow
   then runs after CI completes on `main` and publishes a GitHub Release
   carrying that summary plus links back to the release PR and the compare
   view, with the desktop bundles attached; publishing creates the `vX.Y.Z`
   tag.

## Release summary

The release PR body is split in two by a marker line:

```markdown
## Summary

<!-- release-summary:todo -->
_Write the release summary here. It becomes the body of the GitHub Release.
Delete the marker comment above once written._

<!-- changelog:auto -->

## Features

- feat: ...
```

- Everything **above** `<!-- changelog:auto -->` is yours. The bot carries it
  through verbatim every time it regenerates the body, so it survives the
  force-pushes that rebuild the release branch on each push to `main`.
- Everything from the marker down is regenerated from `git log` on every push
  to `main`; edits made there are overwritten.

Because the summary is the body of the GitHub Release, the release is gated on
it: `Validate Release PR` fails while the summary is empty or still carries the
`<!-- release-summary:todo -->` sentinel. That workflow also runs on body
edits, so saving the summary re-runs the check and turns it green without any
further push. If a release nonetheless reaches the `Release` workflow without
a summary, that workflow fails before creating the draft rather than
publishing a Release without one.

## Promoting to minor or major

To cut a minor or major release, **edit the PR title only**:

- `Release v0.1.1` → `Release v0.2.0` (minor)
- `Release v0.1.1` → `Release v1.0.0` (major)

Saving the title edit triggers the bot immediately — you no longer have
to wait for the next push to `main`. (The next push works too, so either
path gets you there.) Body-only edits and no-op title saves are ignored,
so re-opening the edit dialog without changing the title does not
retrigger the bot. When the bot does run, it:

1. Reads the new title and extracts the target version.
2. Runs `cargo set-version --workspace <version>` to update
   `backend/Cargo.toml` and `Cargo.lock`.
3. Force-pushes the result to the existing release branch.
4. Updates the PR body's compare link.

The title is the single source of truth for the next version; you never
edit `Cargo.toml` by hand.

## Allowed title transitions

`validate-release-pr-title.sh` runs as a required check on every release PR
edit and enforces a strict semver progression against `last_tag`. Only
single-step bumps with the lower components reset are allowed:

| Last tag | PR title | Result |
|---|---|---|
| v0.3.10 | `Release v0.3.11` | ✅ patch |
| v0.3.10 | `Release v0.4.0` | ✅ minor |
| v0.3.10 | `Release v1.0.0` | ✅ major |
| v0.3.10 | `Release v0.3.9` | ❌ downgrade |
| v0.3.10 | `Release v0.4.1` | ❌ minor bump with non-zero patch |
| v0.3.10 | `Release v0.5.0` | ❌ minor skip |
| v0.3.10 | `Release v2.0.0` | ❌ major skip |

The PR cannot be merged until the title satisfies this validator (branch
protection requires the check).

## Branch naming

The release branch is named `release/since-<UTC %Y-%m-%d-%H%M>` (e.g.
`release/since-2026-06-23-1431`). The `since-` prefix labels it as the time
the branch was opened, not the release time.

The branch name is intentionally decoupled from the version it carries —
the version lives in the PR title and `Cargo.toml`, while the branch name
is fixed for the lifetime of the PR. Promoting the title from patch to
minor or major keeps reusing the same branch, so the PR's head pointer
stays in sync with every force-push.

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
shell crate, so it matches the tag by construction. The bundles are not
code-signed by Apple's or a distribution's scheme, nor notarized; what that
means for someone installing them is in [the install guide](install/README.md).

The `Release` workflow (`.github/workflows/release.yml`) runs as a chain of
jobs, each holding only the access its step needs:

1. **`plan`** (read-only) checks out the commit CI tested
   (`workflow_run.head_sha`, not whatever `main` points at by then), decides
   whether the version changed and reads the summary.
2. **`draft`** (`contents: write`) creates the Release as a **draft** whose
   tag will point at that commit (`target_commitish`). It runs only the `gh`
   CLI: no checkout, no project code. A draft is not visible to the desktop
   app's update check, and its tag does not exist yet.
3. **`bundles`** (read-only) calls `bundle.yml`, which builds each platform
   from that commit and uploads the bundle as a workflow artifact. The build
   jobs get no write token and keep no git credentials, so nothing they run —
   an action, a build script, a dependency's install-time code — can change a
   Release or push. A release build restores no compiled `backend/target`
   cache, only the downloaded crate sources.
4. **`sign`** (no token; the `release-signing` environment) signs each file
   in the table above with the release key and uploads the signatures as a
   workflow artifact ([Signing](#signing)).
5. **`publish`** (`contents: write`, plus `id-token: write` and
   `attestations: write` for the attestations) downloads the bundles and the
   signatures, checks that every bundle has its signature, attaches them all
   to the draft, checks that every file in the table above and its
   `.minisig` is attached, records a GitHub artifact attestation of each
   bundle's build provenance, and publishes the draft, which creates the tag.
   It runs no project code and no third-party action besides
   `actions/download-artifact` and `actions/attest-build-provenance`.
6. **`rewrite-links`** (`pull-requests: write`) points the release PR's
   compare link at the tag, which now exists.

- **A failed build** leaves the draft unpublished: no tag exists and the desktop
  app sees no new Release. Open the failed `Release` run in the Actions tab
  and use "Re-run failed jobs": the failed platforms build again, `sign`
  signs them, and `publish` then attaches them and publishes the same draft. Re-running
  the whole run is safe too: `draft` updates the draft it created rather than
  making a second one, and `publish` replaces any bundle already attached.
  To rebuild a ref without any Release, run the `Bundle` workflow by hand
  (`workflow_dispatch`) with that ref; it uploads workflow artifacts only.
- **Dry run.** Run the `Release` workflow by hand (`workflow_dispatch`, with
  `dry_run` checked, which is the only accepted value) to exercise the whole
  chain from the dispatched ref without releasing anything. It builds that
  ref, creates a draft under the tag name `v0.0.0-dryrun.<run id>` — a name no
  release uses, never created as a tag because the draft is never published —
  signs the bundles with the release key (so `sign` is exercised end to end;
  dispatch it from a protected branch such as `main`, since only those may
  use the `release-signing` environment), attaches and checks the bundles and
  their signatures, and stops before the attestation, before publishing and
  before the link rewrite. Inspect the draft on the Releases page, then delete
  it.
- **Pull requests.** A pull request that touches the shell or its build
  inputs (the `paths` filter in `bundle.yml`) runs the same builds and uploads
  each platform's bundles as a workflow artifact (`delta-macos-aarch64`,
  `delta-linux-x86_64`), so a broken bundle is caught
  before a release depends on it and reviewers can download and try it.

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
  bundle names `plan` lists, with the trusted comment
  `file:<bundle> version:<version>` (the app refuses a signature naming
  another file or version). On a dry run `<version>` is the dry run's own,
  `0.0.0-dryrun.<run id>`, not the version the bundles are built as: a
  dry-run bundle signed with the release key must not carry a signature the
  app accepts when it installs that version. It then verifies each signature against the
  public key written in the workflow — the one the app compiles in — so a
  secret that does not match the app's key fails the run instead of every
  user's update, and uploads the signatures as the `bundle-signatures`
  artifact. `publish` refuses to attach a bundle without its signature.
- **The public key** is `PUBLIC_KEY` in `backend/crates/libs/release-signature`
  (key id `C2B89994B1AA8DAF`), the only place the code holds it; the app and
  the helper both read it from there, and a test fails if it is written
  anywhere else under `backend/crates/`. The workflow and the install guide
  print it too.
- **Attestations.** `publish` also records a GitHub artifact attestation of
  each bundle (`actions/attest-build-provenance`), which ties it to the
  workflow run and commit that built it; users can check it with
  `gh attestation verify <file> -R x7c1/delta`
  ([install guide](install/README.md#checking-a-download)). Only `publish`
  holds `id-token: write` and `attestations: write`. A dry run skips it.
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

- `.github/workflows/create-release-pr.yml` — opens or updates the
  release PR on every push to `main` (skipped when the push is itself
  the merge of a release PR, to avoid looping). It also runs when a
  release-labelled PR's title is edited, so promoting the title is
  picked up immediately rather than waiting for the next main push.
- `.github/workflows/validate-release-pr.yml` — on every release PR edit,
  enforces the allowed title transitions above and fails while the release
  summary is unwritten.
- `.github/workflows/release.yml` — when CI completes successfully on
  `main`, checks whether the workspace version changed; if it did, reads the
  summary from the merged release PR, creates a draft Release, has
  `bundle.yml` build the desktop bundles, signs them, attaches them with
  their signatures, attests them and publishes the Release (which creates the
  tag). Run by hand, it performs a dry run. See
  [Desktop bundles](#desktop-bundles) and [Signing](#signing).
- `.github/workflows/bundle.yml` — builds the desktop bundles on macOS
  (Apple silicon only) and Linux with read-only access and uploads them as
  workflow artifacts, on pull requests, manual runs and for `release.yml`.

For the underlying setup (the `RELEASE_PAT` secret, why a user PAT is
required instead of `GITHUB_TOKEN`), see "Release automation setup" below.

## Recovery

- A failed `Create Release PR` run can be re-run from the Actions tab; the
  script is idempotent (it rebuilds the release branch from `origin/main`
  on every invocation).
- If you need to abandon the current release PR for any reason, close it
  and delete the branch — the next push to `main` opens a fresh PR with
  the default patch-bumped title.
- The `Release` workflow gates on a green CI: a red `main` will never tag
  a release. Fix CI first and the next successful CI run cuts the
  release automatically.
- If `Release` failed because the summary was missing, no draft and no tag
  were created.
  Write the summary into the merged release PR's body (a merged PR's
  description is still editable) and re-run the failed run from the Actions
  tab. The re-run checks out the same commit CI tested
  (`workflow_run.head_sha`), so it still sees the version change after other
  commits have landed on `main`. Nothing else will cut that release: the
  version-change check compares each commit against its parent, so the `Release`
  runs for later commits see no version change.

## Release automation setup

The flow above is developer-facing; this section covers the supporting
setup: how the release PR is opened under a user PAT and why that is
required.

The `Create Release PR` workflow (`.github/workflows/create-release-pr.yml`)
opens and updates the rolling release PR. It runs on two triggers: every push
to `main`, and `pull_request: types: [edited]` so that promoting a release
PR's title (e.g. `Release v0.1.1` → `Release v0.2.0`) is picked up immediately
rather than waiting for the next main push. The `pull_request` branch is
gated on an **open** PR carrying the `release` label whose **title actually
changed** (`changes.title.from != null`), so body-only edits, no-op title
saves, and bot self-edits do not retrigger the workflow. It pushes a
`release/since-<UTC date+time>` branch (chosen once when the PR is opened and
reused for the lifetime of that PR) and calls `gh pr create` under a
**user-scoped personal access token**, exposed to the workflow as the
repository secret **`RELEASE_PAT`**.

A user-scoped token is required because GitHub's recursion-prevention rule
suppresses `pull_request` workflow runs on PRs authored by `github-actions[bot]`.
With the default `GITHUB_TOKEN` the release PR would be bot-authored, so `CI`
and `Validate Release PR` would sit in `action_required` and never go green.
Pushing and opening the PR under a user PAT makes the PR user-authored, which
lets the existing checks trigger normally.

**Required setup (one-time, per repo).** A maintainer must register
`RELEASE_PAT` in the repository's Actions secrets with these scopes:

- `contents: write` — push the `release/since-<UTC date+time>` branch.
- `pull-requests: write` — create and edit the release PR.

Without the secret, the workflow fails loudly at the `git push` step. There is
no fallback to `GITHUB_TOKEN` by design: a silent fallback would mask exactly
the misconfiguration the PAT is solving.

The `Release` workflow (`.github/workflows/release.yml`) that tags and publishes
after the release PR is merged keeps using the default `GITHUB_TOKEN` — it runs
under a human-triggered merge event, so recursion is not a concern, and no
workflow in this repo listens to tag pushes. Only its `draft` and `publish`
jobs get write access to the repository's contents (see
[Desktop bundles](#desktop-bundles)), and only its `sign` job may read the
`release-signing` environment's `MINISIGN_SECRET_KEY` ([Signing](#signing)). The `Create Release PR` job's own token
is read-only: everything it writes goes through `RELEASE_PAT`.
