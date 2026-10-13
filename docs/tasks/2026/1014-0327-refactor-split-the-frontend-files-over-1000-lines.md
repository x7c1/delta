---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && make check-frontend-file-length && grep -q 'check-frontend-file-length' Makefile && grep -rq 'check-frontend-file-length' .github/workflows/"
assignee: null
branch: task/1014-0327-refactor-split-the-frontend-files-over-1000-lines
created_at: 2026-10-13T18:27:00Z
updated_at: 2026-10-13T19:32:06Z
---

# refactor(frontend): split the files over 1000 lines by responsibility, and gate the length

## Overview

Nineteen frontend source files exceed 1000 lines. Each mixes several
responsibilities, which makes it hard to see what a change touches and to
review it. Seven are application or library code:

| file | lines |
|---|---|
| `frontend/packages/apps/web/src/features/transcript/ThreadTimelineOverlay.tsx` | 1888 |
| `frontend/packages/apps/web/src/features/transcript/TranscriptPane.tsx` | 1877 |
| `frontend/packages/apps/web/src/features/settings/SettingsView.tsx` | 1874 |
| `frontend/packages/testing/api-mocks/src/handlers.ts` | 1803 |
| `frontend/packages/testing/api-mocks/src/fixtures.ts` | 1351 |
| `frontend/packages/gateway/api-client/src/query-hooks.ts` | 1167 |
| `frontend/packages/gateway/api-client/src/http.ts` | 1001 |

and twelve are test files, all under `frontend/packages/apps/web/src/`:
`features/transcript/TranscriptPane.test.tsx` (2873),
`store/liveStore.test.ts` (2716),
`features/workspace/WorkspaceScreen.test.tsx` (2252),
`features/navigator/NavigatorPane.test.tsx` (1664),
`features/settings/SettingsView.test.tsx` (1600),
`features/transcript/ThreadTimelineOverlay.cross-lane-jump.test.tsx` (1310),
`features/transcript/timelineLanes.test.ts` (1237),
`features/transcript/ThreadTimelineOverlay.lanes.test.tsx` (1235),
`features/transcript/ThreadTimelineOverlay.playhead.test.tsx` (1178),
`features/transcript/ThreadTimelineOverlay.external-thread.test.tsx` (1178),
`data/applySessionEvent.test.ts` (1080),
`features/composer/Composer.test.tsx` (1046).

Split each along what it does, following the layout the package already uses
(sibling modules in the same feature directory, or a directory with an
`index.ts` where the package does that; one component, hook or concern per
module). Pick the seams from responsibilities, not from line counts; a piece
that ends near the limit is a sign the seam is wrong. For a component, the
natural seams are its sub-components, the hooks that hold its state, and the
pure helpers it computes with. For the API client and the mocks, group by
resource. For a test file, split by the behaviour each `describe` covers, and
move setup shared between the new files into a helper module next to them
(the `ThreadTimelineOverlay.*.test.tsx` files already show this pattern).

Keep behaviour and keep it a move: no logic rewrite, no renamed exports.
Every import of a moved symbol from outside the split keeps working — either
the original module re-exports it, or each importer is updated in this
change. Respect the dependency rules dependency-cruiser enforces between
packages. Keep every test: the number of test cases `make
check-frontend-test` reports does not change.

So that the count does not creep back, add a `check-frontend-file-length`
target to the `Makefile` that fails, naming each offender, when any tracked
`*.ts` or `*.tsx` file under `frontend/` exceeds 1000 lines; generated files
(for example under `frontend/packages/gateway/wire-gen/`), if any cross the
limit, are excluded explicitly by path. If the backend has a
`check-backend-file-length` target by the time this runs, share its mechanism
instead of writing a second one. Add the target to `CHECK_STEPS`, update the
gate graph comment above `CHECK_STEPS`, run it in a job in
`.github/workflows/ci.yml` (it needs no build, so the frontend job is fine),
and list it in `docs/guides/development/README.md` where the other frontend
checks are listed.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make check-frontend-file-length` passes: no tracked `.ts` / `.tsx` file
      under `frontend/` exceeds 1000 lines.
- [x] `check-frontend-file-length` is part of `make check` (`CHECK_STEPS`)
      and a CI workflow runs it.
- [x] `make check` passes (type check, lint including dependency-cruiser,
      unit tests, both e2e suites).

### Before merge (verified outside the check command)

- [ ] The number of frontend unit test cases is the same on `main` and on
      this branch.
- [ ] `make check-frontend-file-length` fails, naming the file, when one file
      is padded past 1000 lines locally.
- [ ] The diff moves code without changing it, apart from imports, exports
      and the props or parameters a moved piece needs (reviewed with
      `git diff --color-moved=dimmed-zebra`).
