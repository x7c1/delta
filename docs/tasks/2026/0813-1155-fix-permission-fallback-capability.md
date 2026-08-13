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
branch: task/0813-1155-fix-permission-fallback-capability
created_at: 2026-08-13T11:55:34Z
updated_at: 2026-08-13T13:19:00Z
---

# fix(permission): capability-aware guidance when a decision can no longer take effect

## Overview

When `POST /api/permissions/{id}/decision` answers `409 permission_not_pending`,
the permission notice card swaps its Allow/Deny buttons for terminal guidance —
"Answer the prompt in the terminal." plus an "Open terminal" button
(`frontend/packages/apps/web/src/features/transcript/PermissionNotice.tsx`
~line 120, `fallback` state set in the `decide` catch handler). That guidance
is pane-backed-shaped: for Claude the 409 means the hook's browser-decision
wait timed out and the interactive TUI prompt now owns the question, so
pointing at the terminal is exactly right.

For an adapter-backed provider (Codex) the same 409 is reachable — observed in
dogfooding (2026-08-13): the session's `codex app-server` process died while an
approval dialog was pending; the first Allow marked the row decided but the
wire write failed (`Broken pipe`, HTTP 500, dialog stays because no
`permission_resolved` ever arrives), and the retry click got
`409 permission_not_pending` once the runtime no longer had an open agent. The
card then told the user to "Answer the prompt in the terminal" — but a Codex
session has no terminal (`has_terminal: false`), so there is nowhere to answer
and the user reads it as "stuck". A milder route to the same wrong guidance is
a double answer from two tabs: the loser's 409 shows terminal guidance for a
request that was in fact just resolved.

Fix: branch the fallback on the session provider's `has_terminal` capability —
the same capability the workspace already uses to gate the terminal pane
(`frontend/packages/apps/web/src/features/workspace/WorkspaceScreen.tsx`
~line 401, `focusedCapabilities.has_terminal`, sourced from
`GET /api/providers`; `TerminalPane` receives it as a `hasTerminal` prop). Per
repo convention the branch must be capability-driven — no
`provider === 'codex'` (or equivalent provider-literal) checks.

- `has_terminal: true` → exactly today's fallback: "Answer the prompt in the
  terminal." + "Open terminal" + Dismiss. Byte-identical for Claude.
- `has_terminal: false` → guidance that tells the truth for a terminal-less
  provider: the request can no longer be answered from here (it was resolved
  elsewhere or the agent connection was lost), with Dismiss as the only
  affordance — no "Open terminal" button (there is no terminal to open).
  Exact wording is yours; it must not mention a terminal.

Operation × state coverage (fallback rendering vs provider capability — write
a test per row):

- 409 on a session whose provider has `has_terminal: true` → terminal
  guidance with the "Open terminal" button (today's behavior, pinned).
- 409 on a session whose provider has `has_terminal: false` → the
  terminal-less guidance, Dismiss only, no "Open terminal" button.
- Capability unknown (providers list not yet loaded / provider missing) →
  pick a safe default and test it; do not crash. Name the chosen default in
  a comment.
- Non-409 decision failure → unchanged: buttons stay usable for a retry
  (existing behavior, must not regress).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Component/unit tests cover the operation × state rows above: the
      `has_terminal: true` fallback renders today's terminal guidance, the
      `has_terminal: false` fallback renders terminal-free guidance with no
      "Open terminal" button, and the unknown-capability default is pinned.
- [x] The existing pane-backed fallback test
      (`frontend/packages/apps/web/src/features/transcript/TranscriptPane.test.tsx`,
      "Answer the prompt in the terminal.") still passes unchanged for a
      Claude-shaped session.
- [x] No `provider === 'codex'` (or equivalent provider-literal) branch is
      introduced — the switch reads the `has_terminal` capability.
- [x] `grep -rn "Answer the prompt in the terminal" frontend/packages/apps/web/src`
      hits only the capability-gated branch (the string must not render for a
      terminal-less provider in any test fixture).

### Manual / on-hardware (verified by a human before merge)

- [x] Against a real `codex app-server` session: kill the session's
      app-server process (by PID) while an approval dialog is pending, click
      Allow (fails on the dead wire), click again — the card shows the
      terminal-less guidance instead of "Answer the prompt in the terminal",
      and Dismiss clears it.

## Out of scope

- Settling the turn / clearing the dead dialog when the adapter connection
  dies (that removes the most common route to this 409 and is a separate
  task; this task fixes the guidance for every remaining route).
- Wording changes to the pane-backed (Claude) fallback.
- Any backend change.
