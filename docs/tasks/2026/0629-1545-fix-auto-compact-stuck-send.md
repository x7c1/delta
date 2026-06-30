---
status: completed
pipeline_phase: null
plan: null
base_ref: null
blocked_by: []
subagent_type: general-purpose
retries_remaining: 1
check_command: "cd backend && cargo build && cargo test && cargo clippy --all-targets -- -D warnings"
assignee: null
branch: task/0629-1545-fix-auto-compact-stuck-send
created_at: 2026-06-29T15:45:00Z
updated_at: 2026-06-30T03:47:43Z
---

# fix(usecase): re-dispatch outstanding sends stuck behind an auto-compact

## Overview

Resuming a Claude Code session whose context is already near the model's
budget triggers an **auto-`/compact`** the moment the TUI starts accepting
input. If the user sent a message at the same moment (or immediately after
resume), Delta's `OutstandingSend` reaches `Dispatched` (PTY keystrokes
flushed) but the TUI swallows the prompt into the compaction routine
instead of echoing it. Claude writes the compaction group to the JSONL —
a `<local-command-caveat>`, `<command-name>/compact</command-name>`, a
`type:"user"` + `isCompactSummary:true` summary, and a
`<local-command-stdout>...Compacted...</local-command-stdout>` — but
**no echo of the user's prompt ever follows**. The `Dispatched` send sits
in `outstanding` forever; the UI's pending chip stays
"In progress … awaiting reply …" until `make down`.

Observed in dogfooding on 2026-06-30 against a near-full-context resume:
the compaction group (caveat / command-name / summary / stdout) lands
at the tail of the JSONL with no user-prompt line following it; the
`Dispatched` send sits in `outstanding` until shutdown.
Claude Code 2.1.195.

This is the follow-up explicitly listed as "Out of scope" by the prior
fix at
[`docs/tasks/2026/0625-0437-fix-compaction-summary-stuck-send.md`](./0625-0437-fix-compaction-summary-stuck-send.md):

> Re-purposing `SessionStart(source=compact)` to roll outstanding
> `dispatched` sends back to `queued`. This is a sensible follow-up for
> the case where compaction lands as the literal last transcript line
> with no subsequent user prompt …

That prior task fixed the *classification* of the compaction summary
line (so it does not corrupt thread attribution); this task fixes the
*recovery* of any send stuck behind it.

### What changes

Two complementary detection paths, sharing a single re-dispatch routine.

#### Path A — `SessionStart(source=compact)` hook (live path)

1. **Add `SOURCE_COMPACT` constant.** `SessionStartHook` already exposes
   `SOURCE_STARTUP` and `SOURCE_RESUME` (see
   `backend/crates/domain/delta-usecase/src/ports/session_start_hook.rs`).
   Add `pub const SOURCE_COMPACT: &'static str = "compact";` next to
   them so the match arm is no longer a stringy `"compact"` literal.

2. **Re-dispatch in the hook.** In
   `backend/crates/domain/delta-usecase/src/interactor/hooks/on_session_start.rs`,
   replace the current catch-all `other =>` arm's no-op with an explicit
   `SessionStartHook::SOURCE_COMPACT` arm that calls a new helper
   `redispatch_stuck_dispatched(session_id)` (see "shared routine" below).
   Leave the `clear` case as a no-op — a deliberate context wipe should
   not resurrect prior sends. The catch-all stays for unknown future
   sources.

#### Path B — `Role::CompactSummary` emit during attribution (replay path)

The hook does not fire on cold-start replay (no live `SessionStart` is
delivered when the actor re-folds a JSONL on session re-open). So drive
the same recovery from ingestion too:

3. **New `Effect::AutoCompactFinished`.** Add a variant on the
   attribution `Effect` enum in
   `backend/crates/domain/delta-attribution/src/effect.rs` (or
   wherever the enum lives) carrying nothing beyond the implicit
   per-session context the caller already has.

4. **Emit it in `attribute_lines`.** In
   `backend/crates/domain/delta-attribution/src/attribute.rs`, when a
   line classifies as `Role::CompactSummary` (already produced by the
   parser per delta#187), push `Effect::AutoCompactFinished` onto
   `effects`. Do **not** otherwise change the line's role-based
   handling — it must keep inheriting `carry_thread` and emitting no
   `SendMatched`.

5. **Wire the effect into re-dispatch.** Wherever the
   attribution-side effect bus is consumed (the `sync` interactor
   that ingests transcript deltas), route `AutoCompactFinished` to the
   same `redispatch_stuck_dispatched(session_id)` helper used by the
   hook arm.

#### Shared routine — `redispatch_stuck_dispatched`

A new helper on `SessionContext` that:

- Loads the session's `outstanding` queue.
- For each entry whose `status == Dispatched`, re-types its `text` to
  the TUI via the existing `TmuxDriver` send path (the same path
  `dispatch_queued_send` uses). Order is preserved (FIFO).
- Leaves the entries' `status` as `Dispatched` — they are still
  awaiting echo, just on a fresh attempt. The next `on_user_prompt_submit`
  echo will resolve them via the existing `SendMatched` flow.

Idempotency: when the hook and the ingestion effect both fire for the
same compact (live session that *also* sees the summary line during the
same tick), only one re-dispatch must happen. Use a small monotonic
guard on `SessionState` — `last_auto_compact_redispatch_at: Option<Instant>`
plus a debounce window (e.g. 2s) — and skip the second call if the
guard is fresh.

### Why this design

- **No status rollback.** Going `Dispatched -> Queued` was the shape
  delta#187's out-of-scope note hinted at, but on inspection it
  complicates the turn machine (`AwaitingEcho` is keyed on a
  `Dispatched` head; demoting it would force an extra transition).
  Re-typing while staying `Dispatched` matches what already happens
  for `dispatch_queued_send` and keeps the state machine linear.
- **Two detection paths, one action.** The hook is the cheap live
  signal; the effect is the robust replay signal. They share the
  re-dispatch helper, so divergence between them is structurally
  impossible.
- **Leave `clear` as a no-op.** A clear deliberately drops context;
  resurrecting prior sends inverts user intent.

### Two-roundtrip risk

There is one observable risk: if Claude Code's TUI input buffer
preserves keystrokes typed during the brief moment between the user
submitting and auto-compact kicking in, the user's prompt could land
*after* compaction completes — and our re-dispatch would then send it
a second time.

Real-world frequency is unknown. The mitigation, if it shows up, is to
add a transcript-driven gate (skip re-dispatch when an unmatched
user-prompt line for this `send_id` *did* appear). That is left for a
follow-up; in this PR we accept the theoretical second send because
the alternative is the current bug where the user sees nothing forever.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `SessionStartHook::SOURCE_COMPACT` constant exists and the
      `on_session_start` `match` arm dispatches on it (no stringy
      `"compact"` literal in the arm).
- [x] A new helper `redispatch_stuck_dispatched(session_id)` (or
      equivalent on `SessionContext`) re-types each `Dispatched`
      `OutstandingSend` for the session, in FIFO order, via the existing
      `TmuxDriver` send path, leaving statuses as `Dispatched`. Exercised
      by a unit test that seeds two dispatched sends and asserts both
      get re-typed in order on a single call.
- [x] `session_start_compact_redispatches_stuck_dispatched` (rewritten
      from / split out of the existing
      `session_start_clear_and_compact_are_noops`): seeding one
      `Dispatched` send and firing `SessionStart(source=compact)`
      re-types that send's text exactly once. Companion
      `session_start_clear_is_noop` keeps the `clear` half of the old
      assertion (no re-dispatch).
- [x] A new variant `Effect::AutoCompactFinished` is emitted by
      `attribute_lines` exactly when the line classifies as
      `Role::CompactSummary`, and is **not** emitted for plain
      `Role::User` / `Role::Meta` / `Role::Other` lines. Asserted by a
      unit test in `delta-attribution`.
- [x] A `sync`-interactor integration test: ingesting a JSONL fragment
      whose last line is an `isCompactSummary:true` record, with one
      outstanding `Dispatched` send, leads to that send being re-typed
      exactly once.
- [x] Idempotency: when the same compact event hits both the hook and
      the ingestion effect within the debounce window, re-dispatch fires
      exactly once. Asserted by a unit test that triggers both back-to-back.
- [x] Existing `local_command_unsticks_turn_and_folds_to_meta` and the
      delta#187 attribution tests stay green (the new emit must not
      change `Role::CompactSummary`'s thread / `SendMatched` semantics).
- [x] `cd backend && cargo build && cargo test && cargo clippy --all-targets -- -D warnings`
      passes (also the configured `check_command`).
- [x] An e2e-fake spec drives fake-claude through the auto-compact
      sequence (caveat → command-name → summary → stdout) after a
      dispatched send, and asserts the UI's pending chip clears within
      the existing first-reply-window timeout.

### Manual / on-hardware (verified by a human before merge)

- [ ] On a real `make dev` resume of a near-full-context session, send
      a message at the resume moment, watch auto-compact kick in, and
      confirm the previously-stuck pending chip clears within a few
      seconds (matching the prompt that lands in the TUI after
      compaction finishes).
- [ ] No spurious second submission appears on the recipient side
      under the common case (compaction drops the typed prompt). If a
      second submission does occur — i.e. the TUI's input buffer
      survived compaction — record the repro shape so the follow-up
      gate (transcript-driven suppression) can be scoped.

## Out of scope

- Transcript-driven suppression of the re-dispatch when the user's
  prompt actually survived compaction. Worth doing only if the
  "two-roundtrip" risk is observed in practice; the gate would consult
  whether the session's transcript already carries an unmatched
  `type:"user"` line whose text equals the outstanding send before
  re-typing. Tracked as a follow-up under the same sub-plan.
- A separate latent bug: `<command-name>/foo</command-name>`-wrapped
  command-name lines from Claude Code 2.1.x do not byte-equal the bare
  text Delta dispatches, so `is_local_command_name_line` cannot consume
  a slash-command send today. It manifests if a user ever dispatches
  `/compact` (or any slash command) **via Delta itself**, but is
  unrelated to the resume-time auto-compact path this task fixes. Open
  a separate task if/when that is observed.
- UI surfacing of a "session compacted here" marker. delta#187 added
  `Role::CompactSummary` precisely to enable this later, but the UI
  work stays deferred.
- Promoting `SessionStart(source=clear)` to do anything. A clear is a
  deliberate context wipe; resurrecting prior sends inverts intent.
