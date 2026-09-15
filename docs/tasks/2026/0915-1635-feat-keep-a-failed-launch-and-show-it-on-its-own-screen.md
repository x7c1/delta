---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check'
assignee: null
branch: task/0915-1635-feat-keep-a-failed-launch-and-show-it-on-its-own-screen
created_at: 2026-09-15T16:35:09Z
updated_at: 2026-09-15T18:33:41Z
---

# feat(session): keep a launch that failed, and show the failure on its own screen

## Overview

When a launch never binds, Delta deletes the session row outright. The failure
then has nowhere to live, and everything around it is built to compensate for
that absence.

`clean_up_failed_spawn_row`
(`backend/crates/domain/delta-usecase/src/interactor/lifecycle/cancel_launch.rs:195-205`)
deletes the row whenever the session ingested no messages, keeping `Failed` only
for the defensive case of a session that somehow holds data. Its reasoning is
that a spawn which never bound ingested nothing, so there is no conversation to
keep. That is true about messages and false about everything else: the row holds
the prompt text, the working directory, the repository and branch, the launch
options, the originating PR number, the failure reason, and the pane token —
which is the entire list of what someone would want to look at to understand why
their session did not start.

What the deletion costs, all of it visible in the code today:

- The browser has to teleport focus away when the failure lands, because the
  screen the user is on is about to describe a session that no longer exists
  (`frontend/packages/apps/web/src/data/applySessionEvent.ts`, the `isFocused`
  branch).
- The Retry / Dismiss card has to live on the new-session surface, which is
  unrelated to the session that failed
  (`frontend/packages/apps/web/src/features/composer/usePendingSends.ts`).
- The text the user wrote has to be carried out on the `SpawnFailed` event and
  held browser-side, because the rows holding it are deleted by cascade.
- A user who is looking at something else gets **nothing at all**: the navigator
  row silently disappears and no notice is shown. Confirmed by hand on
  2026-09-15 — a launch started against a repository whose startup stalls was
  reaped after 30 seconds while the browser was on another session, and nothing
  appeared anywhere.

Keep the row instead. A failed launch becomes an ordinary thing the user can
open, read, retry, and remove, and each of the compensations above can go.

### What to build

Stop deleting: `clean_up_failed_spawn_row` marks the session `Failed` in every
case. `SessionStatus::Failed` already exists in
`backend/crates/domain/delta-model/src/session_status.rs`; its doc says `Failed`
only survives for a session holding data worth keeping, and that sentence stops
being true here — correct it.

A failed session then needs to be listed, opened, and acted on:

- It appears in the session list with the other non-live sessions. Where it sorts
  is a judgement to make and state in the PR description; `list_sessions_page`
  leads with the live head and then streams the closed ones.
- Opening it shows why it failed — the reason `SpawnFailed` carries when Delta can
  name one — together with the prompt text that was never delivered, and offers
  **Retry** and **Remove**. Retry re-sends against the same workdir, launch
  options and provider, exactly as the existing card does. Remove is the existing
  `DELETE /api/sessions/{id}`.
- The navigator marks it as failed, distinctly from closed.

Then take away what it replaces. At minimum the focus teleport on `spawn_failed`
is no longer needed — the user can stay where they are, and if they are on the
failed session it now has something to show. Judge the new-session-surface card
and the browser-side text hand-back on the same basis: whatever the failed
session's own screen now covers should not be duplicated elsewhere. Say in the PR
description what was removed and what was deliberately kept.

### States a launch can be in when it fails

| | expected |
| --- | --- |
| the user is on the failing session's screen | the screen shows the failure in place; no teleport |
| the user is on another session | they stay there; the navigator row turns failed rather than vanishing |
| the user is on the new-session screen | it stays usable for a new launch; the failure is on its own screen |
| the failure is a cancel (the user closed a starting session) | unchanged in substance: the row is the user's own doing, and the wording names the close rather than a breakage |
| a spawn this browser never tracked fails | unchanged: `reportUntrackedSpawnFailure` already reports it |
| Retry from the failed session succeeds | the user lands in the new session; the failed row does not linger as a duplicate |

### Out of scope

- **Notifying a user who is elsewhere.** With the row kept and marked, the
  navigator carries the signal. Whether a transient notice is also warranted is a
  separate judgement, deliberately not made here.
- **The stale prompt row on the new-session screen.** The first send of a
  `spawning` spawn is rendered on the new-session surface with no label; that is
  its own problem and is not settled by this change.
- **Sessions that failed after ingesting messages.** That path already keeps the
  row and is unchanged.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A backend test asserts that a spawn reaped before it bound leaves its
      session row in place with status `failed`, rather than deleting it, and
      that its recorded workdir, repository and first prompt survive.
- [x] A backend test asserts the failed session is returned by the session list
      endpoint, in the position the PR description states it should occupy.
- [x] An e2e-fake spec asserts that when a launch fails while the browser is on
      another session, the navigator shows that session as failed and focus does
      not move.
- [x] An e2e-fake spec asserts that opening a failed session shows the failure
      reason and the prompt that was never delivered, and offers Retry and Remove.
- [x] An e2e-fake spec asserts Retry from the failed session's screen starts a
      new launch with the same working directory and launch options.
- [x] An e2e-fake spec asserts Remove deletes the failed session and it leaves
      the list.
- [x] A test asserts the cancel path still reads as a cancel and not as a
      breakage, per the row in the state table.

### Manual / on-hardware (verified by a human before merge)

- [ ] In the running app, start a session against a repository whose launch
      stalls, stay on another session, and confirm after the deadline that the
      navigator shows it as failed, that opening it explains why, and that Retry
      and Remove both work.
