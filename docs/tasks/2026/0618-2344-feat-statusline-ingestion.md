---
status: completed
pipeline_phase: null
plan: null
base_ref: null
blocked_by: []
subagent_type: general-purpose
retries_remaining: 1
check_command: "cd backend && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cd ../frontend && pnpm -r build && pnpm -r typecheck && pnpm -r test && pnpm -r lint"
assignee: null
branch: task/0618-2344-feat-statusline-ingestion
created_at: 2026-06-18T14:44:56Z
updated_at: 2026-06-18T16:42:00Z
---

# feat(server): ingest Claude Code statusLine JSON and broadcast it to the UI

## Overview

Claude Code has a `statusLine` feature: a configured `command` receives, on
**stdin**, a JSON snapshot of session state (selected model, context-window
usage, rate limits, cost, workspace) every time the TUI status line refreshes.
None of that information is in the transcript JSONL, so the server cannot
currently surface it. This task makes the server ingest that JSON the same way
it already ingests Claude Code hooks (an HTTP callback), and rebroadcast it to
connected browsers over the WebSocket stream. This is backend-only — no UI is
added here (a follow-up consumes the new event).

The mechanism mirrors the existing `SessionStart` hook exactly. Today the server
generates the per-session settings JSON it hands to `claude --settings <path>`
in `backend/crates/libs/delta-bootstrap/src/settings.rs`
(`render_session_settings`). `SessionStart` is wired there as a `command` hook
that `curl`s the server. Add a `statusLine` entry alongside it:

- Inject a `"statusLine"` block into the generated settings whose `command`
  reads stdin and POSTs it to a new server endpoint, e.g.
  `curl -sS --data-binary @- http://127.0.0.1:<port>/hooks/status-line`. The
  port must come from the same value `render_session_settings` already uses so
  it always matches the listening port.
- Claude Code renders the command's **stdout** as the status-line text in its
  (delta-embedded) terminal; since delta shows this data in the web UI instead,
  the command should emit an empty/minimal string to stdout after POSTing (same
  spirit as the existing `SessionStart` hook discarding its response).

Server side, add the route + handler + wire payload following the existing hook
plumbing under `backend/crates/apps/delta-server/src/` (`app.rs` route table,
`src/hooks/mod.rs` handlers) and `backend/crates/gateway/delta-wire/src/hooks/`
(payload types):

- New route `POST /hooks/status-line` in `app.rs`, handler in `src/hooks/mod.rs`.
- New `StatusLinePayload` in `delta-wire/src/hooks/` capturing at least:
  `session_id`, `model` (`id`, `display_name`), `context_window`
  (`used_percentage`, `context_window_size`, `current_usage`,
  `total_input_tokens`), `rate_limits` (`five_hour` / `seven_day`, each with
  `used_percentage` + `resets_at`), `cost` (`total_cost_usd`), `workspace`
  (`current_dir`), and `fast_mode`. **Every field that can be absent must be
  modeled as `Option`** — measured behavior of Claude Code v2.1.179 confirms
  that before the first API response `rate_limits` is entirely absent and
  `context_window.current_usage` / `used_percentage` are `null`; `rate_limits`
  is also absent on accounts without a Pro/Max subscription. `resets_at` is Unix
  epoch seconds. Unknown/extra fields must be tolerated (Claude Code adds fields
  across versions — `fast_mode` is one example not in the public schema).
- A new browser-facing event variant (e.g. `StatusUpdated`) on the wire
  `SessionEvent` union (`delta-wire/src/session_event.rs`), broadcast over `/ws`
  through the same path the other hooks use. Because the status line fires
  frequently, this is a "latest value", not an append — the event should carry
  the current snapshot keyed by `session_id`. Unlike the raw hook payloads
  (which are server↔Claude-Code only and not exported to TypeScript), this event
  reaches the browser, so it must flow through the TS-exported wire types; run
  `make gen` and commit the regenerated bindings.

Keep the snapshot's units faithful: forward `used_percentage` from
`context_window` directly (it is precomputed by Claude Code against the correct
`context_window_size`); do not attempt to recompute a percentage from token
counts.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `render_session_settings` emits a `statusLine` block whose `command`
      targets `/hooks/status-line` on the same port as the other hooks, asserted
      by a unit test in `delta-bootstrap` (the test inspects the generated
      settings JSON).
- [x] A `delta-server` test issues `POST /hooks/status-line` with a sample
      payload (including the API-response-present shape with `rate_limits` and
      `context_window.used_percentage`) and asserts it deserializes and produces
      a `StatusUpdated`-style `SessionEvent` carrying `session_id`,
      `used_percentage`, and the 5h/7d rate-limit values.
- [x] A second `delta-server` (or `delta-wire`) test feeds the
      **pre-API-response** shape — `rate_limits` key absent and
      `context_window.current_usage` / `used_percentage` null — and asserts it
      deserializes without error (all optional), proving the `Option` modeling.
- [x] A payload containing an unknown extra top-level field deserializes
      successfully (forward-compatibility), asserted by a test.
- [x] Regenerated TypeScript wire bindings are committed and `make check`
      reports no `make gen` diff.
- [x] `make check` passes (backend build + tests + clippy, frontend typecheck,
      wire-gen no-diff).

### Manual / on-hardware (verified by a human before merge)

- [ ] Against a real `claude` session spawned by the server, the injected
      `statusLine` command actually fires and the server receives
      `POST /hooks/status-line` (this end-to-end "Claude Code invokes our
      command" path cannot be exercised by the mock/unit suite — it depends on
      the real CLI). Probe evidence shows the status line fires for `claude` run
      under tmux, so this is a confirmation, not an open risk.

## Out of scope

- Any UI rendering of the new data (separate follow-up task).
- Transcript-derived per-message metadata (separate task).
- Cost/usage history or persistence — this task only broadcasts the latest
  snapshot; it does not store it.
</content>
