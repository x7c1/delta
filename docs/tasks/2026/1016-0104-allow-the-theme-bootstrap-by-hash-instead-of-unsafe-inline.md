---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! git grep -n \"script-src 'self' 'unsafe-inline'\" -- backend/crates/apps/delta-server/src"
assignee: null
branch: task/1016-0104-allow-the-theme-bootstrap-by-hash-instead-of-unsafe-inline
created_at: 2026-10-15T16:04:00Z
updated_at: 2026-10-15T17:27:46Z
---

# fix(web): allow the inline theme bootstrap by hash instead of `'unsafe-inline'`

## Overview

The served page's Content-Security-Policy header
(`CONTENT_SECURITY_POLICY` in
`backend/crates/apps/delta-server/src/app/static_web/index_page.rs`) allows
`script-src 'self' 'unsafe-inline'`, only so that the small inline theme
bootstrap `<script>` in `frontend/packages/apps/web/index.html` (it sets
`data-theme` before the bundle loads) can run. That also lets any injected
inline handler run, which is the first thing an HTML-injection bug would use.
There is no known injection path today; this removes the allowance anyway.

Replace `'unsafe-inline'` in the header's `script-src` with the
`'sha256-…'` source of exactly the inline script(s) the built `index.html`
contains. The hash must follow the built page automatically — derive it from
the embedded `index.html` (for example once, when the static site is set up)
or have the frontend build emit it — so editing the bootstrap cannot silently
break the page. Keep `style-src 'unsafe-inline'` (xterm injects inline
styles). The dev `<meta>` policy in `index.html` keeps `'unsafe-inline'` and
`'unsafe-eval'` for Vite's dev client; both policies are enforced together, so
the header is what tightens the production page. Update the comments in both
places that currently explain why `'unsafe-inline'` stays, and the CSP part of
`docs/guides/security.md`.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The header's `script-src` no longer contains `'unsafe-inline'`
      (`git grep` gate appended to `check_command`).
- [x] A test computes the SHA-256 of every inline `<script>` in the
      embedded/built `index.html` and asserts each appears in the header's
      `script-src` (so a changed bootstrap fails the test, not the page).
- [x] A test asserts `style-src` still allows `'unsafe-inline'` and the other
      directives are unchanged.

### Before merge (verified outside the check command)

- [x] With the embedded build (`make server-embedded` or the desktop dev
      build), the page loads with the saved theme applied on first paint and
      the browser console shows no CSP violation. The merging session runs it
      and records the result in the PR.
