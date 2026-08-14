---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: 'cd backend && cargo fmt --all -- --check && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && BEFORE=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && make gen && AFTER=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && [ "$BEFORE" = "$AFTER" ] && cd frontend && pnpm -r build && pnpm -r typecheck && pnpm -r test && pnpm -r lint && cd .. && make e2e && ! git grep -inE "scan[ _-]?roots?" -- ":!docs/tasks"'
assignee: null
branch: task/0814-1611-refactor-rename-scan-roots-to-clone-roots
created_at: 2026-08-14T16:11:00Z
updated_at: 2026-08-14T19:55:00Z
---

# refactor(workdir): rename repository scan roots to clone roots across all layers

## Overview

"Repository scan root" names the registered parent directories whose direct
children every `GET /api/repositories` call probes for git clones. The concept
is about to grow a second role: the destination directory Delta clones a
repository *into* when the user starts a session from a PR whose repository has
no local clone (follow-up task). "Scan root" describes only the probing half
and becomes inaccurate; the accurate name is **clone root** — a directory where
the user's git clones live. Delta probes a clone root's direct children for
existing clones, and (from the follow-up task onward) creates new clones inside
it.

Rename the concept across **every layer** in one PR, with zero behavior change
(probing stays direct-children-only; validation stays trim + absolute + must
exist as it is today):

- **Routes**: `GET/POST/DELETE /api/repository-scan-roots` →
  `/api/clone-roots` — in the `declare_endpoints!` table
  (`backend/crates/gateway/delta-wire/src/endpoint/table.rs`), the handlers,
  and the `RouteBinder` mounts. The endpoint/docs coverage gates will force the
  docs to follow.
- **Wire + domain types**: `RepositoryScanRoot` and friends (wire structs,
  `ports/session_store.rs` methods `list/insert/delete_repository_scan_root*`,
  `interactor/repository/scan_roots.rs` use cases, generated TS types) →
  clone-root vocabulary. Regenerate wire-gen output in the same commit (the
  hash check in `check_command` enforces sync).
- **SQLite**: table `repository_scan_roots` → `clone_roots`; bump
  `SCHEMA_VERSION` (dev DBs need `make reset` — say so in the PR body, as the
  previous bump did).
- **Frontend**: Settings category id `scan-roots` / label "Repository scan
  roots" (`features/settings/SettingsView.tsx` ~line 81) → `clone-roots` /
  "Clone roots"; query/mutation hook names; test ids. The active Settings
  category is persisted to localStorage — a stored stale `scan-roots` id must
  fall back to a valid category, not a blank pane (add a test if that fallback
  is not already pinned).
- **Docs**: `docs/guides/api/workdirs.md` sections (`GET
  /api/repository-scan-roots` etc.), plus every prose mention. Update the
  concept's definition sentence to the both-roles wording: a clone root is a
  directory where the user's git clones live; `GET /api/repositories` probes
  its direct children. Do not promise the clone-execution feature in docs —
  that lands with its own task.

The `check_command` appends a repo-wide spelling gate:
`! git grep -inE "scan[ _-]?roots?" -- ":!docs/tasks"` — no scan-root spelling
survives anywhere outside `docs/tasks/` (which keeps historical task files
verbatim).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The three routes are declared and mounted as `/api/clone-roots` and the
      route/docs coverage gates pass with the renamed docs sections.
- [x] The SQLite table is `clone_root` (singular, matching the repo's
      table-naming convention) and `SCHEMA_VERSION` is bumped by one.
- [x] No scan-root spelling remains outside `docs/tasks/`:
      `! git grep -inE "scan[ _-]?roots?" -- ":!docs/tasks"` (appended to
      `check_command`).
- [x] Settings renders the category as "Clone roots", and a stale persisted
      `scan-roots` category id falls back to a valid category (unit test).
- [x] Wire-gen output is regenerated and committed in sync (hash check in
      `check_command`).
- [x] Registration validation behavior is unchanged and still pinned by the
      existing tests (now under the new names).

### Manual / on-hardware (verified by a human before merge)

- [ ] After `make reset`, the real app registers and removes a clone root in
      Settings, and the Repository tab surfaces a direct-child clone of that
      root that has never hosted a session.

## Out of scope

- Any behavior change: probing depth, validation rules, ordering.
- The clone-execution feature itself (follow-up task); docs here describe only
  what exists after this PR.
