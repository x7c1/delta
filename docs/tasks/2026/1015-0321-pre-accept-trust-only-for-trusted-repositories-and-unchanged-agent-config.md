---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1015-0321-pre-accept-trust-only-for-trusted-repositories-and-unchanged-agent-config
created_at: 2026-10-14T18:21:02Z
updated_at: 2026-10-14T19:25:46Z
---

# feat(worktree): pre-accept Claude Code's trust only when the user already trusts the repository and the branch leaves its agent configuration unchanged

## Overview

When Delta launches `claude` in a worktree it created under its worktree base,
it pre-accepts Claude Code's workspace-trust and external-`CLAUDE.md`-import
prompts by writing `hasTrustDialogAccepted` and the two import keys into
`~/.claude.json` (`backend/crates/gateway/git-worktree/src/trust.rs`, called
from `prepare_pane_launch` in
`backend/crates/domain/delta-usecase/src/interactor/lifecycle/launch_prep.rs`;
resume goes through `open_session.rs`). This change narrows when it does
this. Trust is
what lets a project's checked-in agent configuration (hooks, status line,
`apiKeyHelper`, MCP servers, `CLAUDE.md` imports) take effect without a prompt,
and the content of a worktree made from a remote branch or a PR is decided by
that branch's author, not by the user.

Narrow the pre-acceptance to the cases where the user has, in effect, already
made the trust decision, and ask in Delta's own UI otherwise.

### Rule A — inherit the repository's trust

Pre-accept for a worktree only when the worktree's repository root (the
repository the user picked) is itself trusted in `~/.claude.json`
(`projects.<repo root>.hasTrustDialogAccepted == true`). A repository that has
never been trusted gets the confirmation below instead. Once the user confirms
it, Delta records trust for the repository root as well (the same keys Claude
Code itself would write when the user accepts its dialog there), so the
question is asked once per repository.

### Rule B — branches from outside: confirm only when agent configuration differs

For a worktree whose start point is a remote branch
(`WorktreeStartPoint::RemoteBranch` / `UseRemoteBranch`, which includes every
PR-origin start), compare the branch's tree with the repository root's
`HEAD` tree (the content the user trusted) on these paths:

- everything under `.claude/` at the repository top (compare the whole
  directory, not a list of known files, so a configuration file Claude Code
  adds later is still covered),
- `.mcp.json` at the top,
- every `CLAUDE.md` and `CLAUDE.local.md` anywhere in the tree.

If nothing differs (the common case — most branches do not touch these),
pre-accept as under rule A without asking. If anything differs, do not
pre-accept; show the confirmation below with the diff.

`WorktreeStartPoint::Head` (a new branch off the user's own `HEAD`) needs only
rule A. Directories outside the worktree base keep today's behaviour (never
pre-accepted; Claude Code's own dialog appears).

### The confirmation

Before the worktree session launches, the new-session flow asks the server
whether launching needs confirmation. When it does, the UI shows a dialog that
states why (repository not yet trusted, and/or the branch changes agent
configuration), lists the changed paths, and shows the diff for each (a
unified diff, scrollable, monospace; reuse the app's existing diff rendering
if one fits). Buttons: launch and trust, or cancel. Cancel creates nothing.

The confirmation is bound to what was shown: the launch request carries the
commit the user confirmed, and the server refuses to pre-accept (and asks
again) if the worktree it checks out is at a different commit. Compute the
diff from git objects (`git diff --name-only` / `git diff` between the two
trees, after the fetch the start point already does), never by reading files
from the not-yet-created worktree.

Resume (`open_session.rs`) does not ask again for a worktree that is already
trusted; trust is per directory.

Codex sessions are out of scope here unless the same seeding code path is
shared; if it is, apply the same rule and say so in the PR.

### Docs

Describe the rule (A, B, the paths compared, the binding to the confirmed
commit) in `docs/guides/security.md`, and update the module docs in
`trust.rs`, `launch_prep.rs`, and `lifecycle/mod.rs`
(`is_under_worktree_base`) that describe the old unconditional rule.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Usecase tests cover each launch state: repository trusted + `Head` →
      pre-accepted without asking; repository not trusted + `Head` → asks;
      repository trusted + remote branch with no difference in the compared
      paths → pre-accepted without asking; repository trusted + remote branch
      that changes `.claude/settings.json` → asks; a change to a new file
      under `.claude/`, to `.mcp.json`, and to a nested `CLAUDE.md` → each
      asks; directory outside the worktree base → never pre-accepted.
- [x] A test asserts that confirming an untrusted repository records trust
      for the repository root, so the next `Head` launch does not ask.
- [x] A test asserts the server refuses to pre-accept when the confirmed
      commit differs from the commit the worktree is checked out at.
- [x] A test asserts cancelling creates no worktree and no session.
- [x] A git-gateway test builds a scratch repository with a branch that
      changes `.claude/` and asserts the reported paths and diff.
- [x] Frontend tests cover the dialog: it lists the changed paths, shows the
      diff, launches with the confirmed commit on accept, and launches nothing
      on cancel; and the flow with no confirmation needed launches directly.
- [x] `make check` passes.

### Before merge (verified outside the check command)

- [ ] In the desktop app, starting a session from a PR that does not touch
      the compared paths launches with no dialog, and from a branch that
      changes `.claude/settings.json` shows the dialog with that diff;
      accepting launches the session.
