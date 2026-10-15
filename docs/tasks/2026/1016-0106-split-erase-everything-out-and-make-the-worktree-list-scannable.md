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
branch: task/1016-0106-split-erase-everything-out-and-make-the-worktree-list-scannable
created_at: 2026-10-15T16:06:00Z
updated_at: 2026-10-15T19:17:26Z
---

# feat(settings): move Erase everything to its own sub-item and make the worktree list scannable

## Overview

Settings → Storage (`frontend/packages/apps/web/src/features/settings/storage/StorageSection.tsx`)
stacks three blocks: prune sessions, the worktree list (`WorktreesBlock.tsx`)
and Erase everything (`EraseBlock.tsx`). With an everyday number of
worktrees the list is long, so Erase everything — rarely used — sits far
below a scroll, and the list itself is hard to act on: removable leftovers and
in-use worktrees are interleaved, the `Clean` badge does not say what it means
(a leftover with no uncommitted changes, safe to remove), and nothing shows how
old a worktree is.

### Erase everything becomes a sub-item of Storage

The Settings menu is the flat `categories` list in
`frontend/packages/apps/web/src/features/settings/SettingsView.tsx`, with the
ids in `frontend/packages/apps/web/src/store/settingsStore.ts`. Add one level:

```
Appearance
Default provider
Storage
    Erase everything
```

- A new category for Erase everything, rendered directly under Storage with its
  label indented to read as Storage's child (the same row style otherwise).
  Its panel holds what `EraseBlock` shows today; Storage's panel no longer
  contains it.
- Keyboard navigation in the menu walks the flat order (Storage, then Erase
  everything) as today; the persisted active category accepts the new id (add
  it to the validated id list) and an unknown stored id still falls back to the
  default.
- Keep a single source for the category list; model the parent/child
  relation as data on the entry (e.g. `parent: 'storage'`), not as a special
  case in the renderer.

### The worktree list

- Group the rows under two headings: worktrees a listed session is using, and
  leftovers (everything removable). Leftovers first, since that is what the
  user comes here to act on; keep each group's count in its heading.
- Replace the bare `Clean` with wording that says what it means for the
  decision, e.g. "No uncommitted changes"; keep the dirty and unregistered
  states' wording consistent with it.
- Show each worktree's last-modified time as a relative age ("3 days ago",
  with the absolute time in a `title`). The wire type `StorageWorktree` has no
  time today: add one in the backend's storage inventory
  (`backend/crates/apps/delta-server/src/storage_inventory/`) — the directory's
  modification time is enough; pick the field name and nullability (a
  directory whose metadata cannot be read) deliberately — and run `make gen`.
- Sort each group newest first so stale leftovers collect at the bottom.

Keep the existing remove flow (confirm panel, per-row errors) unchanged.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `SettingsView` tests: the menu lists Erase everything right after
      Storage, marked as its child (indent class or `aria-level`); selecting it
      shows the erase panel; the Storage panel no longer renders the erase
      block; ↑/↓ moves between Storage and Erase everything; a persisted
      `activeCategory` of the new id restores it and an unknown id falls back
      to the default.
- [x] `WorktreesBlock` tests: rows are split under the two headings with
      counts, leftovers first; each group is sorted newest first; the clean
      state renders the new wording; the age renders from the new field, and a
      `null` time renders without an age.
- [x] A backend test shows the inventory reports a worktree's modification
      time, and `null` when the metadata cannot be read; generated types are
      in sync (`gen-check` in `make check`).

### Before merge (verified outside the check command)

- [x] `make dev` (or `make mock`): open Settings, check the menu shows Erase
      everything indented under Storage, the Storage panel shows the grouped
      list with ages and no erase block, and the erase panel works up to its
      confirm step (do not confirm). The merging session checks this in a
      browser with screenshots and records them in the PR.
