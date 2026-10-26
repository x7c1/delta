---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -rqi 'coalesce' frontend/packages/gateway/api-client/src/cache.test.ts"
assignee: null
branch: task/1026-0530-coalesce-session-list-refetches
created_at: 2026-10-26T05:30:37Z
updated_at: 2026-10-26T06:17:00Z
---

# perf(web): coalesce session list refetches when live events arrive together

## Overview

`applySessionEvent` (`frontend/packages/apps/web/src/data/applySessionEvent.ts`)
calls `invalidateSessions` (`frontend/packages/gateway/api-client/src/cache.ts`)
on `transcript_updated`, `turn_started`, `turn_completed`,
`turn_interrupted`, `subagent_started` and `subagent_finished`, as well as on
the lifecycle events (`session_registered`, `session_opened`,
`session_closed`, `session_removed`, `spawn_pane_ready`, `spawn_failed`).
Each call cancels the list fetch in flight and starts a new
`GET /api/sessions` in the same tick. Nothing coalesces them.

Events arrive in bursts: a turn's end brings `transcript_updated` and
`turn_completed` together, and several open sessions each bring their own.
Each one starts a full refetch of every loaded page. Only the last response
is used, and cancelling a request on the client does not stop the server
from assembling the earlier ones. Since every list fetch now also queries
each live session's actor for its live state, the waste grows with the
number of open sessions.

### The change

Coalesce the list refetches that the high-frequency events trigger into one
trailing refetch:

1. **Keep the stale-response guard immediate.** At the event, mark the
   session list stale and discard any list fetch in flight, exactly as now,
   so a list the server assembled before the event can never land. This is
   what keeps a `turn_completed` from being undone by an older list that
   still reports `in_flight` (see the comments in `applySessionEvent.ts` and
   `useSeedRunningFromSessionList.ts`). Keep the first-fetch case that
   `invalidateDiscardingInFlight` handles.
2. **Delay only the new fetch.** Start it once no further such event has
   arrived for a short window (about 250 ms), with a maximum wait (about
   1 s) so a session that keeps producing transcript lines still refreshes
   the list. Pick the values, name them as constants, and say why in a
   comment.
3. **Leave the lifecycle events immediate.** They keep calling the immediate
   `invalidateSessions`; the workspace's focus reconciliation reads
   `isFetching` right after them. An immediate invalidation also runs any
   pending coalesced refetch now (it is the same refetch), so nothing is left
   waiting on a timer.
4. The coalescing state belongs to the query client it refetches, not to a
   module-level singleton that would leak between tests or clients.

Update the doc comments of `invalidateSessions` and of the event handling in
`applySessionEvent.ts` so they describe which events refetch at once and
which coalesce.

### Out of scope

- The per-session `/sends` and thread-message invalidations.
- Any server-side change.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A test in `frontend/packages/gateway/api-client/src/cache.test.ts`
      (its name contains "coalesce") shows several coalesced invalidations
      inside the window producing exactly one list request, issued after the
      window.
- [x] A test shows that a list fetch in flight when a coalesced event
      arrives is discarded, not applied.
- [x] A test shows the maximum wait: a steady stream of events closer than
      the window still refetches once the maximum wait has passed.
- [x] A test shows an immediate invalidation (a lifecycle event) refetching
      at once and leaving no coalesced refetch pending.
- [x] Existing tests of the running-spinner seed and the stale-list race
      still pass (adjusted for the delay with fake timers where needed, not
      removed).
- [x] `make check` passes.
