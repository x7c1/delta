---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && git grep -q 'chunkSizeWarningLimit' -- frontend/packages/apps/web/vite.config.ts && git grep -qE '1200' -- frontend/packages/apps/web/vite.config.ts && ! (cd frontend && pnpm --filter @delta/web exec vite build --outDir /tmp/delta-bundle-budget-check 2>&1 | grep -q 'chunks are larger than')"
assignee: null
branch: task/1006-1340-build-web-replace-vites-default-chunk-size-warning-with-deltas-own-bundle-budget
created_at: 2026-10-06T04:37:28Z
updated_at: 2026-10-06T05:06:45Z
---

# build(web): replace Vite's default chunk-size warning with Delta's own bundle budget

## Overview

`vite build` for `@delta/web` prints Vite's chunk-size warning on every run:
the app is one JavaScript chunk of about 927 kB (264 kB gzipped), above the
500 kB default. Measured composition: the terminal (xterm and its addons)
about 298 KiB, the Markdown renderer (react-markdown, remark, micromark)
about 154 KiB, react-dom about 140 KiB, TanStack about 69 KiB, the app's own
code about 194 KiB. A warning that fires on every build teaches everyone to
ignore warnings, and `vite build` already runs inside `make check` and CI
(`check-frontend-build`, `pnpm -r build`), where nothing gates on it.

The default threshold's premises do not hold here, so the fix is not to
split the bundle but to state Delta's own budget:

- Vite's 500 kB is a public-web heuristic for code delivered over the
  network where first paint matters and some code may never be used. Delta
  serves the SPA from its own server on `127.0.0.1` (the desktop app and the
  embedded server alike; nothing is reachable from another machine), where
  927 kB loads in milliseconds, and the terminal and the Markdown renderer
  are used in every session, so a lazy boundary would only turn one request
  into two and add a loading state. The assets are served with immutable
  caching.
- A budget still earns its keep as a regression guard: a dependency that
  accidentally doubles the bundle should fail the build, not print a line
  nobody reads.

### Change

- In `frontend/packages/apps/web/vite.config.ts` (`defineConfig` at line 55,
  `plugins` at line 56; there is no `build` key today), set
  `build.chunkSizeWarningLimit` to **1200** (kB) and explain the number in a
  comment: the app is ~927 kB today, a dependency bump will not cross
  1200 kB, pulling in one more heavy library will — and why the bundle is
  not split (localhost delivery, always-used features).
- Add a small build-only Vite plugin in the same file (or a sibling module
  it imports) that **fails the build** when any emitted chunk exceeds the
  same limit: `apply: 'build'`, inspect each chunk in `generateBundle`
  (Vite minifies in `renderChunk`, so sizes there are the final ones) and
  call `this.error(...)` with the chunk name and size. Share the single
  constant between the warning limit and the plugin so they cannot drift.
  Because `pnpm -r build` runs the plugin, the guard covers `make check`,
  CI and `make web-dist` with no Makefile change.
- Do not change imports, add lazy boundaries or `manualChunks`.
- Document the budget in one sentence in `docs/guides/development/README.md`
  near the frontend build commands: what the limit is, why the bundle is not
  split, and that the build fails above it.

### Verification of the gate

Negative-test the plugin while working: temporarily call it with a limit of
100 kB, run `pnpm --filter @delta/web build`, confirm a non-zero exit with
the plugin's message, then restore 1200 and confirm the build passes
without the chunk-size warning. Record both runs in your report so the PR
can cite them.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `vite.config.ts` sets `chunkSizeWarningLimit` to 1200 (`git grep`
      gates) and a production build of `@delta/web` emits no
      "chunks are larger than" line (gate in `check_command`, which builds
      into a throwaway output directory).
- [x] The size-gate plugin runs on `pnpm -r build` and fails a build whose
      chunk exceeds the limit (demonstrated by the negative test recorded
      in the work report; the plugin's error path is exercised by a vitest
      that feeds it a synthetic oversized chunk).
- [x] `make check` passes.

### Before merge (verified outside the check command)

- [x] On the development machine, `make desktop-dev-build` (which runs
      `make web-dist`) completes without the warning and the built app loads
      the SPA, terminal and Markdown exactly as before.
