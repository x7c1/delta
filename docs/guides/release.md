# Release

How to cut a Delta release. Releases are driven by a single rolling pull
request that the bot keeps in sync with `main`; cutting a release is just
merging that PR.

## Overview

Delta uses a "merge the PR" release model. A bot opens and updates one
rolling release PR against `main`; merging it triggers the `Release`
workflow, which creates the matching `vX.Y.Z` tag and a GitHub Release.

## Normal flow (patch bump)

1. The `Create Release PR` workflow opens or refreshes a single open PR
   titled `Release vX.Y.Z` on every push to `main`. By default `X.Y.Z` is
   the last tag patch-bumped (e.g. `v0.1.0` → `Release v0.1.1`).
2. The PR body carries the changelog since the previous tag (auto-generated
   from `git log` via `generate-changelog.sh`).
3. Merge the PR when you want to cut the release. The `Release` workflow
   then runs after CI completes on `main`, creates the `vX.Y.Z` tag, and
   publishes a GitHub Release with the same changelog.

## Promoting to minor or major

To cut a minor or major release, **edit the PR title only**:

- `Release v0.1.1` → `Release v0.2.0` (minor)
- `Release v0.1.1` → `Release v1.0.0` (major)

On the next push to `main`, the bot:

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

## Workflows involved

- `.github/workflows/create-release-pr.yml` — on every push to `main`,
  opens or updates the release PR (skipped when the push is itself the
  merge of a release PR, to avoid looping).
- `.github/workflows/validate-release-pr.yml` — on every release PR edit,
  enforces the allowed title transitions above.
- `.github/workflows/release.yml` — when CI completes successfully on
  `main`, checks whether the workspace version changed; if it did, creates
  the matching tag and GitHub Release.

For the underlying setup (the `RELEASE_PAT` secret, why a user PAT is
required instead of `GITHUB_TOKEN`), see
[development.md → Release automation](development.md#release-automation).

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
