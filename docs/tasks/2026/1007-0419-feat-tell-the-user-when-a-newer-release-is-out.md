---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, error-type-design, rust-module-structure, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && git grep -q --untracked '/api/latest-release' -- backend/crates/gateway/delta-wire/src && git grep -q --untracked '/api/latest-release' -- docs/guides/api && git grep -q --untracked 'DELTA_RELEASE_FEED_URL' -- docs/guides/development && git grep -q --untracked 'DELTA_RELEASE_FEED_URL' -- frontend/packages/apps/web/e2e-fake && git grep -q --untracked 'releases/latest' -- backend/crates && git grep -q --untracked 'rustls-platform-verifier' -- backend/crates/gateway && ! git grep -q --untracked 'webpki-roots' -- backend/crates"
assignee: null
branch: task/1007-0419-feat-tell-the-user-when-a-newer-release-is-out
created_at: 2026-10-07T04:19:49Z
updated_at: 2026-10-07T06:54:20Z
---

# feat: tell the user when a newer release is out

## Overview

Delta never learns that a newer release exists: the desktop app and the
browser build keep running the version they were installed at until the user
happens to look at the Releases page. This task adds the first half of an
in-app update — **noticing** a newer release and **telling** the user — and
nothing that downloads or replaces anything. Later changes build the desktop
app's "Update" action on top of what this one exposes.

### Where the newer release comes from

- The server asks GitHub for the latest release of this repository:
  `GET https://api.github.com/repos/x7c1/delta/releases/latest`. That endpoint
  never returns drafts or pre-releases. Send a `User-Agent` (GitHub rejects
  requests without one) and `Accept: application/vnd.github+json`; no token.
- The backend has no HTTPS client today. Add one in a new crate under
  `backend/crates/gateway/` (for example `reqwest` with rustls and
  `default-features = false`, so no OpenSSL is linked into the desktop
  bundle), behind a trait the server depends on, so tests can substitute it.
- Verify certificates against the operating system's trust store (for
  example through `rustls-platform-verifier`): the Keychain on macOS, the
  system CA store on Linux. The roots bundled with webpki would reject a
  machine whose network re-signs TLS with a CA the user installed there.
- Compare the release's `tag_name` (`v<semver>`) with this build's
  `CARGO_PKG_VERSION` (what `backend/crates/apps/delta-server/src/version.rs`
  renders) using SemVer precedence. Only a strictly greater version counts.
  Compare the base version in every build profile: a debug build
  (`v0.5.0+dev.<sha>`) carries build metadata only, so it is told about a
  release only when the tree is behind one, which is what a developer wants.
- Keep the release's `html_url` as the link to show. Accept it only when it
  starts with `https://github.com/x7c1/delta/releases/`; otherwise treat the
  answer as malformed. A later change downloads assets from the same answer,
  so the place it may point to is pinned here.

### When it is checked

- Check once shortly after the server starts, in the background (never on
  the startup path: an offline machine must start exactly as fast as
  today), then every 6 hours while it runs. Unauthenticated requests are
  allowed 60 per hour, far above this.
- Hold the last result in the server's state. A failed check (offline, DNS,
  timeout, non-2xx including GitHub's 403 rate limit, unparsable JSON, a tag
  that is not SemVer, an `html_url` outside the pinned prefix) logs one
  `warn` naming the cause and keeps the previous result; it never surfaces
  as an error in the UI. Give the request a timeout (for example 10 s).
- `DELTA_RELEASE_FEED_URL` overrides the URL; set to the empty string it
  turns the check off. Document it next to the other `DELTA_*` variables in
  `docs/guides/development/README.md`. The e2e-fake harness
  (`frontend/packages/apps/web/e2e-fake/support/server.ts`) sets it to the
  empty string so CI never reaches GitHub; the Playwright mock suite never
  starts a server and needs nothing.

### What the browser sees

- New endpoint `GET /api/latest-release` declared in
  `backend/crates/gateway/delta-wire/src/endpoint/table.rs` like
  `GetVersion`, with a `WireLatestReleaseResponse` whose only field is
  `newer: { version: string, url: string } | null` — `version` rendered like
  `display_version` (`v0.6.0`). `null` covers "not checked yet", "checked and
  up to date", "check turned off" and "every check so far failed": the UI
  has nothing to say in any of them. Regenerate the TS bindings
  (`make gen`), and document the endpoint in `docs/guides/api/`.
- In the navigator footer (`frontend/packages/apps/web/src/features/navigator/NavigatorPane.tsx`,
  where `Delta <version>` is rendered from `useVersionQuery`), show a short
  notice next to the version when `newer` is set — for example
  `v0.6.0 available` — as a link to `url` that opens outside the app
  (`target="_blank"`; the desktop shell already sends new-window links to
  the default browser). Poll the endpoint with a long interval (for example
  hourly) so a long-running page learns about a release without a reload.
  Add the query hook beside `useVersionQuery` in
  `frontend/packages/gateway/api-client/src/query-hooks.ts` and an MSW
  handler and fixture in `frontend/packages/testing/api-mocks/`.

Out of scope: downloading, verifying or applying a release; any "Update"
button; telling the desktop app apart from the CLI server. The browser build
ends up with exactly what it should have (a notice and a link); the desktop
app gets the same notice until the later change adds its action.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `GET /api/latest-release` is declared in the delta-wire endpoint table,
      exported to TS, and documented under `docs/guides/api/` (both greps in
      `check_command`).
- [x] Unit tests cover the comparison: a greater release version yields
      `newer`; an equal one, a lower one, and a debug build at the same base
      version yield `null`; a tag that is not SemVer is a failed check.
- [x] Tests with a substituted client cover each state a check can end in:
      newer release, up to date, network failure, non-2xx status, malformed
      JSON, `html_url` outside the pinned prefix — and that a failed check
      after a successful one keeps the earlier `newer`.
- [x] A server built with `DELTA_RELEASE_FEED_URL=""` makes no request and
      answers `newer: null` (a test asserts it), and the e2e-fake harness sets
      that variable (grep in `check_command`).
- [x] `DELTA_RELEASE_FEED_URL` is documented in `docs/guides/development/`
      (grep in `check_command`).
- [x] The HTTPS client verifies against the platform trust store, not
      bundled webpki roots (`check_command` greps the gateway crates for
      `rustls-platform-verifier` and finds no `webpki-roots` feature).
- [x] A component or Playwright mock test shows the footer notice with a
      link to the release URL when the mock answers `newer`, and no notice
      when it answers `null`.

### Before merge (verified outside the check command)

- [x] Against the real GitHub API, a release build of `delta-server` whose
      workspace version is temporarily set below the latest tag shows
      `v<latest> available` in the footer, and the link opens the Release
      page in a new browser tab.
- [x] With the network unreachable (for example `DELTA_RELEASE_FEED_URL`
      pointed at a closed local port), the server starts without delay, the
      footer shows no notice, and the log has one `warn` per check naming
      the cause.
- [x] In the desktop app (`make desktop`), the same notice appears and its
      link opens in the default browser, not in the app window.
