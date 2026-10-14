---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && scripts/check-workflow-pins.sh"
assignee: null
branch: task/1015-0549-sign-release-bundles-and-verify-before-installing
created_at: 2026-10-14T20:49:41Z
updated_at: 2026-10-14T21:52:18Z
---

# feat(update): sign release bundles with a key outside the build, and verify the signature before installing an update

## Overview

The in-app update checks a downloaded bundle against the sha256 digest GitHub
reports for the Release asset
(`backend/crates/gateway/release-feed/src/github_asset_downloader.rs`, and the
Linux root helper's `backend/crates/apps/delta-update-helper/src/release_digest.rs`).
That digest is computed by GitHub from whatever was uploaded, so it proves the
download is intact, not that the release pipeline produced it. Add a signature
made with a key that no build step can read, and make the app and the helper
refuse an update whose signature does not verify.

### Signing in the release pipeline

- A minisign (Ed25519) key pair. The secret key lives only in a GitHub
  Environment secret `MINISIGN_SECRET_KEY` of an environment named
  `release-signing` (created by the maintainer; the workflow only references
  it). The key has no password (CI signs unattended).
- A new `sign` job in `.github/workflows/release.yml`, between `bundles` and
  `publish`, with `environment: release-signing` and `permissions: {}` (it
  needs no token): no checkout, no project code, no third-party action other
  than the pinned `actions/download-artifact` / `actions/upload-artifact`. It
  installs minisign from a pinned release archive verified against a sha256
  written in the workflow, signs each bundle (`<asset>.minisig`, with a trusted
  comment naming the asset file name and the version), and uploads the
  signatures as an artifact. `publish` attaches the `.minisig` files next to the
  bundles and checks they are present before publishing. The dry run signs
  too (it exercises the job end to end).
- `publish` also creates GitHub artifact attestations for the bundles
  (`actions/attest-build-provenance`, pinned; `id-token: write` and
  `attestations: write` on that job only).

### Verifying in the app and the helper

- The production public key is
  `RWSvjaqxlJm4wr9SiKBHDAlbhBASzeSf0dbg9loPJdgbTFQOEOX5J1Pl` (minisign key id
  `C2B89994B1AA8DAF`). It is a constant compiled into the app and into
  `delta-update-helper` (one definition shared by both, e.g. in a small crate
  or module both depend on; not read from a file or the environment).
- Verification uses the `minisign-verify` crate (pure Rust, no network). The
  app downloads `<asset>.minisig` from the same Release (same URL checks as the
  asset: fixed host and path prefix) and verifies the bundle before it is
  offered for install; the trusted comment must name exactly the asset file
  name and version being installed (so a valid signature for another file or
  an older version is refused).
- macOS: verify before the bundle is mounted or copied. Linux: the root helper
  verifies by itself again (it already re-derives the digest; it must not
  trust the caller), downloading the signature itself.
- A missing or invalid signature makes the update `Unavailable` with a reason
  shown in the UI, and the manual-update guidance applies. There is no
  fallback to digest-only.

### Transition

Releases made before this change carry no `.minisig`. The first app version
that verifies can only update to releases published after this change, which
all carry signatures; say so in `docs/guides/release.md`. The update's
availability check for an older unsigned release (not newer than the running
version) never arises, so no exception is needed.

### Docs

- `docs/guides/release.md`: the `sign` job, the environment and secret, key
  rotation (a new public key ships in an app release signed with the old key;
  losing the key means users update manually once), and the attestation.
- `docs/guides/install/README.md` (or the macOS / Ubuntu guides): how a user
  checks a downloaded bundle by hand (`minisign -Vm <file> -P <public key>`,
  `gh attestation verify <file> -R x7c1/delta`), with the public key printed.
- `docs/guides/security.md`: what the signature protects (assets replaced
  after the build, or uploaded by anything other than the release pipeline)
  and what it does not (code injected into the build itself).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Unit tests for verification: a bundle signed by a test key with the
      expected trusted comment verifies; a modified bundle, a signature by a
      different key, a trusted comment naming another file or version, and a
      missing signature are each refused with a distinct reason. (Tests use a
      test key pair generated for the tests; the production public key is
      only compiled in.)
- [x] The app's install flow (macOS and Linux paths, with fakes) refuses to
      install when verification fails and reports `Unavailable` with the
      reason; the helper refuses on its own when the signature is invalid even
      if the caller says it verified.
- [x] The public key constant is defined once and used by both the app and
      the helper (a test or grep gate).
- [x] `scripts/check-workflow-pins.sh` passes (the new actions are pinned).
- [x] `make check` passes.

### Before merge (verified outside the check command)

- [x] Reading `release.yml`: only `sign` references the `release-signing`
      environment; `sign` has `permissions: {}`, no checkout, and runs no
      project code; only `publish` holds `id-token: write` /
      `attestations: write`.
- [x] A bundle signed locally with the production secret key verifies with
      the compiled-in public key through the app's verification function.
