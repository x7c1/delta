---
status: completed
pipeline_phase: null
plan: null
follow_up_of: docs/tasks/2026/0911-1416-fix-the-streaming-spec-race-against-the-transcript-refetch.md
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && ! grep -q "assertion expects 1" frontend/packages/apps/web/src/data/applySessionEvent.ts'
assignee: null
branch: task/0911-1617-follow-up-fix-the-streaming-spec-race-against-the-transcript-refetch
created_at: 2026-09-11T07:17:31Z
updated_at: 2026-09-11T07:29:27Z
---

# docs(web): drop a stale streaming-spec reference from the thread-refetch comment

## Overview

The comment above the unconditional thread refetch in
`frontend/packages/apps/web/src/data/applySessionEvent.ts` explains why the
refetch is not gated on focus, and illustrates the old behaviour by naming an
assertion in `frontend/packages/apps/web/e2e-fake/streaming.spec.ts`. That
assertion no longer exists: the spec stopped counting transcript lines during
the streaming window, so "where the assertion expects 1" now points at nothing.

Replace the three lines at 112-114, anchored by the text
`lines had all landed together on the same refetch, surfacing as 3`:

```
      // lines had all landed together on the same refetch, surfacing as 3
      // message-items in the streaming-window of `streaming.spec.ts` where the
      // assertion expects 1. Routing by `event.thread_id` (always carried by
```

with:

```
      // lines had all landed together on the same refetch, leaving the
      // transcript empty for the whole streaming window instead of showing the
      // user prompt right away. Routing by `event.thread_id` (always carried by
```

The surrounding explanation stays as it is. This is comment text only: no
behaviour changes, and nothing outside that comment is touched.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The comment in `frontend/packages/apps/web/src/data/applySessionEvent.ts`
      no longer refers to a removed assertion:
      `grep -q "assertion expects 1" frontend/packages/apps/web/src/data/applySessionEvent.ts`
      finds nothing (this grep is appended to `check_command`), and `make check`
      still passes.
