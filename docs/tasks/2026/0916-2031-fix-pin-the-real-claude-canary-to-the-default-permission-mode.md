---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -rq -- '--permission-mode' backend/crates/apps/delta-server/tests/"
assignee: null
branch: task/0916-2031-fix-pin-the-real-claude-canary-to-the-default-permission-mode
created_at: 2026-09-16T20:31:00Z
updated_at: 2026-09-16T21:23:46Z
---

# fix(canary): pin the real-claude canary to the default permission mode

## Overview

`backend/crates/apps/delta-server/tests/real_claude_canary.rs` launches the
real `claude` binary in a tmux pane (`ClaudeSession::spawn`, around line
203) with `--settings <rendered> --session-id <uuid> <prompt>` and nothing
else. The canary `permission_dialog_fires_the_hook_and_the_allow_decision_is_honored`
(around line 703) then asks Claude to run `rm -f` on a file and relies on
the fact that, in the **default** permission mode, `rm` is never
auto-approved, so an interactive permission dialog is raised and the
`PermissionRequest` hook fires. Its own comment says so: "`rm` is never
auto-approved in default permission mode".

That assumption is not pinned. The launch inherits whatever the host's
`~/.claude/settings.json` sets as `defaultMode`. On a host configured with
`defaultMode: auto` (observed on 2026-09-09) the tool call is auto-approved,
no dialog appears, no `PermissionRequest` POST arrives, and the canary goes
red for a reason that has nothing to do with Delta or with upstream drift —
exactly the false signal the gate exists to avoid.

Pin the mode the test assumes. In `ClaudeSession::spawn`, add
`--permission-mode` and `default` to the `command` vector, next to the
existing `--settings` and `--session-id` pair, so every canary launch runs
in the mode the suite was written against regardless of host settings.

`default` is the value Delta's own launch-option vocabulary uses for this
mode (`backend/crates/gateway/claude-agent/src/launch_option_danger.rs`
lists it among the benign values). The installed CLI (2.1.273) accepts it —
`claude --permission-mode default --version` succeeds while an invalid value
is rejected at parse time — although its `--help` lists `manual` rather
than `default`. Use `default`; if a future CLI rejects it, that is an
upstream contract change for the canary to report, not something to
pre-empt here.

Update the comment on the `rm` canary to say the mode is pinned by the
launch rather than assumed, and add a line to the module doc's list of
what the suite guarantees about its launch environment, if one exists, so
the next reader knows the mode is deliberate.

The roadmap entry also says to clear the red `last-attempt` marker under
`~/.local/state/delta/e2e-real/claude/` after the fix. That is host state,
not repository state: the authoring host carries no such marker, so there is
nothing for this task to do about it beyond the Manual item below.

### Session-state coverage

Not applicable: the change is to how the canary launches its own subject,
not to any operation Delta exposes.

### Pipeline notes

- One test file. `make check` compiles the canary (it is `#[ignore]`d, so
  it never runs in the hermetic gate); the appended grep is the gate that
  the flag is actually passed. It was negative-tested at authoring time:
  `grep -rq -- '--permission-mode' backend/crates/apps/delta-server/tests/`
  finds nothing on `main` today.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The canary's launch command passes `--permission-mode default`
      (`grep -rq -- '--permission-mode'
      backend/crates/apps/delta-server/tests/`, appended to
      `check_command`), and the canary crate still compiles under
      `cargo test` (which builds `#[ignore]`d tests without running them).
- [x] The comment on the `rm` permission canary states that the mode is
      pinned by the launch, not assumed from the host (diff inspection; the
      old wording "in default permission mode" alone no longer appears as
      an assumption).

### Manual / on-hardware (verified by a human before merge)

- [ ] `make e2e-real-claude` passes on a host whose
      `~/.claude/settings.json` sets `defaultMode: auto` (or, failing such
      a host, passes once on any host after the merge — it spends a handful
      of real turns). The permission canary in particular must see the
      `PermissionRequest` POST.

## Out of scope

- Pinning any other launch flag (`--model`, plugins) for the canary.
- The Codex canaries: `codex app-server` has no equivalent mode flag on the
  path the canary uses.
- Changing what the gate script does with a red marker.
