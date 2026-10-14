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
branch: task/1015-0711-harden-the-update-path-https-helper-env-and-manual-guidance
created_at: 2026-10-14T22:11:01Z
updated_at: 2026-10-14T22:57:18Z
---

# fix(update): fetch updates over HTTPS only, run the root helper with a fixed environment, and guide manual updates outside Delta's terminal

## Overview

Smaller hardening of the in-app update path, next to the signature check.
Paths are under `backend/crates/`.

1. **HTTPS only.** `gateway/release-feed/src/https_client.rs` and
   `github_asset_downloader.rs` build reqwest clients without `https_only`,
   so a redirect to `http://` is followed. Set `.https_only(true)` on every
   client the update path uses (feed, asset, signature download, and the
   helper's own client in `apps/delta-update-helper`).
2. **Feed override.** `apps/delta-server/src/config/mod.rs` honours
   `DELTA_RELEASE_FEED_URL` in every build. Ignore it (with a warning) in a
   build whose origin is `release` (see the build-origin mechanism in
   `docs/guides/development/README.md#build-origin`), and accept only an
   `https://` URL elsewhere.
3. **Root helper environment.** The helper relies on `pkexec` to clear the
   environment, but reads `TMPDIR` (`std::env::temp_dir()`), and reqwest /
   the CA loader read proxy and certificate variables. At the start of the
   helper: work in a fixed root-owned directory (create it `0700`, owned by
   root, and refuse to continue if it or its parent is writable by anyone
   else), build the HTTP client with `no_proxy()`, and load CAs only from the
   system's fixed location, ignoring `SSL_CERT_FILE` / `SSL_CERT_DIR`. Add a
   test for each.
4. **Manual update guidance.** The manual-install text
   (`frontend/packages/apps/web/src/features/navigator/UpdateControl.tsx`,
   `domain/delta-usecase/src/release_update/manual_install.rs`) suggests a
   `sudo apt install …` command without saying where to run it. Say to run it
   in a terminal outside Delta, use `sudo -k apt install …` so no sudo
   timestamp is left behind, and add a line to check the file
   (`sha256sum`, and `minisign -Vm` with the public key once signatures
   exist) before installing.
5. **macOS bundle location.** `gateway/update-installer/src/bundle_installer.rs`
   checks only that the parent directory of `Delta.app` is writable. Also
   require that it is owned by the user or root and not writable by group or
   others (or has the sticky bit); otherwise report the update as
   `Unavailable` with that reason. Exception: group write is allowed when the
   group is an administrators' group (`wheel` or `admin` on macOS), because
   `/Applications` is `root:admin` `0775` and its members can already
   change anything there.

6. **Dry-run signatures.** The release workflow's `dry_run` signs the bundles
   with the production key and a trusted comment naming the branch's
   workspace version, so a dry-run bundle carries a signature the app would
   accept for that version. Make the `sign` job write the dry-run tag's version
   (`0.0.0-dryrun.<run id>`) into the trusted comment on a dry run, so the
   app's check (which requires the version being installed) refuses it.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Tests: each update HTTP client refuses an `http://` redirect target.
- [x] Tests: `DELTA_RELEASE_FEED_URL` is ignored for a release-origin build and
      refused when it is not `https://`.
- [x] Tests: the helper ignores `TMPDIR`, proxy and certificate variables, and
      refuses a work directory whose parent is writable by others.
- [x] Tests: the manual-install guidance names a terminal outside Delta and
      uses `sudo -k`.
- [x] Tests: the macOS installer reports `Unavailable` when the bundle's
      parent directory is world-writable, or group-writable by a group other
      than `wheel` / `admin`, without the sticky bit; `/Applications`-like
      `root:admin 0775` is accepted.
- [x] `make check` passes.

### Before merge (verified outside the check command)

- [x] Reading `release.yml`: on a dry run the trusted comment's version is the
      dry-run tag's version, not the workspace version.
