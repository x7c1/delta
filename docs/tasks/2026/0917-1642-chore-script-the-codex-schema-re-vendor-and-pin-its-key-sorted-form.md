---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && scripts/vendor-codex-schema.sh --check && [ -z \"$(git diff --name-only origin/main -- backend/crates/gateway/codex-agent/vendor/app-server-schema | grep '\\.json$')\" ] && ! grep -rqi 'verbatim' --include=README.md backend/crates/gateway/codex-agent/vendor/app-server-schema/"
assignee: null
branch: task/0917-1642-chore-script-the-codex-schema-re-vendor-and-pin-its-key-sorted-form
created_at: 2026-09-17T07:42:25Z
updated_at: 2026-09-17T09:20:05Z
---

# chore(codex): script the Codex schema re-vendor and pin its key-sorted form

## Overview

`backend/crates/gateway/codex-agent/vendor/app-server-schema/` holds the JSON
Schema that `codex app-server generate-json-schema` emits, as the ground
truth Delta's Codex wire types are reconciled against. Its README calls the
files "generated verbatim" and gives the bare generator command as the way
to refresh them.

Neither is accurate enough to follow:

- The generator's output order is **not stable**: the same Codex version can
  emit the same definitions in a different order from one run to the next.
  A re-vendor done by hand with the documented command therefore produces a
  diff full of reorderings, in which the real changes are hard to find.
- The 274 vendored files are in fact already key-sorted: each is
  byte-identical to `jq -S -j .` of itself (sorted keys, two-space indent, no
  trailing newline). That normal form is what makes a re-vendor diff
  readable, but nothing records it or enforces it, and "verbatim" says the
  opposite.
- Which generator outputs are vendored and which are dropped (the v1 stub,
  most loose top-level per-type files) is described in prose only, so a
  re-vendor has to re-derive the selection by reading.

The drift canary compares structure, not bytes, so none of this affects it.

### Change

- Add `scripts/vendor-codex-schema.sh` with two modes:
  - default: run the generator into a temp directory
    (`${DELTA_CODEX_BIN:-codex} app-server generate-json-schema --out <tmp>`),
    select exactly the outputs the README's "Files" section lists (the two
    combined documents, `ServerRequest.json`, the three
    `*RequestApprovalParams.json` / `*RequestApprovalResponse.json` pairs, and
    `v2/*.json`), normalise each with `jq -S -j .`, and replace the vendored
    files with the result — removing vendored files the generator no longer
    emits, and leaving `README.md` alone. It prints the generator's version
    and reminds the operator to bump `VENDORED_CODEX_VERSION`
    (`codex-agent/src/schema.rs`) and the README's version pin when it
    differs from the pin.
  - `--check`: without running the generator, verify every vendored `.json`
    is byte-identical to `jq -S -j .` of itself, listing offenders and
    exiting non-zero otherwise. This needs only `jq`.
  Follow the conventions of the existing scripts in `scripts/` (strict mode,
  `log`/`die` helpers, a usage header, clear errors when `jq` or the codex
  binary is missing).
- Makefile: `vendor-codex-schema` (runs the script) and
  `vendor-codex-schema-check` (runs `--check`), each with a `##` help line
  like the other targets; add the check to `make check` next to `gen-check`.
  Add the same check as a step of the backend job in
  `.github/workflows/ci.yml` (`jq` is preinstalled on the runner image), so
  CI and `make check` keep covering the same things.
- README of the vendored directory: replace the bare generator command in
  the version-pin table and the re-vendor guidance with the make target;
  replace "generated verbatim" with what is true — generated, then
  key-sorted — and say in one short paragraph why (unstable generator
  order, readable re-vendor diffs) and that the canary is structural and
  unaffected. Update `docs/guides/development/` where it lists make targets
  or describes re-vendoring.
- The vendored `.json` files themselves must not change in this PR: they are
  already in the normal form. `--check` passing on the untouched files is
  the proof.

### Verifying the default mode without the pinned Codex

The pinned version is `0.153.4`, and the machine running this task may have a
different Codex or none. Do not re-vendor in this PR. Exercise the default
mode against a stub: a test (shell, under `scripts/tests/` beside the
existing script tests, wired wherever those are run) that points
`DELTA_CODEX_BIN` at a fake generator emitting a small unsorted fixture tree
— including a v1 file and an unlisted top-level file that must be dropped —
into a throwaway target directory, and asserts the selection, the sorting,
the removal of a stale vendored file, and that `--check` then passes. The
script therefore needs its target directory overridable (an env var is
enough).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `scripts/vendor-codex-schema.sh --check` passes on the vendored files
      as they are, and no vendored `.json` file is modified by this PR
      (`check_command` runs the check and asserts `git diff --name-only
      origin/main` lists no `.json` under the vendored directory).
- [x] `--check` fails on an unsorted file (covered by the script test).
- [x] The default mode, run against a stub generator, vendors exactly the
      listed outputs key-sorted, drops v1 and unlisted top-level files, and
      removes a stale vendored file (script test, run by `make check`).
- [x] `make check` runs the vendored-form check, and the CI backend job has
      the same step.
- [x] The vendored README no longer says "verbatim" (`check_command` greps).

### Manual / on-hardware (verified by a human before merge)

- [ ] On a machine with the pinned Codex CLI: `make vendor-codex-schema`
      leaves the working tree unchanged.
