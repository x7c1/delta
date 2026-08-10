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
branch: task/0808-1421-feat-comms-log-inspector-pane
created_at: 2026-08-08T14:21:20Z
updated_at: 2026-08-08T17:20:46Z
---

# feat(codex): comms-log inspector pane for terminal-less sessions

## Overview

Claude sessions have a terminal pane in the workspace right pane — a window
into "what is the agent doing right now". Codex sessions are adapter-backed
(headless JSON-RPC against `codex app-server`) and have no terminal, and the
right pane renders nothing at all for them: the pane and its toggle button are
gated on `terminalOpen && focusedHasTerminal` in
`frontend/packages/apps/web/src/features/workspace/WorkspaceScreen.tsx`
(~line 334–448), and no alternative exists — the only streaming routes are
`/ws` and `/pty` (`backend/crates/apps/delta-server/src/app.rs:125,127`).
This lack of a window has real cost: diagnosing recent Codex issues (e.g.
delta#304's `gitInfo: null`) required querying the sqlite DB directly and
hand-running ignored canary tests just to see what the wire actually carried.

Add a **communication-log inspector**: surface the delta ↔ `codex app-server`
JSON-RPC frames (both directions — client requests/notifications, server
responses, server-originated requests, server notifications) as a live,
time-ordered log in the workspace right pane for sessions whose provider has
no terminal.

Implementation direction (details are yours, invariants are not):

- **Tap at the wire boundary.** The byte-level contract lives in
  `backend/crates/gateway/codex-agent/src/wire.rs`; the adapter send/receive
  paths are the natural tap points. Emit each frame into a provider-neutral
  comms-log sink (direction, monotonic sequence/timestamp, method or kind,
  payload JSON) defined as a port in a neutral crate, injected at the
  composition root (`delta-bootstrap`). The Claude adapter simply never emits.
- **Observability only — not conversation.** Frames must NOT flow through
  `SessionEvent` / the persistence pipeline / attribution. No DB writes. Keep
  a bounded in-memory ring buffer per live session (a few hundred frames) so
  a client connecting mid-session sees recent history, then tails live.
  Buffer is lost on server restart; that is acceptable v1.
- **Never block the agent.** Emitting a frame must be non-blocking (bounded /
  lossy broadcast). A slow or absent UI consumer must not be able to stall a
  turn — this is the "never let the session hang invisibly" invariant that
  motivated removing the terminal for Codex in the first place.
- **Dedicated streaming endpoint** alongside `/pty` and `/ws` (e.g. a
  WebSocket keyed by session id), so the existing event stream and its
  consumers are untouched.
- **Capability-driven UI, never `provider == codex`.** Extend the curated
  `WireProviderCapabilities`
  (`backend/crates/gateway/delta-wire/src/rest/providers_response.rs`)
  following the `has_terminal` / `launch_option_style` precedent, and branch
  the right pane on the capability: providers with a terminal keep the
  terminal pane (byte-identical behavior), terminal-less adapter-backed
  providers get the comms-log pane and its own toggle button. Render frames
  time-ordered with direction + method visible at a glance and the payload
  inspectable (e.g. expandable JSON); follow the existing pane/panel idioms.

Operation × state coverage (right-pane toggle vs focused-session state):

- Focused session is Claude → terminal pane and toggle behave exactly as
  today (the comms toggle does not appear).
- Focused session is Codex, adapter live → comms pane streams frames.
- Focused session is Codex but closed/dormant (no live adapter) → pane opens
  with an explicit empty/idle state; no crash, no spinner-forever.
- Persisted pane-open state from a previous session of the other provider →
  same fail-safe handling as the existing `terminalOpen` gating (a persisted
  `true` must not open the wrong pane).
- Capability unknown (availability fetch not landed) → fail-safe: do not show
  the comms pane.

Claude remains byte-identical end to end: no change to its adapter, event
stream, persistence, or golden fixtures.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A provider-neutral comms-log port exists in a neutral crate (not inside
      `codex-agent`), and the codex adapter emits one entry per wire frame in
      both directions, covering request, response, server-request, and
      notification kinds — asserted by an adapter-level test against
      `fake-codex`.
- [x] Emission is non-blocking: a test proves a full turn completes with no
      comms-log consumer attached (and with a saturated buffer), i.e. the
      sink cannot stall the adapter.
- [x] Frames do not enter the conversation pipeline: no `SessionEvent`
      variant is added and no sqlite schema change is made for comms logs
      (`SCHEMA_VERSION` unchanged); golden / e2e-fake suites pass unchanged
      (Claude byte-identical).
- [x] A dedicated streaming endpoint serves the per-session comms log with
      ring-buffer replay followed by live tail — asserted by a server-level
      test (connect mid-session, receive buffered frames then a live one).
- [x] `GET /api/providers` exposes the new capability in
      `WireProviderCapabilities`; generated types are in sync
      (`make gen-check` clean, covered by `make check`).
- [x] `WorkspaceScreen` selects the right pane by capability (no
      `provider === 'codex'` checks): frontend tests cover the five
      operation × state rows listed in the Overview.
- [x] Frontend e2e: with the fake Codex provider, opening the comms pane
      shows time-ordered frames with direction and method visible; existing
      terminal-pane e2e scenarios for Claude pass unchanged.

### Manual / on-hardware (verified by a human before merge)

- [x] Against a real `codex app-server` session: while a turn is in flight,
      the pane shows the live request/notification flow (thread/turn/item
      frames), and a server-originated approval request is visible when one
      occurs.
- [x] Run `make e2e-real-codex` (ignored canaries) once and confirm green —
      these do not run in CI.
- [x] A Claude session on the same build shows the terminal pane exactly as
      before, and toggling between a Claude and a Codex session swaps the
      pane correctly with no stale persisted-open leakage.
