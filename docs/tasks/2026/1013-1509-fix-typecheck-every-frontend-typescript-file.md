---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -rn --include='tsconfig*.json' -E '\"exclude\"' frontend/packages/"
assignee: null
branch: task/1013-1509-fix-typecheck-every-frontend-typescript-file
created_at: 2026-10-13T06:09:00Z
updated_at: 2026-10-13T08:24:25Z
---

# fix(frontend): typecheck every TypeScript file, tests and configs included

## Overview

`pnpm -r typecheck` (part of `make check` as `check-frontend-typecheck`) runs
`tsc --noEmit` per package, but each package's `tsconfig.json` limits it to
`include: ["src"]`, and most also exclude their tests. Vitest, Vite and
Playwright transpile with esbuild and never look at types, so the following
are not type-checked by anything today:

- unit tests: `src/**/*.test.ts(x)` and `src/test/**` in `@delta/web`;
  `*.test.ts(x)` in `@delta/api-client`, `@delta/api-mocks` and
  `@delta/ui-kit` (see each package's `exclude`)
- the Playwright suites in `@delta/web`: `e2e/`, `e2e-fake/`, `e2e-real/`
- the package-root files of `@delta/web`: `vite.config.ts`,
  `playwright.config.ts`, `playwright.fake.config.ts`,
  `playwright.real.config.ts`, `bundle-budget.ts`, `bundle-budget.test.ts`

A probe that widened the configs (2026-10-13) found 70 type errors in
`@delta/web` (66 under `src`, mostly missing required props in test fixtures
— TS2739 / TS2741 / TS2322 / TS2345 — plus 2 in `e2e`, 1 in `e2e-fake`, 1 in
`vite.config.ts`), 3 in `@delta/api-client` and 4 in `@delta/api-mocks`. These
are real: a fixture missing a required field tests a shape the app never
receives.

Bring every `.ts` / `.tsx` file in `frontend/packages/` under a type check
that `pnpm -r typecheck` runs, and fix every error it reports at its cause
(complete the fixture, correct the type) — no `any`, `@ts-ignore`,
`@ts-expect-error` or non-null assertions added to silence one.

The library configs are also the build configs (`composite`, `outDir`
`dist-types`, `rootDir: "src"`), so tests must not leak into emitted
declarations. A conventional shape is a solution-style setup per package: the
existing config keeps building `src` without tests, and a second config (for
example `tsconfig.test.json` for tests, and in `@delta/web` one for the
Playwright suites and the root-level config files, which need Node and
Playwright types) covers the rest, with the package's `typecheck` script
checking all of them. Choose the layout, keep it the same across packages, and
document it in `docs/guides/development/README.md` (or wherever the frontend
layout is described) so a new package follows it.

After the change, no `tsconfig*.json` under `frontend/packages/` may use
`exclude` to drop files from type checking (the `check_command` greps for it);
a build config that must omit tests does so through `include` patterns or by
being one of several configs that together cover every file.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `pnpm -r typecheck` (under `make check`) type-checks the unit tests, the
      three Playwright suites and the root-level config files listed above,
      and passes.
- [x] No `tsconfig*.json` under `frontend/packages/` contains `"exclude"`.

### Before merge (verified outside the check command)

- [x] Introducing a type error into one file of each previously unchecked
      group (a `src` test, an `e2e/` spec, `vite.config.ts`) makes
      `pnpm -r typecheck` fail, and reverting it makes it pass.
- [x] The diff adds no `any`, `@ts-ignore`, `@ts-expect-error` or non-null
      assertion to silence an error (reviewed in the diff).
- [x] `pnpm -r build` emits no declarations for test files
      (`find frontend/packages -path '*/dist-types/*' -name '*.test.d.ts'`
      prints nothing).
