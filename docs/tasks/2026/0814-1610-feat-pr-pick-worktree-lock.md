---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: 'cd backend && cargo fmt --all -- --check && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && BEFORE=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && make gen && AFTER=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && [ "$BEFORE" = "$AFTER" ] && cd frontend && pnpm -r build && pnpm -r typecheck && pnpm -r test && pnpm -r lint && cd .. && make e2e && ! git grep -n "newSessionSelectedPrUrl" -- ":!docs/tasks"'
assignee: null
branch: task/0814-1610-feat-pr-pick-worktree-lock
created_at: 2026-08-14T16:10:00Z
updated_at: 2026-08-14T17:45:00Z
---

# feat(new-session): lock the worktree options to the PR's head branch when a PR is picked

## Overview

Picking a pull request in the new-session dialog's PR tab already commits the
session to that PR's head branch: `onPickPr`
(`frontend/packages/apps/web/src/features/new-session/tabs/PRTab.tsx` ~line 80)
sets the workdir to the local clone, enables the worktree, and sets the start
point to `{ kind: 'use_remote_branch', name: pr.head_ref }`. Yet the worktree
UI below the composer (`features/composer/WorktreeOptions.tsx`) still renders
the full generic selector — the "Start in an isolated git worktree" checkbox,
the Current HEAD / Latest `<default_branch>` / Other remote branch radios, and
the use-vs-new branch-mode choice. None of those choices make sense for a PR
pick: the session is for the PR, so the branch is decided. Worse, the user can
toggle the worktree off or move the start point away from the PR branch,
producing a session that silently is not on the PR at all.

The root cause is that `WorktreeOptions` does not know *how* the workdir was
picked. Fix it by making the pick's provenance first-class state:

- **Store**: replace the standalone `newSessionSelectedPrUrl` field
  (`store/composerStore.ts` ~line 166) with a workdir-provenance value set
  atomically with the workdir, e.g.
  `newSessionWorkdirSource: { kind: 'directory' } | { kind: 'pr', url, number,
  repo_owner, repo_name, head_ref }`. Every existing `setNewSessionWorkdir`
  call site (Repository tab, recent-workdir picks, dialog resets — enumerate
  them all) supplies `directory` provenance; only the PR pick supplies `pr`.
  This also structurally fixes a latent bug: `setNewSessionWorkdir`'s reset
  side-effects (composerStore ~line 270) do not clear `newSessionSelectedPrUrl`
  today, so a PR row can keep its "picked" highlight after the user has moved
  on to a directory pick.
- **UI**: when provenance is `pr`, `WorktreeOptions` renders a locked one-line
  summary instead of the selector — same section chrome, no checkbox, no
  radios, no branch-mode choice, no branch picker:

  > Start in an isolated git worktree
  > On `<head_ref>` — PR #<number>'s head branch.

  The worktree stays force-enabled with
  `{ kind: 'use_remote_branch', name: head_ref }`. Note this loses nothing:
  `use_remote_branch` already reuses a worktree that has the branch checked
  out, **including the main tree**, so "work in the main tree" needs no
  opt-out. Directory provenance keeps today's full selector byte-identical.
- **PR row highlight**: `PrRow`'s `isSelected` derives from the provenance
  (`kind === 'pr' && url` match) instead of the removed URL field.

The send payload is unchanged: the composer keeps attaching the worktree
settings exactly as `onPickPr` writes them today.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Picking a PR renders the worktree section as the locked summary: no
      checkbox, no start-point radios, no branch-mode radios, no branch picker;
      the summary names the PR's `head_ref` and PR number (component test).
- [x] Picking a directory (Repository tab / recent workdir) renders the full
      selector exactly as today (existing `WorktreeOptions` tests still pass
      unchanged).
- [x] Picking a PR and then picking a directory restores the full selector and
      clears the PR row highlight — no row shows `data-selected="true"`
      (component test pinning the provenance reset).
- [x] Starting a session from a PR pick sends the worktree as
      `{ kind: 'use_remote_branch', name: <head_ref> }` (test at the composer
      submit layer).
- [x] `newSessionSelectedPrUrl` no longer exists:
      `! git grep -n "newSessionSelectedPrUrl" -- ":!docs/tasks"` is appended
      to `check_command`.
- [x] The PR row highlight still appears on the picked row while provenance is
      `pr` (component test).

### Manual / on-hardware (verified by a human before merge)

- [x] In the real app, picking a PR shows the locked worktree line and the
      started session lands in a worktree on the PR's head branch.

## Out of scope

- Cross-fork PR head resolution (existing limitation, documented at the pick
  site, unchanged).
- Any change to the PR tab's `has_local_clone` handling — the no-clone
  dead-end is a separate task.
- The worktree UI for directory-provenance picks.
