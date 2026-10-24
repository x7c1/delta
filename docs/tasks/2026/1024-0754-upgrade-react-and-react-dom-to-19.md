---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && make audit"
assignee: null
branch: task/1024-0754-upgrade-react-and-react-dom-to-19
created_at: 2026-10-24T07:54:03Z
updated_at: 2026-10-24T08:59:52Z
---

# build(deps): upgrade react and react-dom to 19

## Overview

Dependabot opened the React 19 bump as two separate PRs: `react` and
`@types/react` 18.3 → 19.3, and `react-dom` and `@types/react-dom`
18.3 → 19.3. Each one alone fails CI, because `react` and `react-dom`
must stay on the same version. This task moves both together, in one change,
and adapts the frontend to React 19.

- In every package under `frontend/packages/` that depends on them, raise
  `react`, `react-dom`, `@types/react` and `@types/react-dom` to
  the 19.3 line (`^19.3.1` for the runtime packages, `^19.3.0` for the
  types), and update `frontend/pnpm-lock.yaml`. Widen the `react`
  `peerDependencies` ranges of `ui-kit` and `api-client` so they accept
  19 (`^19.0.0`; drop 18, since nothing in the workspace uses it any more).
- Fix the type errors React 19's types introduce. In particular, replace the
  deprecated `MutableRefObject` with `RefObject` (React 19's `useRef`
  returns a mutable `RefObject`), give every `useRef` call an initial
  argument, and use `React.JSX` where the global `JSX` namespace is used.
  Do not paper over errors with casts or `any`.
- Fix the tests that change behaviour under React 19 (for example stricter
  `act` warnings, changed error reporting from error boundaries, or
  changed ref-callback semantics). Fix the code or the test so that it
  reflects the intended behaviour; do not skip or delete tests.
- If another React-ecosystem dependency (`@testing-library/react`,
  `@vitejs/plugin-react`, `react-markdown`, `@tanstack/*`) needs a newer
  version to support React 19, raise it in this change, keeping the change
  as small as possible.
- Do not commit generated build output (`dist/`, `dist-types/`).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `react`, `react-dom`, `@types/react` and `@types/react-dom` resolve
      to 19.x in `frontend/pnpm-lock.yaml`, and no 18.x copy of `react` or
      `react-dom` remains in the lockfile.
- [x] `MutableRefObject` no longer appears under `frontend/packages/*/*/src`.
- [x] `make check` passes: the frontend builds, type-checks, lints, and its
      unit and e2e suites pass with no skipped or deleted tests.
- [x] `make audit` passes.

### Before merge (verified outside the check command)

- [x] The web UI loads in `make dev`, a session can be opened, and its
      transcript and timeline render without React errors in the browser
      console.
