---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! git grep -n 'delta-auth-token' -- backend/crates frontend/packages scripts && git grep -qn 'one-time' -- docs/guides/security.md"
assignee: null
branch: task/1015-0207-hand-the-auth-token-to-the-page-through-a-one-time-code
created_at: 2026-10-14T17:07:32Z
updated_at: 2026-10-14T18:04:37Z
---

# feat(auth): hand the per-run token to the page through a one-time code, never in a served page

## Overview

Today the per-run bearer token reaches the frontend inside the page itself:
`delta-server` (with `embed-web`) injects a `<meta name="delta-auth-token">`
tag into `index.html` (`backend/crates/apps/delta-server/src/app/static_web/index_page.rs`),
and in development Vite does the same from `DELTA_AUTH_TOKEN`
(`frontend/packages/apps/web/vite.config.ts`). The token should not be
carried by a response served without credentials. Change
the handoff so that **no response served without credentials carries the
token**, in the packaged desktop app, the embedded server run directly, and
`make dev`.

### The new handoff

1. **One-time code.** The server can mint short-lived, single-use codes
   (random, at least 128 bits from the OS CSPRNG, compared in constant time,
   valid for 10 minutes or until first use, whichever comes first). Minting is
   an in-process call on `AppState`, plus an authenticated endpoint (bearer
   token required) for the re-issue command below.
2. **Exchange.** The frontend is opened at a URL carrying a code in the
   fragment (`/#code=<code>`, so it never reaches a server log or a
   `Referer`). On boot it removes the fragment from the address bar
   (`history.replaceState`), POSTs the code to an exchange endpoint under
   `/api/` (so the Vite dev proxy forwards it), and receives the token in the
   response body. A used, expired or unknown code answers `401` and returns
   no token. The exchange endpoint is the only route besides `/health` and
   `/hooks/*` that the bearer guard lets through, and it stays behind the
   Origin/Host guard.
3. **Keeping the token in the page.** The frontend keeps the token in
   `localStorage` (origin-scoped, so it includes the port; two servers on
   different ports do not share it) and presents it exactly as today
   (`Authorization: Bearer` on REST, `token=` on the WebSocket upgrades). A
   cookie is deliberately not used: cookies are not port-scoped, so a cookie
   set on `127.0.0.1` would be sent to every other listener on `127.0.0.1`.
   When the stored token is missing or answers `401` (the server restarted
   with a new token), the app shows a short screen instead of the normal UI,
   telling the user to open the link the server printed, or to run the
   re-issue command (named on the screen).
4. **Where the code comes from.**
   - **Desktop** (`backend/crates/apps/delta-desktop`): the shell runs the
     server in process, so it mints a code directly and navigates the window
     to `http://127.0.0.1:<port>/#code=<code>` instead of the bare root
     (`server_url` in `main.rs`). Each launch mints a fresh one. Keep the
     link classifier in `links.rs` treating that URL as the app.
   - **`delta-server` run directly**: on start it prints the URL with a code
     to stderr (one line, easy to copy), in place of any bare-URL hint.
   - **Re-issue without restarting**: add a `delta-server` subcommand (for
     example `delta-server open-url`; pick a clear name) that prints a fresh
     URL with a code for the running server. It authenticates to the running
     server with the bearer token, which the server writes on start to a
     `0600` file in its data directory (the data directory is already
     `0700`) and removes on a clean stop. `--origin <url>` lets it print a URL
     for another origin (the Vite dev server).
   - **`make dev`** (`scripts/dev.sh`): prints the Vite URL with a code once
     both servers are up, and names the re-issue command (with the Vite
     origin) for later.
5. **No token in served pages or agent environments.** Remove the meta-tag
   injection from both `index_page.rs` and `vite.config.ts`, and the
   `<meta>` read in `frontend/packages/apps/web/src/config.ts`. Make sure the
   server does not pass `DELTA_AUTH_TOKEN` on to the agents and other child
   processes it spawns (today `make dev` exports it, so it can leak into the
   tmux sessions' environment).

`DELTA_AUTH_TOKEN` stays as the way tests and scripts fix the token in
advance. Update the e2e suites (`e2e-fake`, the real-claude script, the mock
e2e) to the new handoff — for example by seeding `localStorage` with the
fixed token in an init script, or by opening a minted code URL — rather than
reintroducing any served token.

### Docs

- `docs/guides/security.md`: describe the handoff (one-time code in the
  fragment, exchange, where the token is kept and why not a cookie, the token
  file and its mode). Use the phrase "one-time code".
- `docs/guides/development/local-run.md`: the URL to open is the one printed;
  how to re-issue.
- `docs/guides/api/README.md`: how a non-browser client authenticates (bearer
  token fixed by `DELTA_AUTH_TOKEN`, or read from the token file).
- Module docs that describe the old handoff (`static_web/mod.rs`,
  `auth_guard.rs`, `config.ts`, `vite.config.ts`) are rewritten, not left
  describing the meta tag.

Out of scope: the hook secret (`hs`) and port handling, which a separate
change covers.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A route test asserts that `GET /`, `GET /index.html`, and an SPA
      fallback path, requested without credentials, answer `200` with a body
      that does not contain the token (replacing
      `serves_the_page_at_the_root_and_at_index_html_without_a_token`'s
      reliance on the injected tag).
- [x] Tests cover the exchange for each state of a code: valid (returns the
      token once), already used (`401`), expired (`401`), unknown (`401`),
      and missing (`4xx`), and a foreign `Origin` on the exchange (`403`).
- [x] A test asserts minting through the authenticated endpoint requires the
      bearer token (`401` without it).
- [x] A test asserts the token file is written with mode `0600` and removed
      on a clean stop.
- [x] A test asserts `DELTA_AUTH_TOKEN` is absent from the environment the
      server hands to spawned agents.
- [x] Frontend unit tests cover: a fragment code is exchanged, stored, and
      removed from the URL; a stored token is used without a code; a `401`
      shows the re-open screen.
- [x] Desktop: a unit test asserts the URL the window navigates to carries a
      `#code=` fragment and is classified as the app by `links.rs`.
- [x] No source under `backend/crates`, `frontend/packages`, or `scripts`
      mentions `delta-auth-token` (grep gate in `check_command`).
- [x] `docs/guides/security.md` describes the one-time code (grep gate).
- [x] `make check` passes, including both e2e suites and the embedded check.

### Before merge (verified outside the check command)

- [ ] macOS desktop (`make desktop-dev`): the window opens the app,
      sessions list loads, and a session's terminal attaches; the request
      log shows no `401` after the exchange. Relaunching the app works
      without any manual step.
- [ ] Embedded server run directly (`make server-embedded`, then the binary):
      the printed URL opens the app in a browser; a second use of the same URL
      shows the re-open screen; the re-issue command prints a URL that opens
      the app without restarting the server; `curl` of `/` returns a page
      without the token.
- [ ] `make dev`: the printed Vite URL opens the app; the re-issue command
      with `--origin` prints a working Vite URL.
