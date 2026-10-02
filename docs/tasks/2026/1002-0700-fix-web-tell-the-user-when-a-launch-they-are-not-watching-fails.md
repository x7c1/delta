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
branch: task/1002-0700-fix-web-tell-the-user-when-a-launch-they-are-not-watching-fails
created_at: 2026-10-02T04:11:46Z
updated_at: 2026-10-02T04:38:00Z
---

# fix(web): tell the user when a launch they are not watching fails

## Overview

When a session launch fails (`spawn_failed`), the browser learns of it in one of two
ways today (`frontend/packages/apps/web/src/store/live/spawnsSlice.ts`):

- a launch this window did **not** start raises a snackbar
  (`reportUntrackedSpawnFailure`; the snackbar stack is
  `features/notifications/NotificationSnackbar.tsx`, fixed to the bottom-right);
- a launch this window **did** start raises nothing: its card turns failed (and is
  pinned to the top of the navigator, see `features/navigator/launchesFirst.ts`), and,
  if the user is looking at that session, the main pane shows the failed-session screen.

That leaves one case with no signal at all: the user starts a session, moves to
another session while it is starting, and the launch then fails. The main pane shows the
other session, and the pinned failed card is out of view whenever the navigator is
scrolled down. Typical causes are a launch preparation error, Claude Code exiting
right after starting, or a launch that sat on a first-run prompt past its deadline.

Raise the same kind of snackbar for a tracked launch that fails while the user is not
focused on that session. Do not raise one when the user is focused on it (the
failed-session screen already says so), and never for a launch the user cancelled
themselves (`cancelled: true` from Close on a starting session). Reuse the existing
notification path and wording style of the untracked case; the message should name the
session in the user's terms so they can find it, and, if the snackbar component supports
an action, offer to open the failed session. Keep the untracked behaviour unchanged.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A store or component test shows that a tracked launch failing while another
      session is focused raises a snackbar naming it.
- [x] A test shows no snackbar for a tracked launch that fails while it is the focused
      session, and none for a cancelled launch.
- [x] The existing untracked-failure snackbar test still passes unchanged.

### Manual / on-hardware (verified by a human before merge)

- [ ] Start a session in a directory that makes the launch fail, switch to another
      session before it fails, and a bottom-right notification tells you which session
      failed.
