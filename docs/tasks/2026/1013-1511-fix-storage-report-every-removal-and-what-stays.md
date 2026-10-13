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
branch: task/1013-1511-fix-storage-report-every-removal-and-what-stays
created_at: 2026-10-13T06:11:00Z
updated_at: 2026-10-13T11:07:55Z
---

# fix(storage): report shared-worktree removals correctly, show what erase leaves behind, and list downloaded updates

## Overview

Four defects in Settings → Storage and the erase/prune paths behind it.

### 1. Removing two sessions that share a worktree leaves a merged branch reported as kept

Two sessions can share one worktree (a second session started on a branch
that already has one). Erase-everything and bulk prune remove sessions one at
a time through the single-session removal
(`backend/crates/domain/delta-usecase/src/interactor/routing/erase_everything.rs`,
`remove_every_session`; prune does the same). The first session's removal
keeps the worktree because the second still uses it
(`KeepReason::InUseByAnotherSession`); the second's removal then deletes the
worktree. The first session's `delta-<id>` branch is never revisited: it
stays, and the report lists it as `WorktreeKept`, even when it is merged and
would have been deleted.

After all rows are removed, run a second pass over the worktrees and branches
that were kept only because another session in the same batch used them, and
apply the normal removal rules to them, so the report reflects the final
state. Cover it with usecase tests for erase and for prune: two sessions on
one worktree, merged and unmerged branch.

### 2. The erase confirmation does not say which branches will remain

Before confirming erase-everything, the panel lists worktree directory names
only. Branches that survive the erase — the unmerged branch of a clean
worktree, branches Delta did not create — are reported only afterwards, and
the desktop app quits when the erase finishes, so its user never sees that
report. Show, in the confirmation, which branches will be left in place and
why, from the worktrees query's branch data (extend the query if it lacks
what is needed). Cover it with a component test.

### 3. Escape in a Storage confirmation panel closes the whole Settings dialog

`frontend/packages/apps/web/src/features/settings/storage/ConfirmPanel.tsx`
is an inline panel inside the Settings dialog, deliberately not a second
dialog. Pressing Escape while it is open closes Settings entirely, discarding
the context the user was in. Escape should cancel the open panel first (the
same as its Cancel button) and close Settings only when no panel is open.
Keep the existing focus handling. Cover both cases with a test.

### 4. The storage inventory omits downloaded updates

`GET /api/storage` (`backend/crates/apps/delta-server/src/storage_inventory/report.rs`)
does not count the `updates/` directory where the desktop app keeps a
downloaded release (several MB), although erase-everything removes it. Add it
to the wire report and show it in the Storage section with the other entries.
Regenerate the TypeScript wire bindings (`make gen`). Cover it with the
report's unit tests.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Usecase tests show erase-everything and bulk prune of two sessions that
      share a worktree end with the merged branch deleted and the report
      listing only what actually remains.
- [x] A component test shows the erase confirmation naming the branches that
      will remain.
- [x] A test shows Escape in an open Storage confirmation panel cancels the
      panel and leaves Settings open; with no panel open, Escape closes
      Settings.
- [x] The storage report includes `updates/` with its size, covered by a
      report unit test, and the generated wire bindings are in sync
      (`make check` runs `gen-check`).

### Before merge (verified outside the check command)

- [x] In a running Delta, Settings → Storage lists the downloaded update, and
      Escape on an open confirmation returns to the Storage section.
