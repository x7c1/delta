---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -n 'per-spawn workdirs' scripts/dev.sh"
assignee: null
branch: task/1018-0441-match-meter-readout-and-fix-stale-docs
created_at: 2026-10-17T19:41:00Z
updated_at: 2026-10-17T22:01:59Z
---

# fix(web): announce meters as displayed and correct two stale descriptions

## Overview

Three small corrections, each too small to justify a pipeline run of its own.
They share no code, so review them as independent hunks.

### 1. `Meter` announces a different number from the one on screen

`frontend/packages/ui/ui-kit/src/Meter.tsx` sets `aria-valuenow` to the
clamped, unrounded value, while every caller renders a rounded percentage
next to it. At 42.7 % the screen shows `43%` and a screen reader announces
42.7. Give `Meter` an `aria-valuetext` that matches what the caller shows, and
use it from both callers: the context-usage meter on the composer rail
(`frontend/packages/apps/web/src/features/composer/ContextUsageMeter.tsx`,
which shows `Math.round(clamped)%`) and the rate-limit rows in
`frontend/packages/apps/web/src/features/navigator/NavigatorPane.tsx`.
Check how the rate-limit rows format their visible number and match that
exactly rather than assuming it is the same rounding. Decide whether `Meter`
should take the text from the caller or format it itself from a rounding
rule, and document the choice on the prop; keep `aria-valuenow` as a number.
Extend `frontend/packages/ui/ui-kit/src/Meter.test.tsx` and the callers'
tests.

### 2. `make dev` says it creates per-spawn workdirs

`scripts/dev.sh` logs
`Data directory: $DEV_DATA_DIR (database, session settings, tmux config and per-spawn workdirs)`.
Sessions now always start in a repository, so nothing creates per-spawn
working directories under the dev data directory any more. Drop that item
from the message, and check the other `log` lines in the script for the same
claim.

### 3. `live-channels.md` does not say `subagent_started` is sent again after a re-adoption

When a Claude pane that survived a server restart is re-adopted, the
background subagents it launched that have not completed are restored to the
running set. `docs/guides/api/sends.md` (the `running_subagents` item) says
so; the `subagent_started` entry in `docs/guides/api/live-channels.md`
(under "Streaming and subagents") does not. Check in the code whether a
re-adoption broadcasts `subagent_started` for each restored entry
(`backend/crates/domain/delta-usecase/src/interactor/lifecycle/adopt_pane.rs`
and the running-set rebuild it calls), and state in `live-channels.md`
what a client receives in that case — a resent `subagent_started`, or
nothing live with the sends envelope as the only source — linking to the
`sends.md` text rather than repeating it.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A `Meter` test shows the element exposes an `aria-valuetext` equal to
      the text its caller displays (e.g. `43%` for 42.7), while
      `aria-valuenow` stays numeric.
- [x] Tests of the context-usage meter and of a rate-limit row assert that
      their meter's `aria-valuetext` matches the percentage they render.
- [x] `scripts/dev.sh` no longer mentions per-spawn workdirs (the grep in
      `check_command`).

### Before merge (verified outside the check command)

- [ ] Reading the diff of `docs/guides/api/live-channels.md` against the code
      confirms that what it says a client receives after a re-adoption is what
      the server does.
