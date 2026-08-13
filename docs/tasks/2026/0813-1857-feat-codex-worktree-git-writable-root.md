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
branch: task/0813-1857-feat-codex-worktree-git-writable-root
created_at: 2026-08-13T18:57:06Z
updated_at: 2026-08-13T21:20:00Z
---

# feat(agent): grant the worktree's real git dir to Codex's sandbox at thread start

## Overview

Codex's `workspace-write` sandbox makes the session's cwd writable but keeps
the `.git` at a writable root's top level read-only, so routine `git add` /
`git commit` escalate for approval. Users can lift that per-repo with
`sandbox_workspace_write.writable_roots` in `~/.codex/config.toml` (a relative
`".git"` entry resolves against the cwd and works for a normal clone). In a
**linked worktree, that fix does not reach**: `<worktree>/.git` is a pointer
file (`gitdir: <main-repo>/.git/worktrees/<id>`), and git's actual writes land
in the main repository's `.git` — the per-worktree `worktrees/<id>/index` plus
the shared `objects/` and `refs/` — which is outside the worktree cwd and
outside any cwd-relative writable root.

Verified empirically on codex-cli 0.144.4 (via `codex sandbox` with
`sandbox_mode=workspace-write`, control test confirming enforcement):

- In a linked worktree, `git add` fails inside the sandbox with
  `Unable to create '<main>/.git/worktrees/<id>/index.lock': Operation not
  permitted` — in a live session this becomes an approval prompt on every
  git write.
- Adding the main repository's `.git` (absolute path) to
  `sandbox_workspace_write.writable_roots` makes `git add` / `git commit`
  succeed inside the sandbox.
- Codex 0.144.4 has no built-in worktree special-casing.

Delta is the party that **creates** the per-session worktree and knows the
source repository root, so it should close this gap itself: when launching or
resuming a **Codex session whose workdir is a Delta-created worktree**, inject
`"config": {"sandbox_workspace_write.writable_roots": ["<main-repo>/.git"]}`
into the `thread/start` / `thread/resume` params. The injection is a request
parameter derived fresh on every launch/resume — nothing is registered or
persisted, so there is no cleanup lifecycle.

Implementation seams:

- `backend/crates/gateway/codex-agent/src/adapter.rs` — `thread_start_params`
  (~line 417) builds the params from `workdir` + user launch options, with
  `DELTA_OWNED_THREAD_FIELDS` rejection and duplicate-key rejection.
- `backend/crates/domain/delta-usecase/src/agent/adapter.rs` — `LaunchRequest`
  (`session_id`, `workdir`, `launch_options`, `first_prompt`) and
  `ResumeRequest` carry no worktree information today; extend them (e.g. an
  optional worktree source-repo root) rather than having the adapter guess
  from the filesystem.
- `backend/crates/domain/delta-usecase/src/interactor/lifecycle/spawn_fresh.rs`
  already resolves `worktree_repo_root` (~line 121) before the worktree is
  created; the resume path must re-derive the same value from the session's
  stored repo-root column (recorded since the session-metadata work), never
  from a new persistent registration.
- The Claude adapter ignores the new field — its launch must stay
  byte-identical.

**Conflict policy with user-registered launch options (decided):** if the
user's registered `config` launch option contains any key under
`sandbox_workspace_write` — a dotted key with that prefix, or a nested
`sandbox_workspace_write` object — Delta does **not** inject: the user's
config passes through verbatim and the deferral is surfaced (server log at
minimum). Otherwise Delta merges its single entry into the user's `config`
object (creating `config` when absent), leaving every other user key
untouched. The existing loud rejection for user-vs-user duplicate keys is
unchanged. Rationale: no deep-merge machinery on top of unverified upstream
semantics, never silently rewrite an explicit user sandbox setting, and the
degraded outcome (approval prompts return in worktrees) is the visible
pre-feature status quo.

**Resolve during the work phase (with an empirical check, not assumption):**

1. Whether `thread/start`'s `config` accepts dotted keys with the same
   leaf-level override semantics as the CLI's `-c` flag. This is unverified
   for the app-server path; if dotted keys are not accepted, find the shape
   that works (e.g. nested) and pin it in a test against the vendored schema
   plus the real-codex canary.
2. Interaction with the user's global `~/.codex/config.toml`: a leaf-level
   override presumably **replaces** the global `writable_roots` list for the
   session (acceptable for v1 — a cwd-relative `".git"` is useless in a
   worktree anyway — but document the choice; a `config/read`-then-union
   approach is the candidate if replacement proves harmful).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Launching a Codex session in a Delta-created worktree produces
      `thread/start` params whose `config` grants the main repository's
      `.git` via `sandbox_workspace_write.writable_roots` (unit test on the
      params builder).
- [x] Launching a Codex session in a plain (non-worktree) directory produces
      params byte-identical to today — no `config` injection (unit test).
- [x] Resuming a Codex worktree session re-derives and injects the same grant
      (unit test on the resume path).
- [x] A user-registered `config` launch option containing a
      `sandbox_workspace_write` key — dotted and nested spellings each — 
      suppresses the injection and passes the user's value through verbatim
      (unit tests for both spellings).
- [x] A user-registered `config` launch option without any
      `sandbox_workspace_write` key is merged: all user keys preserved and the
      injected grant present (unit test).
- [x] Two user-registered launch options both named `config` are still
      rejected loudly (existing behavior pinned by a test).
- [x] Claude sessions' launch argv is unaffected by the new request field
      (existing golden/launch tests still pass unchanged).

### Manual / on-hardware (verified by a human before merge)

- [ ] In a real Codex worktree session started from Delta, `git add` and
      `git commit` complete without raising an approval prompt (also confirms
      the real app-server accepts the injected `config` shape).
- [ ] In a real Codex non-worktree session, behavior is unchanged.

## Out of scope

- Pruning stale `<main>/.git/worktrees/<id>` entries after Delta deletes a
  worktree (`git worktree prune` concern) — pre-existing lifecycle topic,
  independent of this injection.
- A granular grant that excludes `hooks/` and `config` inside the main
  `.git` (e.g. only `objects/`, `refs/`, `logs/`, `worktrees/<id>`) — v1
  deliberately grants the whole `.git`; the granular variant is recorded here
  as a possible follow-up.
- Injecting anything for non-worktree sessions (a cwd-relative `".git"`
  writable root remains the user's own global-config choice).
- Claude-side sandbox behavior and any frontend/UI change.
