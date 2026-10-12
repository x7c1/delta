---
status: completed
pipeline_phase: null
plan: null
base_ref: feat/in-app-update
perspectives: [completeness, clarity, error-type-design, rust-module-structure, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && git grep -q --untracked 'DELTA_BUILD_ORIGIN' -- .github/workflows/bundle.yml && git grep -q --untracked 'DELTA_BUILD_ORIGIN' -- backend/crates && git grep -q --untracked '/api/latest-release/download' -- backend/crates/gateway/delta-wire/src && git grep -q --untracked '/api/latest-release/download' -- docs/guides/api && git grep -q --untracked 'releases/download/' -- backend/crates && git grep -q --untracked 'sha2' -- backend/crates"
assignee: null
branch: task/1007-1154-feat-download-and-verify-a-newer-release-in-the-desktop-app
created_at: 2026-10-07T11:54:41Z
updated_at: 2026-10-07T13:01:40Z
---

# feat: download and verify a newer release in the desktop app

## Overview

Delta already notices a newer release: the server asks GitHub's
`releases/latest` (`backend/crates/gateway/release-feed`), judges it in
`delta-usecase`'s `release_check`, serves the verdict on
`GET /api/latest-release`, and the navigator footer
(`frontend/packages/apps/web/src/features/navigator/NavigatorPane.tsx`) shows
`v<version> available` linking to the release page. This task adds the next
step of an in-app update: an **Update** action that downloads this platform's
asset of that release into the data directory and verifies its sha256. It does
**not** apply anything yet — replacing the app and restarting are later
changes. This PR targets the `feat/in-app-update` integration branch, not
`main`, so no release ships a half-finished Update.

### Who may update: the launcher and the build origin

Update is offered only where replacing the running app is both possible and
safe:

- **Launcher.** Only the desktop shell can be replaced. The identifier does not
  tell the shells apart (the CLI's `DEFAULT_IDENTIFIER` equals the bundle
  identifier), but only the desktop shell builds its config through
  `delta_server::config::config_from_env_for`
  (`backend/crates/apps/delta-server/src/config/mod.rs`; called from
  `backend/crates/apps/delta-desktop/src/main.rs`). Record who launched the
  server in `delta_bootstrap::Config` along that path (e.g. a `launcher`
  field: CLI or desktop).
- **Build origin.** A desktop app built locally from a newer `main`
  (`make desktop`) carries the same version and identifier as the release
  bundle, so replacing it with the release would roll the user's tree back.
  Embed where the build was made at compile time from a `DELTA_BUILD_ORIGIN`
  environment variable: `release` only when the bundle workflow
  (`.github/workflows/bundle.yml`, the `tauri-action` step) sets it, and
  `local` for every other build (`make desktop-build`, `make desktop-dev`,
  `cargo build`). An unset or unknown value means `local`, so a build can only
  claim `release` by asking for it.

The server reports what the UI may offer next to the notice, alongside the
existing `newer` in `GET /api/latest-release`:

- desktop + `release`: **Update** (the download below)
- desktop + `local`: no Update; a short hint that this build is made locally
  and is updated by rebuilding it (`make desktop`)
- CLI (the browser version): the notice and link only, as today

### Download and verify

`POST /api/latest-release/download` starts the download of the newer
release's asset for this platform; the UI shows its progress and outcome.

- **Asset choice.** One asset per OS/arch, by tauri-action's naming as in
  v0.5.0: Linux x86_64 → `delta-desktop_<version>_amd64.deb`, macOS aarch64 →
  `Delta_<version>_aarch64.dmg` (`<version>` without the `v`). Any other
  platform, or a release without the matching asset, is reported as
  unsupported rather than guessed.
- **Release data.** Extend what the feed reports (`PublishedRelease` in
  `delta-usecase/src/ports/release_feed.rs`, parsed in
  `release-feed/src/github_release_feed.rs`) with the release's assets: name,
  `browser_download_url` and `digest`. GitHub returns `digest` as
  `sha256:<hex>` per asset; v0.5.0's assets both carry one. Keep these with
  the newer release the check records, so the download uses exactly the
  release the UI announced.
- **Where it may come from.** Accept a download URL only under
  `https://github.com/x7c1/delta/releases/download/` (scheme, host and path
  prefix), like the existing `RELEASE_PAGE_PREFIX` pin for the page. GitHub
  redirects these to its asset host; following that redirect is fine, the pin
  is on the URL the API gave.
- **Verification.** Compute sha256 while streaming (the `sha2` crate) and
  compare with the asset's `digest`. An asset without a `sha256:` digest is
  refused, never downloaded unverified. On a mismatch, an interrupted
  transfer, or any I/O failure, the partial file is deleted and the failure
  is reported with its cause.
- **Where it goes.** A new `updates/` directory in `DataLayout`
  (`backend/crates/libs/delta-bootstrap/src/data_layout.rs`). Write to a
  temporary name and rename to the asset's name only after the digest
  matches, so a file under its final name is always a verified one. Files of
  other versions in `updates/` are removed when a download completes. The
  data directory is already what "erase everything" removes, so `updates/` is
  covered without further work.
- **Timeouts.** The feed's 10-second `REQUEST_TIMEOUT` is for the small JSON
  answer; a download of several MB needs a connect timeout and a stall
  (read) timeout instead of a whole-transfer cap.
- **Who may call it.** The endpoint refuses (4xx with a stable error code,
  documented in `docs/guides/api/`) when the launcher is the CLI, when the
  build is `local`, or when there is no newer release. These refusals are
  enforced by the server, not only by hiding the button.

### UI

In the footer, next to `v<version> available`, a desktop `release` build shows
an **Update** control. Pressing it starts the download and the control shows
its state: downloading, then "ready" (verified, waiting for the later apply
step) or failed (with a retry, and the cause in the tooltip or a short
message). A `local` build shows the rebuild hint instead. The footer row's
wrapping rule from the previous change (the label and the notice wrap as
whole phrases) must hold for the new control too.

### Out of scope

- Applying the downloaded file and restarting (macOS: replacing `Delta.app`;
  Linux: `apt install` in Delta's terminal) — later changes on the same
  integration branch.
- Windows and Intel Mac assets.
- Resuming a partial download; a retry starts over.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `Config` records the launcher; only the `config_from_env_for` path
      (the desktop shell) yields the desktop launcher, and `config_from_env`
      yields the CLI one (unit tests in `delta-server`'s config tests).
- [x] The build origin is `release` only for `DELTA_BUILD_ORIGIN=release` at
      compile time and `local` otherwise, decided by a function covered by
      unit tests for `release`, unset, empty and an unknown value; the bundle
      workflow sets `DELTA_BUILD_ORIGIN: release` on the bundle build step
      (the `check_command` greps `bundle.yml`).
- [x] `GET /api/latest-release` tells the UI which of Update, the rebuild
      hint, or nothing to offer, and its wire type is regenerated for the
      frontend (`make gen-check` inside `make check`).
- [x] The feed parses each asset's name, download URL and digest; a release
      JSON with assets, without assets, and with an asset lacking `digest`
      are each covered by parser tests.
- [x] Asset choice is covered for Linux x86_64, macOS aarch64, an
      unsupported platform, and a release missing the platform's asset.
- [x] A download URL outside `https://github.com/x7c1/delta/releases/download/`
      is refused before any request (tests with another host, another repo,
      `http://`, and a path outside `releases/download/`).
- [x] Download outcomes are covered against a local test server (as
      `release-feed`'s tests already do): a matching digest leaves exactly the
      final file in `updates/`; a mismatching digest, a missing digest, a
      non-2xx answer and a transfer cut short each leave no file under the
      final or temporary name and report the cause; a completed download
      removes another version's file from `updates/`.
- [x] `POST /api/latest-release/download` against each server state answers
      as specified (app tests in `delta-server`): CLI launcher → refused;
      `local` build → refused; no newer release (check off, not yet run, up
      to date) → refused; desktop `release` with a newer release → accepted;
      a second request while a download is running → does not start a second
      transfer; a request after a failed download → starts over; a request
      after a completed download of the same version → reports ready without
      downloading again.
- [x] The footer shows Update only for desktop `release`, the rebuild hint
      only for desktop `local`, and neither for the CLI, and renders the
      downloading, ready and failed states (component tests next to
      `NavigatorPane.test.tsx`).
- [x] `DataLayout` has `updates/`, and the docs describe the endpoint and its
      errors (`docs/guides/api/`) and `DELTA_BUILD_ORIGIN`
      (`docs/guides/development/`).

### Before merge (verified outside the check command)

- [x] On this Linux machine, a desktop build with `DELTA_BUILD_ORIGIN=release`
      and its workspace version temporarily set below the latest release
      shows Update; pressing it (or calling the endpoint with the run's
      token) downloads the real latest `.deb` into `updates/` of the dev data
      directory, and its sha256 matches GitHub's `digest`
      (`sha256sum` of the file against `gh api repos/x7c1/delta/releases/latest`).
- [x] The same build without `DELTA_BUILD_ORIGIN` shows the rebuild hint and
      no Update, and the browser version (`make dev`) shows only the notice.
