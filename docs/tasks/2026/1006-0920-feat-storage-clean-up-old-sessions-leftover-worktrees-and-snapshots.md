---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && git grep -q 'PruneSessions' -- backend/crates/gateway/delta-wire/src/endpoint/table.rs && git grep -q 'ListOrphanWorktrees' -- backend/crates/gateway/delta-wire/src/endpoint/table.rs && git grep -q 'DeleteSnapshot' -- backend/crates/gateway/delta-wire/src/endpoint/table.rs && git grep -q '/api/storage/snapshots' -- docs/guides/api/settings.md && ! git grep -q 'never deleted automatically' -- backend/crates/gateway/delta-sqlite/src/migrations/runner.rs"
assignee: null
branch: task/1006-0920-feat-storage-clean-up-old-sessions-leftover-worktrees-and-snapshots
created_at: 2026-10-06T00:17:48Z
updated_at: 2026-10-06T04:28:17Z
---

# feat(storage): clean up old sessions, leftover worktrees and snapshots from Settings → Storage

## Overview

Settings → Storage shows what Delta keeps on the machine and how big the
database is, and removing a session now takes its clean worktree and branch
with it. What a long-time user still cannot do from the app: delete many
old sessions at once, remove the worktrees that outlived their sessions
(kept because they held work, or left by versions that did not clean up),
and delete the pre-migration snapshots `delta.db.bak-v<N>` that the
migration runner writes and never removes. Each of these is a manual job on
the command line today.

This task adds those three operations to the Storage category. It is the
last of the three cleanup slices: the database already shrinks on delete
(`auto_vacuum`), and single-session removal already owns the session's
worktree, so this task composes those and adds the two listings it needs.

### Rule (unchanged from single-session removal)

Delta never destroys the user's work. Bulk removal applies the existing
`delete_session` to each session, so a worktree with uncommitted changes or
an unmerged branch is kept exactly as a single removal keeps it. Only the
explicit, per-item worktree removal below may force.

### Backend

- **Bulk session removal.** `POST /api/sessions/prune` with a body
  `{ "older_than_days": N, "statuses": ["ended", "failed"] }`
  (`statuses` optional, default both; `older_than_days` required, ≥ 0).
  "Old" means the session's most recent activity —
  `COALESCE(last_activity_at, created_at)` in the session table
  (`backend/crates/gateway/delta-sqlite/src/migrations/session.rs`) — is at
  least that many days ago. Open or spawning sessions are never candidates
  even if their row matches (the existing refusals in `delete_session`
  stay; the prune skips rather than fails on them). Add a
  `SessionStore::list_prunable_sessions(cutoff, statuses)` query (and its
  fake) returning the candidate ids; run `delete_session` for each through
  the interactor so every existing guard and the worktree rule apply;
  broadcast `session_removed` for each, as the single route does. Respond
  with the count removed, the ids skipped (and why), and the aggregated
  `SessionRemoval` kept items so the UI can say what stayed on disk.
- **Preview.** The same body on `GET /api/sessions/prune?older_than_days=N&statuses=…`
  returns the candidate count and ids without deleting anything, so the UI
  can show "N sessions will be removed" before the user confirms.
- **Orphan worktrees.** `GET /api/storage/worktrees` lists the directories
  directly under `worktree_base` (`Config::worktree_base`) with, for each:
  the path, whether a listed session still uses it (`cwd`, `requested_workdir`
  or a message `cwd` — the same check `delete_session` uses to keep a
  worktree another session works in), the repository root git reports for
  it (if still a worktree of a repository), and whether it has uncommitted
  or untracked changes (`git status --porcelain`; `null` when it is no
  longer a registered worktree). Directories with no owning session are
  the orphans; the UI shows all with the owned ones marked.
  `DELETE /api/storage/worktrees` with `{ "path": …, "force": bool }`
  removes one orphan: refuse (409) a path outside `worktree_base`, a path
  that a listed session still uses, or — without `force` — a dirty one;
  with `force` run `git worktree remove --force`. After removal run
  `git worktree prune` in the repository (when known) and forget the
  `~/.claude.json` trust entry. A directory git no longer knows (prune
  already forgot it, or the repository is gone) is removed as a plain
  directory tree when forced, and refused otherwise. Extend the
  `GitWorktree` port only as needed (a status query, a forced removal);
  reuse `remove_worktree`, `prune_worktrees`, `forget_dir_trusted`.
- **Snapshots.** `DELETE /api/storage/snapshots` with `{ "path": … }`
  deletes one `delta.db.bak-v<N>` listed by `GET /api/storage` (refuse any
  path that is not one of those). Update the "never deleted automatically"
  wording in `migrations/runner.rs` (`back_up`) to say they are kept until
  the user deletes them from Settings → Storage, and the install guide's
  snapshot sentence to match.
- Declare every route in `delta-wire/src/endpoint/table.rs`, one wire type
  per module under `rest/`, bind in `app/mod.rs`, handlers in their own
  files under `api/`, `make gen`, and document them in
  `docs/guides/api/settings.md` (Storage section) and
  `docs/guides/api/sessions.md` (prune, next to `DELETE /api/sessions/{id}`).

### Frontend

- In `features/settings/storage/`, add three blocks below the inventory,
  each in its own module and each fetching only while the category is
  `active`:
  1. **Remove old sessions** — a number input for days (default 30) and a
     status choice (ended / failed / both), a live preview line fed by the
     GET ("12 closed sessions older than 30 days"), a Remove button that
     asks for confirmation naming the count, then shows the result: how
     many were removed, how many skipped, and which worktrees or branches
     were kept and why (the kept items from the response). Invalidate the
     sessions and storage queries afterwards so the navigator and the
     database size refresh.
  2. **Worktrees** — the list from `GET /api/storage/worktrees`: path
     (same row style as the inventory, with copy), repository, state
     ("in use by a listed session" / "clean" / "has uncommitted changes" /
     "not registered with git"), and a Remove control on each orphan. A
     clean orphan is removed after a plain confirmation; a dirty or
     unregistered one requires the user to type the directory's final path
     component before the forced removal is enabled, and the dialog says
     the changes will be lost. Owned worktrees have no control.
  3. **Snapshots** — reuse the existing snapshots sub-list of the Database
     row: add a Delete control per snapshot with a confirmation that names
     the file and its size.
- api-client: the new methods, query keys and mutation hooks; api-mocks: handlers
  and fixtures (two orphan worktrees, one dirty; one owned worktree; two
  snapshots) so mock mode and the Playwright mock suite exercise every state.
- Wording follows the Storage section's existing voice. The confirmation
  dialogs follow the pattern the `Remove` action in the session card uses
  (`features/navigator/SessionNode.tsx`).

### Tests

- Store: `list_prunable_sessions` honours the cutoff and the status filter
  (sqlite tests next to `store/tests/sessions.rs`).
- Usecase: the prune applies `delete_session` per id, skips open/spawning,
  aggregates kept items; the orphan listing marks owned worktrees; the
  forced removal refuses paths outside the base and owned paths; snapshot
  deletion refuses a path that is not a listed snapshot.
- Gateway: `git status --porcelain` classification and forced removal
  against a temporary repository.
- Endpoint tests for each route (`app/tests/`), `gen-check`.
- Frontend: vitest for each block (preview line, confirmation gating,
  result rendering), and Playwright mock specs that remove an old
  session, force-remove a dirty orphan, and delete a snapshot.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `PruneSessions`, `ListOrphanWorktrees` and `DeleteSnapshot` are
      declared in the endpoint table, `/api/storage/snapshots` is documented,
      and `migrations/runner.rs` no longer says snapshots are "never deleted
      automatically" but points at Storage (`git grep` gates in
      `check_command`).
- [x] Store, usecase, gateway and endpoint tests cover the behaviours
      listed under Tests; `make check` passes including the new Playwright
      mock specs.

### Before merge (verified outside the check command)

- [x] On the development machine (macOS), against a server built from this
      branch with fake-claude sessions in Delta-created worktrees: the prune
      preview with a cutoff of 0 days counted the closed session; the prune
      removed it, reported its worktree (made dirty with an untracked file)
      and branch as kept, and skipped the still-open session with the reason
      `open`; `GET /api/storage/worktrees` listed that worktree as dirty and
      the open session's as in use; the forced removal of the dirty orphan
      answered 204 and the directory was gone; deleting a planted
      `delta.db.bak-v9` answered 204, the file was gone and the inventory no
      longer listed it. The typed-name gate in front of the forced removal
      is exercised by the Playwright mock spec rather than by this API run.
