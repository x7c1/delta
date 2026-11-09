---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -qE '\"typescript\": \"\\^6\\.' frontend/package.json && grep -qE '^  typescript@6\\.' frontend/pnpm-lock.yaml"
assignee: null
branch: task/1109-0335-upgrade-typescript-to-6
created_at: 2026-11-08T18:28:00Z
updated_at: 2026-11-09T04:19:25Z
---

# build(deps): upgrade TypeScript to 6

## Overview

Dependabot opened #600 to bump `typescript` from 5.9.3 to 6.0.3 in `frontend/`, but the bump alone does not build: TypeScript 6 types typed arrays over `ArrayBufferLike`, and `frontend/packages/gateway/api-client/src/pty.ts` passes a `Uint8Array<ArrayBufferLike>` to `WebSocket.send`, which rejects it (`TS2345: Argument of type 'Uint8Array<ArrayBufferLike>' is not assignable to parameter of type 'string | Blob | BufferSource'`). #603 fixed that one site on top of the Dependabot branch. This task replaces both with a single change based on the current `main`.

What to do:

1. **Bump `typescript`** in `frontend/package.json` to `^6.0.3` (or the newest 6.x) and update `frontend/pnpm-lock.yaml` with `pnpm install` in `frontend/`. Keep every package that declares `typescript` on the same major
2. **Adapt the code to TypeScript 6** — fix every type error the new compiler reports across the workspace, starting with `PtyConnection.send` in `frontend/packages/gateway/api-client/src/pty.ts` (accept `Uint8Array<ArrayBuffer>` and make its callers produce one). Fix the cause of each error; do not silence it with `any`, `@ts-ignore`, `@ts-expect-error` or a loosened `tsconfig` option
3. **Review options TypeScript 6 deprecates or changes defaults for** — check the `tsconfig*.json` files under `frontend/` against the TypeScript 6.0 release notes, and update any option it deprecates so the build prints no deprecation warning. Do not add `ignoreDeprecations`
4. **Keep the linters on a supported range** — make sure `typescript-eslint` (and any other tool that has a peer dependency on `typescript`) supports TypeScript 6 at the version the lockfile resolves; bump it within its current major if it does not
5. **Update the docs** that name the TypeScript version, if any

Out of scope:

- Other major dependency updates (`@xterm/xterm` 6 is a separate change)

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make check` passes with TypeScript 6
- [x] `frontend/package.json` declares `typescript` `^6.x`, and `frontend/pnpm-lock.yaml` resolves `typescript@6.x`
