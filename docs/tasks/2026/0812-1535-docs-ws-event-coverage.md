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
branch: task/0812-1535-docs-ws-event-coverage
created_at: 2026-08-12T15:35:18Z
updated_at: 2026-08-12T18:12:00Z
---

# docs(api): document every /ws session event and gate coverage on the wire union

## Overview

`docs/guides/api/live-channels.md`'s `GET /ws` section documents 8 of the 18
`WireSessionEvent` variants
(`backend/crates/gateway/delta-wire/src/session_event.rs`), while claiming
"the union below cannot drift from the implementation" — a claim that is true
of the generated TypeScript bindings but not of this prose, which has already
drifted. The route-level coverage gate added by
`backend/crates/gateway/delta-wire/tests/api_docs_cover_every_route.rs`
cannot see this: `/ws` is one route, and the gap is per event variant. This
task documents the missing variants and adds the event-level counterpart of
that gate.

Missing variants (each needs a JSON example in the code block and an
explanatory bullet, in the same style as the existing eight):
`send_dispatched`, `send_parked`, `turn_interrupted`, `question_asked`,
`permission_resolved`, `spawn_failed`, `assistant_streaming`,
`subagent_started`, `subagent_finished`, `status_updated`.

Write from the source of truth: each variant's doc comment on
`WireSessionEvent` and its domain twin (`delta-usecase`'s `SessionEvent`),
and — where the doc comment alone does not pin the semantics — the emitting
code. Do not invent semantics; describe only what the code establishes.
Payload field meanings matter (e.g. `send_parked`'s reason and returned text,
`status_updated`'s snapshot shape with provider-keyed rate-limit windows,
`assistant_streaming`'s relation to the final message via `provider_item_id`,
`subagent_*`'s tool_use correlation). With 18 events, consider grouping the
bullets under small thematic sub-lists (session lifecycle / send-and-turn /
permissions-and-questions / streaming-and-subagents / status) so the section
stays scannable; keep one code-block example per variant.

Gate: add a test (in the existing
`api_docs_cover_every_route.rs` or a sibling test file in
`backend/crates/gateway/delta-wire/tests/`) that iterates
`delta_wire::event_kinds()` (`session_event.rs:372`, already public and
pinned to enumerate every variant in declaration order) and asserts each kind
string appears in `docs/guides/api/live-channels.md` as a documented kind
(match the JSON form `"kind": "<name>"` so prose mentions do not count).
Failure messages must name the undocumented kind. Then update the "cannot
drift" sentence to state the real invariant: the TS types are generated, and
this prose is held to the union by the coverage test.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A `delta-wire` test iterates `event_kinds()` and asserts every kind
      appears in `docs/guides/api/live-channels.md` in its JSON form
      (`"kind": "<name>"`); it runs under `cargo test`, so leaving any of the
      ten variants listed in the Overview undocumented fails the check.
- [x] Each of the ten missing variants has both a JSON example and an
      explanatory bullet in the `GET /ws` section, consistent in style with
      the existing eight.
- [x] The "cannot drift from the implementation" sentence is replaced with an
      accurate statement of what is generated (TS bindings) and what is
      test-enforced (this prose's event coverage).
- [x] The generated TypeScript bindings are byte-identical before and after
      (`make gen` hash comparison in `check_command`): this change is docs
      plus tests, no wire-shape edits.

### Manual / on-hardware (verified by a human before merge)

- [x] In a live session (`make dev`), observe at least one newly documented
      event on `/ws` (e.g. `send_dispatched` when a queued send types into
      the pane, or `status_updated` after a turn) and confirm the received
      frame matches the documented shape.

## Out of scope

- Documenting `/comms` frame payload methods per provider (the `CommsFrame`
  envelope is already documented; the JSON-RPC methods inside it belong to
  the provider's protocol, not Delta's contract).
- Any change to event behavior or wire shapes.
- The `PendingSend`-vs-`Send` naming divergence between the docs and the wire
  type (tracked separately).
