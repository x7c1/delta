---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 0
check_command: "make check && ! grep -rq 'cfg!(debug_assertions)' backend/crates/apps/delta-server/src/"
assignee: null
branch: task/1028-1528-show-the-commit-on-every-local-build-retry-1
created_at: 2026-10-28T15:28:00Z
updated_at: 2026-10-28T16:05:42Z
---

# feat(server): show the commit in the version of every local build

## Overview

The navigator footer shows `Delta <version>` from `GET /api/version`, which
`backend/crates/apps/delta-server/src/version.rs` (`display_version`) renders.
Today it appends `+dev.<short-sha>` only when `cfg!(debug_assertions)` is on.
`make desktop` builds the release profile, so a desktop app built locally from
the latest `main` reads `v0.6.0` exactly like the published bundle. Dogfooding
runs on such builds, and the footer cannot tell whether the installed app
really changed after a rebuild.

The build already records where it was made: `delta_server::config::build_origin()`
reads `DELTA_BUILD_ORIGIN` at compile time and answers `BuildOrigin::Release`
only for the bundle workflow (`.github/workflows/bundle.yml`), `Local` for every
other build (`make desktop`, `make desktop-dev`, `make dev`, `cargo build`). See
`docs/guides/development/README.md#build-origin`. `build.rs` already embeds
`DELTA_GIT_SHA` in every profile and reruns when `DELTA_BUILD_ORIGIN` changes.

### The change

- Decide the suffix by build origin instead of build profile: a `release` build
  answers `v<version>`, a `local` build `v<version>+dev.<short-sha>`, whatever
  the profile. The suffix stays `+dev.<sha>` (SemVer build metadata), so debug
  builds read as they do today.
- Make both branches testable in one test run: for example, render from the
  version, the sha and a `BuildOrigin` passed in, with `display_version()`
  supplying `build_origin()`. Replace the `#[cfg(debug_assertions)]` /
  `#[cfg(not(debug_assertions))]` tests with tests that assert each origin's
  exact output.
- The release check keeps comparing the base version (`VERSION`, no metadata);
  this change must not alter what `GET /api/latest-release` judges newer.
- Update every description of the rule "debug builds carry `+dev.<sha>`, release
  builds do not" to the origin rule. Known sites:
  - `backend/crates/apps/delta-server/src/version.rs` (module doc and fn doc)
  - `backend/crates/apps/delta-server/src/api/version.rs:12`
  - `backend/crates/apps/delta-server/build.rs` (module doc)
  - `backend/crates/gateway/delta-wire/src/rest/version_response.rs:10`
    (regenerate `frontend/packages/gateway/wire-gen/src/generated/VersionResponse.ts`
    with the repo's generator, do not hand-edit it)
  - `backend/crates/gateway/delta-wire/src/endpoint/table.rs:275`
  - `frontend/packages/apps/web/src/features/navigator/NavigatorPane.tsx:340`
  - `frontend/packages/gateway/api-client/src/http.ts:734`
  - `frontend/packages/testing/api-mocks/src/releaseHandlers.ts:9`
  - `docs/guides/api/settings.md` (`GET /api/version`, and the "debug build"
    example under `GET /api/latest-release`)
  - `docs/guides/development/README.md#build-origin` (add the version display
    to what the build origin decides)
  - `backend/crates/domain/delta-usecase/src/release_check/mod.rs:12`
  `grep -rn '+dev' backend frontend docs/guides` for any others. Past task files
  under `docs/tasks/` are records and stay as they are.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `display_version` no longer branches on `cfg!(debug_assertions)`
      (`! grep -rq 'cfg!(debug_assertions)' backend/crates/apps/delta-server/src/`
      in `check_command`).
- [x] A unit test asserts that a `BuildOrigin::Release` build renders exactly
      `v<version>` and a `BuildOrigin::Local` build renders exactly
      `v<version>+dev.<sha>`, both running under the default test profile.
- [x] The existing release-check tests still pass unchanged in what they
      assert (a `+dev.<sha>` version is told only about a release above its base).

### Before merge (verified outside the check command)

- [x] A release-profile `delta-server` built without `DELTA_BUILD_ORIGIN`
      (as `make desktop-build` builds it), started on a spare port with a
      throwaway data directory, answers `GET /api/version` with
      `v<version>+dev.<sha>` carrying the sha of the built commit.

## Last failure

The check failed in `make check`'s `check-backend-doc` step (`RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items`):

```
error: public documentation for `display_version` links to private item `render_version`
  --> crates/apps/delta-server/src/version.rs:47:27
   |
47 | /// build's origin. See [`render_version`] for the forms.
   |                           ^^^^^^^^^^^^^^ this item is private
   |
   = note: this link resolves only because you passed `--document-private-items`, but will break without
   = note: `-D rustdoc::private-intra-doc-links` implied by `-D warnings`
```

A public item's doc must not link to a private one. Everything else in `make check` that ran before this step passed.
