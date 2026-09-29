---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make -j check"
assignee: null
branch: task/0929-1141-build-make-check-a-dependency-graph-and-compare-gen-check-before-and-after-regeneration
created_at: 2026-09-29T11:41:06Z
updated_at: 2026-09-29T12:46:00Z
---

# build(make): run `check` as a dependency graph under `-j`, and make `gen-check` compare the bindings before and after regeneration

## Overview

Two defects of the pre-PR gate, both in `Makefile`.

### 1. `gen-check` compares against HEAD, so it fails before a commit

`gen-check` (`Makefile:54-59`) runs `make gen` — which overwrites
`frontend/packages/gateway/wire-gen/src/generated/` in place — and then
fails if `git status --porcelain` shows anything under wire-gen. That
compares the regenerated bindings with **HEAD**, which is only the right
question on a clean checkout (CI). Locally, a developer who changed a wire
type and ran `make gen` correctly has bindings that match the Rust
contract but differ from HEAD, so `make check` fails until they commit —
delta#413's run had to make a throwaway commit to get through. The gate's
own question is "are the bindings on disk what the contract generates
now?"; whether they are committed is CI's concern, and CI's clean checkout
answers it for free (working tree == HEAD there, so the same comparison
covers both).

Change `gen-check` to compare **before and after regeneration**:

- give `export-ts` (`backend/crates/gateway/delta-wire/src/bin/export-ts.rs`)
  an optional output directory (a positional arg or `--out-dir`) defaulting
  to today's `OUT_DIR`, so `gen-check` can generate into a temporary
  directory without touching the working tree;
- `gen-check` generates into a temp dir and `diff -r`s it against
  `wire-gen/src/generated/`; a difference prints the diff and fails with
  the existing "run 'make gen' and commit the result" message; no
  difference passes. It must no longer depend on `gen` or on git state.
  Clean the temp dir on every exit path;
- make CI call `make gen-check` instead of its inline copy of the old
  check (`.github/workflows/ci.yml:55-60`), so there is one
  implementation. On CI's clean checkout the before/after comparison also
  catches bindings that were regenerated but not committed, which is what
  the inline step was for.

### 2. `check` is a serial script that takes twice as long as CI

`check` (`Makefile:107-115`) lists eight steps in one recipe, so they run
one after another: over ten minutes locally, while CI runs the same content
as three parallel jobs and finishes in about five (backend 3:10, frontend
2:30, e2e-fake 5:08 measured 2026-09-17). Express the steps as make targets
with their real dependencies so `make -j check` (or `make -j4 check`) runs
independent columns concurrently and nothing runs before what it needs:

- backend: `fmt --check` → `cargo build` → { `cargo test`, `clippy`,
  `gen-check` } (`gen-check` needs the built `export-ts`; `cargo run`
  reuses the build);
- frontend: `pnpm install` (if the recipe includes it) → `pnpm -r build` →
  { `typecheck`, `test`, `lint`, `e2e` };
- `e2e-fake` needs both the backend build (delta-server, fake-claude,
  fake-codex) and the frontend build;
- `vendor-codex-schema-check`, `vendor-codex-schema-test` and
  `e2e-real-gate-test` depend on nothing;
- `check` depends on all leaves. Keep every existing target name and its
  standalone behaviour (`make e2e-fake` alone still works), keep the
  coverage identical to today's list, and keep `check` working without
  `-j` (serial order then follows the graph).

Two Playwright suites and two package managers in parallel share the
machine: pin the ports and sockets they already pin (e2e on 5199, e2e-fake
on its per-run tmux socket and temp DB) and make sure the two suites do not
share a Playwright output directory or a `.tmp/` log path. If `cargo` and
`pnpm` steps collide on a shared cache or lockfile, serialise only those
two with an order-only prerequisite rather than flattening the graph.

Update `docs/guides/development/README.md` where `make check` and the
gate are described (the `## check:` help line in the Makefile too) to say
it is a dependency graph and how to run it with `-j`.

### Flakes under load

A load-dependent flake surfaced by parallelism is treated as a defect of
a fixed waiting window in the test, to be fixed on the test side, never by
lengthening the window. This task does **not**
fix such tests: if `make -j check` reproduces one of the known
load-dependent flakes (echo-deadline:43, queued-prompt:21,
ws-reconnect:104, fake-claude `full_loop` 20 s), record which one, with
the artifacts path, in the PR description and leave the Makefile change
intact. A green serial `make check` plus a documented flake list is an
acceptable end state for this PR; the flakes then become their own task.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make gen-check` passes on a working tree whose bindings match the
      contract but are not committed, and fails when a generated file is
      stale, without modifying the working tree in either case (a shell
      test under `scripts/tests/` in the style of the existing ones, run by
      `make check`).
- [x] `export-ts` accepts an output directory and defaults to the
      committed location.
- [x] `make -j check` runs the same steps as before with the dependency
      order above, and `make check` without `-j` still passes.
- [x] CI's backend job calls `make gen-check` and has no inline copy of the
      check.

### Manual / on-hardware (verified by a human before merge)

- [ ] Time `make -j check` on the dogfooding machine against the last
      serial run and record both in the PR; confirm the wall clock is in the
      range of CI's longest job rather than the serial sum.
- [ ] Any flake reproduced under `-j` is named in the PR with its
      artifacts, not silenced.

## Out of scope

- Fixing load-dependent flakes (see above).
- Changing what `check` covers, or the CI job split.

