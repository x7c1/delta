---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && [ "$(sed -n "/fn compact_group/,/^    }$/p" backend/crates/apps/fake-claude/src/transcript.rs | grep -c "self.append(")" -eq 0 ]'
assignee: null
branch: task/0912-1244-fix-write-the-fake-compact-group-as-one-append
created_at: 2026-09-12T12:44:43Z
updated_at: 2026-09-12T13:18:58Z
---

# fix(fake-claude): write the compact group as one append with one timestamp

## Overview

`e2e-fake/compact-swallow.spec.ts` fails under load — twice on 2026-09-12,
both times in a full `make check`, never when run alone — at the assertion
that waits for `second turn ack after compaction`. The cause is a fidelity
gap in the fake, not a server bug.

Claude Code writes a `/compact` group — caveat, bare command name, summary,
stdout, all sharing one `promptId` — as **one atomic transcript append with
one shared timestamp**. The attribution fold relies on that: its
`local_command_prompts` set (`backend/crates/domain/delta-attribution/src/attribute/state.rs`,
doc comment on the field) is deliberately not seeded from the store, because
"the whole group always lands in a single tail batch in production", and the
server re-seeds `AttributionState` from the store on every 500 ms poll
(`backend/crates/domain/delta-usecase/src/interactor/sync/conversation_source.rs:119`),
so the set starts empty each time.

The fake breaks that contract. `TranscriptWriter::compact_group`
(`backend/crates/apps/fake-claude/src/transcript.rs:189`) calls `append` four
times, and each `append` opens the file, writes one line, and stamps its own
`rfc3339_now()`. When the tail's poll lands between the caveat and the
command-name line — rare, but the odds rise with the load a full `make check`
puts on the machine — the second poll sees a `role: user` line reading
`/compact` whose `promptId` is not in the (empty) set. It is classified as a
human turn, consumes the outstanding `Dispatched` send as its echo, and moves
the turn machine out of `AwaitingEcho`. The compact summary that follows still
emits `AutoCompactFinished`, but `redispatch_stuck_dispatched`
(`interactor/enqueue/redispatch_stuck.rs`) finds nothing awaiting an echo and
re-types nothing; the fake's `await_prompt` never fires; the spec times out.

Make the fake write the group the way Claude Code does, so the scenario it
models is the one the server is built for.

### Design

1. **One write, one timestamp.** In `transcript.rs`, add a private helper on
   `TranscriptWriter` that takes several prepared line bodies, stamps them
   with a single `rfc3339_now()` value and consecutive uuids on the chain,
   serialises them joined by `\n` (trailing newline included) and appends
   them with **one** `write_all` on an append-mode handle. Advance `next_seq`
   and `last_uuid` per line exactly as `append` does today, so the uuid
   chain and the resume logic in `open` are unchanged. Rewrite `compact_group`
   to build its four bodies and hand them to this helper; it must no longer
   call `append` per line (the gate appended to `check_command` pins that).
   Keep `append` and `write_line` for the single-line steps.
2. **Say why.** The helper's doc comment states the contract it upholds: a
   local-command group is one atomic append with a shared timestamp in the
   real transcript, and the server's attribution fold depends on a group
   never being cut by a tail poll. Update the `compact_group` row of the step
   table in `backend/crates/apps/fake-claude/src/scenario.rs:47` to say the
   group is written atomically.
3. **Pin it.** In `transcript.rs`'s existing `#[cfg(test)]` module add tests
   that, after `compact_group`, the file holds exactly four new lines which
   share one `promptId` and one `timestamp`, keep consecutive uuids chained
   through `parentUuid`, and carry `isMeta` on the first and
   `isCompactSummary` on the third; and that a following single-line append
   continues the chain from the fourth line's uuid. Atomicity of the syscall
   itself is not unit-testable here; the single-`write_all` shape plus the
   gate is the pin.
4. **Do not touch** the server's attribution fold, `local_command_prompts`
   seeding, the redispatch path, the e2e spec, or its scenario JSON. The spec
   is correct as written; only the fake's write pattern is wrong.

### Session-state coverage

No user-facing operation changes; this is a test double. Not applicable.

### Pipeline notes

- Rust only (`fake-claude`). No wire change, no `make gen`.
- `make check` takes over ten minutes and needs tmux on the host; the check
  phase is expected to run it through the driver's long-running path rather
  than a capped worker call.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `compact_group` no longer appends line by line: the function body
      contains no `self.append(` call (the gate appended to `check_command`).
- [x] After `compact_group`, the transcript holds exactly four new lines
      sharing one `promptId` and one `timestamp`, with consecutive uuids
      chained through `parentUuid`, `isMeta` on the caveat and
      `isCompactSummary` on the summary (cargo test, `fake-claude`).
- [x] A single-line append after `compact_group` chains from the fourth
      line's uuid, and reopening the writer resumes numbering after the group
      (cargo test, `fake-claude`).
- [x] `e2e-fake/compact-swallow.spec.ts` passes inside `make check`.

### Manual / on-hardware (verified by a human before merge)

- [ ] `make check` run twice in a row on a busy machine keeps
      `compact-swallow.spec.ts` green (the flake reproduced only under
      full-gate load, so a single green run is weak evidence on its own).

## Out of scope

- Seeding `local_command_prompts` from the store so the server tolerates a
  split group even from a non-atomic writer. That would reverse a
  deliberate design decision recorded in `state.rs` and needs its own
  discussion.
- Any change to the spec, the scenario, the redispatch debounce, or the
  echo-deadline watchdog.
