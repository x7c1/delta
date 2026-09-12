---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && ! grep -q "rmSync(ARTIFACT_DIR" frontend/packages/apps/web/e2e-fake/support/server.ts && grep -q "globalSetup" frontend/packages/apps/web/playwright.fake.config.ts'
assignee: null
branch: task/0912-1456-fix-keep-e2e-fake-diagnostics-across-worker-reboots
created_at: 2026-09-12T14:56:33Z
updated_at: 2026-09-12T15:33:56Z
---

# fix(e2e-fake): keep every server boot's diagnostics when a worker reboots

## Overview

The fake-mode e2e suite destroys the evidence of the very failures it is
supposed to explain. Its `delta-server` is booted by a worker-scoped
Playwright fixture (`frontend/packages/apps/web/e2e-fake/support/fixtures.ts`),
and `bootServer()` in `e2e-fake/support/server.ts` starts by wiping the
artifact directory `frontend/packages/apps/web/test-results/e2e-fake/`
(`fs.rmSync(ARTIFACT_DIR, …)` around line 290) before writing this boot's
`server.log`, `server.2.log`, … and, on teardown, its `transcripts/`.

Playwright tears a worker down after a failed test and starts a fresh one for
the specs that follow, and the fresh worker re-runs the fixture. So the
second `bootServer()` deletes the first boot's logs and transcripts — exactly
the ones that cover the failure. Observed on 2026-09-12: after
`ws-reconnect.spec.ts:45` failed, the surviving `server.log` was 64 lines
long and began with the *next* server's startup banner; it covered only the
three specs that ran afterwards. The same mechanism explains the
`server.2.log` ENOENT seen on 2026-08-27. The wipe exists for a good reason
(a stale `server.log` from a previous run must not masquerade as this run's),
but it is doing it at the wrong scope: per boot instead of per run.

Make the wipe run once per run, and give each boot its own directory so
nothing a later boot writes can overwrite an earlier boot's evidence.

### Design

1. **Wipe once per run.** Add a `globalSetup` to
   `frontend/packages/apps/web/playwright.fake.config.ts` — a small module
   under `e2e-fake/support/` that removes and recreates the artifact
   directory. `globalSetup` runs once per `playwright test` invocation in
   its own process, which is exactly the "start of the run" scope the wipe
   wants; the fixture's doc already explains why the *server handle* cannot
   live there, and that reasoning is untouched. Remove the `rmSync` from
   `bootServer()` (the gate appended to `check_command` pins that). Move
   the `ARTIFACT_DIR` constant into a module both the setup and
   `server.ts` import, so the path is written once.
2. **One directory per boot.** In `bootServer()`, allocate
   `<ARTIFACT_DIR>/boot-<N>/` where `N` is one more than the number of
   existing `boot-*` entries, and write everything this boot produces under
   it: `server.log`, `server.2.log`, … for its generations, and
   `transcripts/` on teardown. A worker reboot therefore lands in
   `boot-2/` next to `boot-1/`, and the restart spec's second generation
   stays `boot-1/server.2.log`. Keep the append-mode open and the
   "written there directly, not copied on teardown" property for logs.
3. **Follow the references.** Grep the suite for the old layout —
   `server-restart.spec.ts` asserts that every generation's log is
   preserved and may name `test-results/e2e-fake/server.2.log` directly;
   `docs/guides/development/e2e.md` (around line 96) and the header doc of
   `server.ts` describe the layout; `.github/workflows/ci.yml` uploads
   `test-results/` on failure (a directory, so `boot-*/` subdirectories
   are included — confirm, and adjust the path only if it names files).
   Update each to the new layout.
4. **Do not touch** the sweep of stale tmux servers and temp dirs
   (`sweepStaleRuns`), the per-run temp dir, the tmux socket naming, or
   anything about how the server is spawned, restarted, or torn down beyond
   the paths it writes to.

### Pipeline notes

- TypeScript and docs only; no Rust, no wire change.
- `make check` runs the fake-mode suite through the new fixture, so a green
  run proves the boot path works; it takes over ten minutes, and the check
  phase is expected to run it through the driver's long-running path rather
  than a capped worker call.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `bootServer()` no longer wipes the artifact directory
      (`! grep -q "rmSync(ARTIFACT_DIR" …/support/server.ts`, appended to
      `check_command`).
- [x] The fake-mode config declares a `globalSetup` that performs the
      per-run wipe (`grep -q "globalSetup" …/playwright.fake.config.ts`,
      appended to `check_command`).
- [x] The fake-mode suite passes inside `make check` with logs and
      transcripts written under `test-results/e2e-fake/boot-1/`, and the
      restart spec's generation assertion passes against the new layout.

### Manual / on-hardware (verified by a human before merge)

- [ ] Force one spec to fail (e.g. a temporary wrong expectation), run
      `make e2e-fake`, and confirm `test-results/e2e-fake/` holds
      `boot-1/server.log` covering the failed spec **and** `boot-2/` for
      the reboot that followed; then revert the forced failure.

## Out of scope

- Diagnosing the `ws-reconnect.spec.ts:45` failure itself; this change
  exists so the next occurrence leaves evidence.
- Any change to what the server logs or at which level.
