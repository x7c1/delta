---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -rq "does_not_auto_trust_a_user_selected_workdir" backend/crates && grep -rq "auto_trusts_a_delta_worktree" backend/crates && test -f docs/guides/security.md'
assignee: null
branch: task/0901-1616-fix-scope-auto-trust-to-delta-worktrees
created_at: 2026-09-01T16:16:00Z
updated_at: 2026-09-01T16:48:00Z
---

# fix(trust): only pre-accept Claude Code trust for Delta's own worktrees

## Overview

`ensure_dir_trusted` (`backend/crates/gateway/git-worktree/src/trust.rs`) writes
`hasTrustDialogAccepted` / `hasClaudeMdExternalIncludesApproved` into the user's
**global** `~/.claude.json` for a directory, permanently pre-accepting Claude
Code's workspace-trust dialog — which means any checked-in `.claude/settings.json`
hooks in that directory run without the user ever being asked, and the effect
leaks into the user's plain `claude` runs in that same directory outside Delta.
Today Delta seeds this trust too broadly:

- **`spawn_fresh.rs` (~L207-230)** computes `seed_trust`: a worktree request →
  `true` (fine — a worktree Delta manages), but **a user-selected
  `requested_workdir` that is any git repo → also `true`** (the `None =>
  Some(dir)` arm, `trust = repo_root(dir).is_some()`). So pointing Delta at an
  arbitrary existing repo (or a freshly `gh clone`d one) globally auto-trusts
  it.
- **`open_session.rs` (~L125-126)** on resume: `if repo_root(workdir).is_some()
  { ensure_dir_trusted(workdir) }` — same over-broad seeding for the resumed
  session's cwd.

**Decision (strict):** auto-trust **only directories under Delta's own worktree
base** — i.e. worktrees Delta itself created. For any other directory (a
user-selected existing repo, a cloned repo, a worktree the user made elsewhere),
do **not** pre-seed trust; let Claude Code show its normal one-time trust dialog
(explicit opt-in). This is a per-directory dialog Claude Code remembers after
the user accepts once, so the ongoing cost is a single prompt per new external
repo, while Delta-created worktrees (new paths each spawn) stay seamless.

### The change

- **Gate `ensure_dir_trusted` on "under the worktree base"** at both seeding
  sites. The `worktree_base` is available on the interactor
  (`interactor/mod.rs:123` `worktree_base: String`).
  - `launch_prep.rs:184` — currently `if pane.seed_trust { … ensure_dir_trusted(&launching.workdir) }`.
    Add the base check: seed trust only when `launching.workdir` is under
    `self.worktree_base` (keep the existing `seed_trust` git-working-tree gate
    too, so the empty `<base>/<token>` scratch dir is still not seeded — it is
    under the base but not a git tree). Result: `pane.seed_trust && is_under(base, launching.workdir)`.
  - `open_session.rs:125` — add the same base check: seed on resume only when
    `workdir` is under `self.worktree_base` (and is a git repo, as today).
- **Path check must be robust:** canonicalize both the base and the candidate
  dir before comparing (defeat `/tmp` vs `/private/tmp` on macOS and `..`), and
  compare by **path components** (`Path::starts_with`), not string prefix, so
  `<base>-evil` does not count as under `<base>`. Put the helper somewhere
  sensible (a small `pub(in crate::interactor)` fn, or reuse an existing path
  util). Comment WHY (the trust trade-off).
- You may keep the `seed_trust` bool as the "is a git working tree /
  trust-eligible" signal and AND it with the base check, or refactor it away and
  gate purely on the base+git-tree condition at the two sites — your call; keep
  it minimal and don't change the navigator repo-name derivation
  (`launch_repo_root`), which is independent.

### User-facing threat-model / trust guide (new)

There is no security/threat-model doc. Create **`docs/guides/security.md`** (an
Overview section first, per repo convention). Cover, concisely:
- **The trust boundary for the localhost server** — delta-server binds loopback
  only but is reachable by any process/page on the machine; it is now defended
  by the Origin/Host guard, a per-run bearer token, per-session-less hook
  secret, and (this change) scoped trust seeding. State plainly that the server
  is "unauthenticated-by-port" in the sense that reaching the loopback port is
  the trust boundary, and what each guard covers.
- **The Claude Code trust-seeding trade-off** (the subject of this change):
  Delta auto-accepts Claude Code's workspace-trust dialog **only for worktrees
  it creates under its own worktree base**; for any directory you point Delta
  at yourself (an existing repo, a clone), Claude Code shows its normal trust
  dialog once — Delta does not pre-accept it on your behalf, because that would
  also silently trust that directory's checked-in automation in your plain
  `claude` sessions.
Keep it short and factual; it can be extended by later hardening work.

### Test blast radius

- Lifecycle tests that assert trust IS seeded for a user-selected workdir now
  need updating (the fake `git_worktree` port records `ensure_dir_trusted`
  calls — find its recorder). In the test `Config`, `worktree_base` is
  `/tmp/delta-worktrees` (`delta-bootstrap/src/lib.rs:321`) — so a test dir must
  be **under that base** to still be trusted, and a user-selected dir **outside**
  it must now assert **no** trust seeding. Check
  `interactor/lifecycle/tests/*` (e.g. the use-branch / worktree-reuse and
  launch-preparation tests) and any test using the trust recorder.
- Add the two required tests (name them so the greps match): one proving an
  arbitrary external user-selected workdir (a git repo NOT under the worktree
  base) is **not** written into `~/.claude.json` / not passed to
  `ensure_dir_trusted` (`does_not_auto_trust_a_user_selected_workdir`), and one
  proving a Delta worktree under the base **is** trusted
  (`auto_trusts_a_delta_worktree`).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A fresh spawn (and a resume) whose effective launch dir is a git repo the
      user selected **outside** the worktree base does NOT call
      `ensure_dir_trusted` (no write to `~/.claude.json`) — test name contains
      `does_not_auto_trust_a_user_selected_workdir`; grepped by `check_command`.
- [x] A spawn whose worktree Delta creates under the worktree base DOES seed
      trust (test name contains `auto_trusts_a_delta_worktree`), and the empty
      `<base>/<token>` scratch dir still does not (it is not a git tree).
- [x] `docs/guides/security.md` exists (checked by `check_command`) and
      documents the loopback trust boundary and the scoped trust-seeding
      trade-off.
- [x] `make check` passes green — the base-scoped trust logic and the updated
      lifecycle tests pass; no regression to the navigator repo-name line or the
      worktree launch flow.

### Manual / on-hardware (verified by a human before merge)

- [ ] Live: starting a Delta session on a worktree still runs without a trust
      prompt; pointing Delta at an existing external repo now shows Claude
      Code's trust dialog once (and is remembered after accepting). `~/.claude.json`
      gains no entry for the external repo until the user accepts. (Non-blocking
      for merge under the agreed CI-green autonomous policy; recorded for
      dogfooding.)

## Out of scope

- Removing `ensure_dir_trusted` / trust seeding entirely (Delta worktrees still
  need it so their panes don't stall on the interactive dialog).
- Changing what keys are written (`hasTrustDialogAccepted` etc.) or the
  `~/.claude.json` format.
- A UI opt-in control for trusting an external repo from within Delta (a future
  enhancement); for now the opt-in is Claude Code's own dialog.
