---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -q minimatch frontend/pnpm-workspace.yaml && ! grep -qE '^  minimatch@(([0-9]|10)\\.|11\\.0\\.0[:(])' frontend/pnpm-lock.yaml"
assignee: null
branch: task/1117-1400-drop-the-minimatch-override
created_at: 2026-11-17T05:00:17Z
updated_at: 2026-11-17T05:13:32Z
---

# build(deps): bump eslint and typescript-eslint and drop the minimatch override

## Overview

#628 added a pnpm override `minimatch: ^11.0.1` in
`frontend/pnpm-workspace.yaml` to fix Dependabot alert #44
(GHSA-g3hv-75wg-m249: minimatch >= 7.0.0, < 11.0.1). It was needed because the
parents that pulled minimatch in at the time declared a 10.x range, so
Dependabot could not update it (`security_update_not_possible`). minimatch is a
dev-only transitive dependency here, reached only through
`@eslint/config-array`, `eslint` and `@typescript-eslint/typescript-estree`.

An override forces a version outside the range the parents declare, so it is a
stopgap: left in place, it can later pin an older minimatch than a parent asks
for. The parents now declare minimatch 11 themselves:

| Package | In the lockfile | Latest | Its minimatch range |
|---|---|---|---|
| `@eslint/config-array` | 0.23.5 | 0.23.6 | `^11.0.5` |
| `eslint` | 10.14.0 | 10.15.0 | `^11.0.5` |
| `@typescript-eslint/typescript-estree` | 8.72.2 | 8.73.0 | `^11.0.1` |

Bump `eslint` and `typescript-eslint` in `frontend/package.json` (and
`@eslint/js` alongside `eslint`, if a matching release exists) to the releases
that bring these versions in, remove the `minimatch` entry from `overrides` in
`frontend/pnpm-workspace.yaml` (and the `overrides:` key itself if it becomes
empty), and regenerate `frontend/pnpm-lock.yaml` with `pnpm install`. Fix any
new lint findings the bumped eslint or typescript-eslint report, so the lint
gate stays clean without disabling rules.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `frontend/pnpm-workspace.yaml` no longer mentions minimatch (gate:
      `! grep -q minimatch frontend/pnpm-workspace.yaml` in `check_command`).
- [x] Every minimatch the lockfile resolves is 11.0.1 or later, without the
      override (gate: no `minimatch@` entry at 10.x or below, or at 11.0.0, in
      `frontend/pnpm-lock.yaml`, in `check_command`).
- [x] Lint, typecheck, build and tests of the frontend pass with the bumped
      eslint and typescript-eslint (`make check`).
