---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1001-0412-fix-keep-a-live-event-invalidation-that-lands-during-a-first-fetch
created_at: 2026-09-30T19:12:51Z
updated_at: 2026-09-30T19:45:28Z
---

# fix(web): keep a live-event invalidation that lands during a query's first fetch

## Overview

A thread can show no messages even though the backend has ingested them. It
happens when a live event invalidates a query whose **first** fetch is still in
flight:

1. The browser starts the initial `GET /api/threads/<id>/messages` and gets an
   empty list, because the transcript has not been ingested yet.
2. A few milliseconds later the backend ingests the transcript and pushes
   `transcript_updated`. `applySessionEvent.ts` calls
   `invalidateThreadMessages` (`frontend/packages/gateway/api-client/src/cache.ts`).
3. TanStack Query (query-core 5.101) ignores `cancelRefetch` while
   `state.data === undefined`: it hands back the in-flight promise instead of
   starting a new fetch. When that fetch succeeds, it clears `isInvalidated`.
   The pre-ingest empty list is therefore kept as fresh data, and with
   `MESSAGES_STALE_TIME = 30_000` (`query-hooks.ts`) nothing refetches it.

A turn that is held open — a prompt queued mid-turn and waiting for Escape, for
example — has no later event to repair the view. This is what failed
`e2e-fake/queued-prompt.spec.ts:21` in CI (`expect(messages).toHaveCount(2)`
got 0). The server log and trace show the ingest committed about 14 ms after
the messages GET was answered, followed by a refetch of sends, threads and
sessions but never of messages. The flakes previously seen in
`echo-deadline:43` and `ws-reconnect:104` also hold a turn open and likely share
this cause.

Fix: add one helper in `cache.ts` (for example
`invalidateDiscardingInFlight(queryClient, queryKey)`) that cancels the query
and then invalidates it —
`void queryClient.cancelQueries({ queryKey }).then(() => queryClient.invalidateQueries({ queryKey }))`.
Cancelling reverts an in-flight first fetch, so the active observer fetches
again and sees the post-event state. Use the helper in every event-driven
invalidation helper in `cache.ts` that can race a first fetch this way:
`invalidateThreadMessages`, `invalidateSessionSends`,
`invalidateSessionThreads` and `invalidateSessions`. Check the rest of
`cache.ts` for the same pattern. Write a doc comment on the helper explaining
the race, so a later reader does not "simplify" it back into a plain
`invalidateQueries`.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] An api-client unit test, using a real `QueryClient` and a deferred
      `queryFn`, calls `invalidateThreadMessages` while the query's first
      fetch is pending. It asserts that `queryFn` runs a second time and that
      the query settles on the second (post-event) result. Without the helper
      the test fails: check this by temporarily reverting the helper while
      authoring.
- [x] Every invalidation helper in `cache.ts` that the live-event path
      (`applySessionEvent.ts`) calls goes through the new helper.
- [x] The existing api-client and web test suites pass (`make check`,
      including the fake e2e lane).
