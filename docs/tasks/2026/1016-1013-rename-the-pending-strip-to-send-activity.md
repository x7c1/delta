---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! git grep -nE 'PendingQueue|usePendingSends|PendingEntry|PendingSurface|pendingEntries|pendingSurface' -- frontend/packages"
assignee: null
branch: task/1016-1013-rename-the-pending-strip-to-send-activity
created_at: 2026-10-16T10:11:14Z
updated_at: 2026-10-16T17:08:06Z
---

# refactor(web): rename the pending strip to send activity

## Overview

The strip above the composer is implemented by `PendingQueue`
(`frontend/packages/apps/web/src/features/composer/PendingQueue.tsx`) and
fed by `usePendingSends` (`usePendingSends.ts`), with the types
`PendingEntry` and `PendingSurface`. The names say "pending queue", but the
rows are three kinds: a send the server still holds open (`queued` or
`dispatched`), a send the server already matched to its transcript line whose
turn has not ended, and a submit whose POST is in flight or was rejected. Only
the first kind is the server's open-send queue, so the name misstates the
set. The visible heading was already changed from "Pending sends" to "In
progress" for the same reason; the identifiers were left as they were.

Rename the identifiers to say what the strip shows, the sends the user is
still waiting on: `PendingQueue` → `SendActivityStrip`, `usePendingSends` →
`useSendActivity`, `PendingEntry` → `SendActivityEntry`, `PendingSurface` →
`SendActivitySurface`, `PendingQueueProps` → `SendActivityStripProps`, and
the `pendingEntries` / `pendingSurface` locals in
`features/transcript/TranscriptPane.tsx` and
`TranscriptComposerStack.tsx` to `sendActivity` / `sendActivitySurface`.
Rename the two source files and their test files to match
(`SendActivityStrip.tsx`, `useSendActivity.ts`, and the `.test.tsx`
siblings). Update every import, the prose in the comments that name the old
identifiers (`TranscriptComposerStack.tsx`, `data/applySessionEvent.ts`,
`data/useSessionEvents.ts`, `store/live/noticesSlice.ts`, and the comments
inside the renamed files), and the doc comment at the top of
`useSendActivity.ts` so it states the three row kinds and that the server's
open-send list is only one of them.

Keep the `data-testid` values (`pending-item`, `pending-text`,
`pending-fail-reason`): the e2e suites select on them and they are test
hooks, not vocabulary. Keep the visible text, the row order, and the
behaviour; this is a rename.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `PendingQueue`, `usePendingSends`, `PendingEntry`, `PendingSurface`,
      `pendingEntries` and `pendingSurface` no longer appear under
      `frontend/packages` (`git grep` gate appended to `check_command`).
- [x] `SendActivityStrip.tsx` and `useSendActivity.ts` exist with their
      tests, and the web app typechecks, lints and passes its unit tests
      with the new names (`make check`).
- [x] The `data-testid` values are unchanged and the mock and fake e2e
      suites that select on them pass (`make check`).

## Out of scope

- Any change to what the strip shows or how rows are ordered.
- The backend's send vocabulary (`queued`, `dispatched`, open-send list).
