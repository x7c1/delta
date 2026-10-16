---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! git grep -nE 'ForkCapability|SteerCapability|pub fast_mode|pub output_tokens' -- backend/crates && ! git grep -n 'allow(dead_code)' -- backend/crates/gateway/delta-transcript"
assignee: null
branch: task/1016-1011-drop-the-unread-capabilities-and-payload-fields
created_at: 2026-10-16T10:11:14Z
updated_at: 2026-10-16T15:15:02Z
---

# refactor(backend): drop the fork and steer capabilities nothing reads and the payload fields nothing consumes

## Overview

Two groups of backend declarations are set or deserialized but never read.

Capabilities. `AgentCapabilities` in
`backend/crates/domain/delta-usecase/src/agent/capabilities/mod.rs` (lines
61–64) carries `fork: ForkCapability` and `steer: SteerCapability`
(`fork_capability.rs`, `steer_capability.rs`). Every profile sets both to
`None` — Claude's in `backend/crates/gateway/claude-agent/src/lib.rs`,
Codex's in `backend/crates/gateway/codex-agent/src/adapter/mod.rs`, the test
fake in `delta-usecase/src/interactor/testing/fake_agent.rs`, the wire
fixture in `delta-wire/src/rest/providers_response.rs` — and no code reads
`.fork` or `.steer`; `NativeThreadFork` and `MidTurn` are defined and never
constructed. The field comments say they are carried for the future. Delete
the two fields, the two enums, their modules, their re-exports in
`agent/capabilities/mod.rs`, `agent/mod.rs` and the crate root, and every
initializer. Rewrite the comments that mention them so they describe the
present: the `CLAUDE_CAPABILITIES` doc in `claude-agent` ("Fork/steer are
unused in v1"), and the prose in `enqueue/enqueue_send.rs` (around line 133),
`codex-agent/src/content/mod.rs` (around lines 72 and 101), and the test
comments in `enqueue/tests/codex_second_send_dispatches_over_the_adapter.rs`
and `fake-codex/tests/full_loop/branching.rs` that explain a branch send is
accepted because there is no `ForkCapability` rejection any more. Those
sentences describe a removed rejection; state the current rule (a branch send
is an ordinary send on the branch thread) without naming the deleted type.
Branch sends and Codex mid-turn sends themselves are untouched.

Payload fields. Three deserialized fields have no consumer (`workspace` is
not one of them: `status_snapshot_from` projects `workspace.current_dir` onto
`StatusSnapshot.current_dir`, which is on the wire, so it stays):

- `RawMessage::role` in
  `backend/crates/gateway/delta-transcript/src/parse/raw_message.rs` (lines
  9–10) is marked `allow(dead_code)`; the parser derives the role from the
  top-level `line_type` (`parse/mod.rs`). Delete the field and the
  attribute.
- `StatusLinePayload::fast_mode` in
  `backend/crates/gateway/delta-wire/src/hooks/status_line_payload.rs` (line
  48) and `StatusLineCurrentUsage::output_tokens` in
  `status_line_current_usage.rs` (line 11). `status_snapshot_from` in
  `backend/crates/apps/delta-server/src/hooks/mod.rs` (around line 395)
  projects neither of them. Delete the two fields. `cost` and `workspace` are
  projected and stay.

These structs rely on serde's default of ignoring unknown fields, so a payload
that still carries `fast_mode` or `output_tokens` must keep deserializing. Confirm no `deny_unknown_fields` is in play and do not remove
those keys from any existing JSON fixture (the status-line route test in
`delta-server/src/app/tests/status_line.rs` sends both); the fixtures
passing unchanged is the evidence that the tolerance holds. Update the
`status_line_payload.rs` module comment, which currently cites `fast_mode` as
the example of a field Delta models without using.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `ForkCapability` and `SteerCapability` no longer appear under
      `backend/crates`, in code or comments (`git grep` gate appended to
      `check_command`).
- [x] `pub fast_mode` and `pub output_tokens` no longer appear under
      `backend/crates`, and no `allow(dead_code)` remains in
      `delta-transcript` (`git grep` gates appended to `check_command`).
- [x] The status-line route test still sends `fast_mode` and `output_tokens`
      and still passes, proving unknown keys are tolerated (`make check`).
- [x] Transcript fixtures are unchanged and every line still parses to the
      same `Role` (`delta-transcript` tests in `make check`).
- [x] The generated wire types and the `/api/providers` capability projection
      are unchanged: the wire freshness check in `make check` passes without
      regenerating anything, and the existing provider-display and branch-send
      tests pass.

## Out of scope

- Any other capability field, and the `WireProviderCapabilities` projection.
- The streaming-preview runtime; a separate change covers it.
