---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -rqE 'void queryClient\\.invalidateQueries\\(\\);' frontend/packages/apps/web/src/data/"
assignee: null
branch: task/1001-0540-fix-keep-the-reconnect-resync-from-being-absorbed-by-a-first-fetch
created_at: 2026-09-30T20:40:09Z
updated_at: 2026-09-30T20:49:48Z
---

# fix(web): keep the reconnect resync from being absorbed by a first fetch in flight

## Overview

When the live channel reconnects, `useSessionEvents.ts`
(`frontend/packages/apps/web/src/data/`) resynchronises the view with a
bare `void queryClient.invalidateQueries();`. That call can lose the same
race `invalidateDiscardingInFlight` in
`frontend/packages/gateway/api-client/src/cache.ts` was written for.
TanStack Query hands back the in-flight promise of a query whose first fetch
is still running, and that fetch's success clears the invalidation. A thread
opened while the channel was down, whose first GET is still in flight at
reconnect, therefore keeps a snapshot without the changes made between that
GET and the reconnect.

Export an all-queries variant from `cache.ts` (for example
`invalidateAll(queryClient)`, built on `invalidateDiscardingInFlight` with the
empty key that matches every query), and call it from `useSessionEvents.ts`
instead of the bare `invalidateQueries()`. Keep the synchronous part of the
helper, which the focus reconciliation relies on. Point the comment at the
call site to the helper's doc comment rather than repeating it.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] An api-client unit test (real `QueryClient`, deferred `queryFn`): the
      all-queries helper, called while a query's first fetch is pending,
      makes `queryFn` run again and the query settles on the second result.
- [x] `useSessionEvents.ts` no longer calls a bare
      `queryClient.invalidateQueries()` (gated in `check_command`).
- [x] `make check` passes, including the fake e2e lane (the reconnect specs).
