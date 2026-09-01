---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -q ''http-equiv="Content-Security-Policy"'' frontend/packages/apps/web/index.html && grep -q "Content-Security-Policy" frontend/packages/apps/web/vite.config.ts && grep -rq "foreign image referenced by assistant markdown" frontend/packages/apps/web/e2e-fake'
assignee: null
branch: task/0901-0928-feat-content-security-policy
created_at: 2026-09-01T09:28:00Z
updated_at: 2026-09-01T09:50:00Z
---

# feat(web): restrict resource loading with a Content-Security-Policy

## Overview

Assistant messages are attacker-influenceable: their markdown is rendered by
`react-markdown` (`frontend/packages/apps/web/src/features/transcript/AssistantMarkdown.tsx`,
consumed at `MessageItem.tsx:219` and `TranscriptPane.tsx:1744`) with no
Content-Security-Policy anywhere in the app. Raw HTML/script is already inert
(no `rehype-raw`), but `![](https://attacker.example/x.png)` renders a real
`<img src>` and autolinked `https://` URLs render real hrefs — so a
prompt-injected or malicious agent output causes an **outbound request to an
arbitrary host**, an exfiltration channel (the request path/query carries the
data). Add a CSP that forbids this.

Baseline directives to enforce (the security floor):

```
img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; object-src 'none'
```

with `default-src 'self'`, `base-uri 'self'`, plus the minimum needed for the
app to run (see below). `connect-src 'self'` also blocks the agent output from
opening a socket or `fetch()` to a foreign host; `img-src 'self' data:` blocks
the external-image channel while keeping inline data-URI images working.

**Where the CSP goes — important, and different from a naive "set a response
header on the backend".** `delta-server` does **not** serve the frontend HTML
document at all: it binds only `/health`, `/hooks/*`, `/api/*`, `/ws`, `/pty`,
`/comms` (`backend/crates/apps/delta-server/src/app/mod.rs:25–79`), and there
is no `ServeDir`/`ServeFile`/index route. The page is served by the **Vite dev
server** (`localhost:5173`), which proxies `/api`, `/ws`, `/pty`, `/comms` to
the backend (`frontend/packages/apps/web/vite.config.ts:12–22`). A CSP response
header from `delta-server` would only decorate JSON/WS responses and would
**not** cover the HTML document. So:

1. Put the CSP in `frontend/packages/apps/web/index.html` as a
   `<meta http-equiv="Content-Security-Policy" content="…">` in `<head>`
   (before the existing inline theme `<script>` and the module script). This
   is the primary control and is what protects the document in every serving
   mode.
2. Also set the same CSP as a dev response header via Vite
   `server.headers` in `vite.config.ts`, so the header path is covered during
   `make dev`.

**Loosen only what is strictly needed, and say why in a comment.** Confirm the
app runs under the CSP — react-markdown, the xterm terminal, and the Vite dev
client / React Fast Refresh:

- `script-src`: the app has an inline `<script>` in `index.html` (the
  synchronous theme bootstrap, lines 7–33) and, in dev, Vite injects its HMR
  client and the React Refresh preamble as inline scripts and uses `eval`.
  Prefer to keep the inline theme script working via a **hash**
  (`'sha256-…'`) rather than blanket `'unsafe-inline'` if it survives
  `make check`; dev tooling (`'unsafe-inline' 'unsafe-eval'`) is acceptable to
  keep since there is no production static-serving path in this repo (scope
  note below). Whatever you end up with, keep it as tight as the suite allows
  and comment each allowance.
- `style-src`: xterm and Vite dev inject inline styles, so `'unsafe-inline'`
  is expected here; comment it.
- `font-src 'self' data:` for any glyphs xterm/data-URI fonts pull.
- `connect-src 'self'`: `/ws`, `/pty`, `/comms`, and the Vite HMR socket are
  all same-origin (`localhost:5173`), so `'self'` covers them. Verify HMR and
  the live channels still connect under it.

Do not add any external host to any directive. If a directive genuinely must
be widened for the app to function, widen it to the narrowest token that
works and explain it inline.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `frontend/packages/apps/web/index.html` contains a
      `<meta http-equiv="Content-Security-Policy">` whose content includes at
      least `img-src 'self' data:`, `connect-src 'self'`,
      `frame-ancestors 'none'`, and `object-src 'none'` (the
      `http-equiv="Content-Security-Policy"` grep is appended to
      `check_command`).
- [x] `frontend/packages/apps/web/vite.config.ts` sets the same CSP as a dev
      response header via `server.headers` (the `Content-Security-Policy` grep
      on `vite.config.ts` is appended to `check_command`).
- [x] A new Playwright `e2e-fake` spec loads a scenario whose assistant
      message contains `![](https://example.invalid/x.png)` and asserts that
      **no network request is made to `example.invalid`** (fail the test if
      such a request is observed) — the real browser enforces the CSP, so this
      proves the image is blocked. The spec's title contains the phrase
      "foreign image referenced by assistant markdown" (grepped by
      `check_command`).
- [x] `make check` passes green: react-markdown rendering, the xterm terminal,
      the existing e2e / e2e-fake suites, and the Vite dev client all function
      under the CSP (i.e. the CSP did not break the app).

### Manual / on-hardware (verified by a human before merge)

- [ ] In a live `make dev` browser session, the transcript, terminal
      (xterm), and theme bootstrap render correctly with no CSP violation in
      the devtools console, and an assistant message with an external image
      shows a blocked image rather than fetching it. (Non-blocking for merge
      under the agreed CI-green autonomous policy; recorded for dogfooding.)

## Out of scope

- A CSP response header from `delta-server`: it does not serve the HTML
  document, so a header there would not protect the page. If a production
  static-serving path is ever added to the backend, the CSP must be attached
  there too — but that path does not exist in this repo today.
- A nonce-based `script-src` requiring per-response injection: there is no
  server rendering the HTML to mint a nonce, so a static meta CSP with a
  hashed inline script (or the dev-tooling allowances) is the mechanism.
- Tightening `connect-src`/`img-src` to specific hosts beyond `'self'` /
  `data:` — the floor above is the target.
- Any change to how markdown is parsed or to the `react-markdown` plugin set
  (raw HTML is already not rendered).
