---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0929-1126-fix-keep-the-session-list-when-a-background-refetch-fails
created_at: 2026-09-29T11:26:47Z
updated_at: 2026-09-30T00:12:00Z
---

# fix(web): keep the session list on screen when a background refetch fails

## Overview

`WorkspaceScreen`
(`frontend/packages/apps/web/src/features/workspace/WorkspaceScreen.tsx:434`)
replaces the whole workspace with the "Could not load sessions." screen
whenever `sessionsQuery.isError` is true. TanStack Query sets `isError` on
a failed **background** refetch too, even though `data` still holds the
last good list. So one failed refetch — a server restart, a blip on the
loopback, a request that raced a shutdown — throws away a list the user was
reading and unmounts everything below it, including the embedded terminal
column, whose PTY bridge is torn down with it. Refetches now also run while
a launch is in progress (delta#413's `pane_starting` row), so the window
for this has grown.

### Change

- The full-screen error is for when there is nothing to show: keep it for
  `isError && data === undefined` (the initial load failed). The initial
  `isPending` branch above it is unchanged.
- When `isError` fires with cached `data`, render the workspace from that
  data exactly as on success, and surface the failure through the existing
  notification store (`store/notificationStore.ts`, `showError`) so it
  appears in `NotificationSnackbar`: one entry per failure episode, not one
  per retry tick — TanStack retries and refetches on its own, so push the
  notification when the query transitions into the error state and not
  again until it has recovered and failed anew. Word it as a statement of
  what happened ("Couldn't refresh the session list") with a Retry action
  if the snackbar supports one; if it does not, the message alone is
  enough, since the next refetch interval or a reload recovers. Do not add
  a persistent banner: the list is still usable and the query keeps
  retrying, so a transient notice is the honest shape.
- Keep the terminal column and the focused session mounted across the
  failure — that is the whole point; do not restructure the early returns
  in a way that remounts the tree when the error clears.

### Tests

`WorkspaceScreen.test.tsx` (vitest, MSW mocks as the file already uses):

- the initial sessions request failing renders the full-screen error with
  Retry, and Retry refetches;
- a successful load followed by a failing refetch keeps the list (and the
  focused session's screen) rendered and pushes exactly one error
  notification; a second failing refetch in the same episode pushes no
  second entry; a refetch that succeeds and a later one that fails pushes
  a new entry.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A failed initial sessions load still shows the full-screen error with
      Retry (component test).
- [x] A failed background refetch keeps the cached session list and the
      focused session on screen and pushes one error notification per
      failure episode (component tests).
- [x] `make check` is green.

### Manual / on-hardware (verified by a human before merge)

- [ ] Against a real server with the terminal column open on an open
      session: stop the server, wait for a refetch to fail, and confirm the
      list and the terminal column stay on screen with a snackbar notice;
      start the server again and confirm the notice does not repeat until
      the next distinct failure and the terminal re-attaches (or its own
      reconnect path takes over) without a reload.

## Out of scope

- Changing the refetch cadence or TanStack retry settings.
- The messages query and other per-thread queries: only the session list
  gates the whole workspace.
