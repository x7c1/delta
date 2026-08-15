---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: 'cd backend && cargo fmt --all -- --check && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && BEFORE=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && make gen && AFTER=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && [ "$BEFORE" = "$AFTER" ] && cd frontend && pnpm -r build && pnpm -r typecheck && pnpm -r test && pnpm -r lint && cd .. && make e2e'
assignee: null
branch: task/0814-1612-feat-clone-pr-repo-from-pr-tab
created_at: 2026-08-14T16:12:00Z
updated_at: 2026-08-15T08:30:00Z
---

# feat(new-session): clone a PR's missing repository into a clone root from the PR tab

## Overview

A PR row whose repository has no registered local clone is a dead end today:
the row is de-emphasised, its click is a silent no-op, and an inline hint tells
the user to run `gh repo clone` somewhere themselves
(`frontend/packages/apps/web/src/features/new-session/tabs/PRTab.tsx`, `PrRow`
~line 278). From the user's seat the obvious reaction is "then clone it for
me". Delta already has everything it needs: `gh` is authenticated (the PR tab
is gated on `gh_available`) and clone roots (renamed from scan roots by the
prior task) say where clones live.

**Prerequisites**: this task builds on the clone-root rename and on the PR-pick
worktree provenance (both already on the default branch when this task runs).

### Backend

- New endpoint `POST /api/repositories/clone`, body
  `{ repo_owner, repo_name, clone_root }`:
  - `clone_root` must be a registered clone root; otherwise a 4xx with a
    machine-readable `code`, no job.
  - Destination is exactly `<clone_root>/<repo_name>` — no fallback naming. If
    that path already exists, **409** (e.g. `code: "clone_dest_exists"`), no
    job.
  - Happy path: **202**; an async job runs `gh repo clone <owner>/<name>
    <tmp>` where `<tmp>` is a deterministic temporary sibling inside the clone
    root (e.g. `<clone_root>/.delta-clone-tmp-<repo_name>`), then atomically
    renames `<tmp>` to the destination on success. On failure the temp dir is
    removed. The destination therefore never exists half-cloned. A stale temp
    dir left by a dead server process is removed before a new job starts.
  - Jobs live in an **in-memory registry keyed by destination path** — no
    persistence, gone on server restart. A second request for the same
    destination while a job runs **joins it** (202, no second `gh` process,
    one completion event serves both). Requests for different repositories run
    concurrently.
  - The job is fully independent of session lifecycles: starting sessions
    (pane- or adapter-backed) while a clone runs must not block or be blocked.
- Completion and failure are announced on the existing `/ws` stream as new
  event kind(s) carrying `repo_owner`, `repo_name`, `clone_root`, the
  destination path, and an error message on failure (exact shape and whether
  one kind with a status field or two kinds is the implementer's call — the
  event-union coverage gate forces documentation either way). Drive the `gh`
  invocation through a port so use-case tests run against a fake.

### Frontend

- A no-clone PR row becomes clickable; the click expands an inline clone panel
  under the row (the dialog stays put — no navigation):
  - 0 registered clone roots → the panel contains a path input that registers
    the root via the clone-roots API, then proceeds. The Settings section is
    not the answer here; registration is a step of this flow.
  - 1 root → shown as the destination (no choice needed).
  - N roots → a selector, defaulting to the most recently registered.
  - A Clone action fires the POST; while the job runs the row shows a spinner.
    Everything else in the dialog — other rows, other tabs, the composer —
    stays interactive.
- On the completion event: refetch the PR list / repositories (the row's
  `has_local_clone` flips), and **auto-continue** into the normal PR pick
  (workdir + locked worktree provenance) **only if that PR's clone intent is
  still the active one in this dialog** — if the user has since picked
  something else, started another session, or closed the dialog, just let the
  refetch enable the row; never hijack current state.
- On the failure event: inline error on the row (the event's message); retry is
  simply clicking again.
- Page reload while a clone runs: the local intent is lost by design; the row
  still shows no-clone until the refetch, and a re-click joins the running job
  via the dedupe. Document this in the route's docs section.

### State coverage (operation = requesting a clone)

Destination absent (happy path) / destination exists (409) / same-destination
job already running (join) / different-repo job running (concurrent) / `gh`
clone fails (failed event, temp removed) / zero registered roots (inline
registration) / dialog closed mid-clone (no auto-continue) / intent superseded
mid-clone (no auto-continue) / server restarted mid-clone (job forgotten,
stale temp cleaned on next request). Each testable one appears below.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `POST /api/repositories/clone` with an unregistered `clone_root` returns
      a 4xx with a machine-readable code and starts no job (test).
- [x] An existing `<clone_root>/<repo_name>` returns 409 and starts no job
      (test).
- [x] The happy path clones into the temp path, renames atomically to the
      destination, and emits the completion event (use-case test with a fake
      gh port).
- [x] A failing `gh` clone removes the temp dir and emits the failure event
      with the error message (test).
- [x] A duplicate request while the same destination's job runs joins it: one
      `gh` invocation, one completion (test).
- [x] A stale temp dir from a dead job is removed when a new job for the same
      destination starts (test).
- [x] Session spawn paths take no lock/dependency on the clone registry — a
      session can be created while a clone job is in flight (test at the
      use-case layer).
- [x] The endpoint is declared in `ENDPOINTS` and documented; the new `/ws`
      event kind(s) are documented (both coverage gates pass).
- [x] Frontend: a no-clone row click opens the inline panel; zero roots shows
      the registration input; a completion event auto-continues into the PR
      pick only while the intent is active, and does nothing to composer state
      when the intent was superseded (component tests for both).

### Manual / on-hardware (verified by a human before merge)

- [ ] On a temporary server (separate port and DB), with real `gh`: click a
      no-clone PR row, clone a small public repository visible to the
      authenticated account, watch the row flip and the auto-continue land in
      the locked-worktree compose state on the PR's head branch.
- [ ] While that clone runs, start an unrelated session — it opens normally.
- [ ] A pre-existing destination directory shows the inline 409 error.

## Out of scope

- Cross-fork PR head-branch resolution (existing limitation, unchanged).
- Streaming `gh` output as progress (spinner only in v1).
- Persisting clone jobs across server restarts.
- Any change to probing depth or clone-root semantics beyond consuming them.
