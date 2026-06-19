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
branch: task/0618-2346-feat-message-metadata
created_at: 2026-06-18T14:44:56Z
updated_at: 2026-06-18T19:48:00Z
---

# feat(web): show per-message model, working dir, branch and response time

## Overview

The transcript JSONL already carries useful per-message metadata that delta
currently discards while tailing. Each assistant line includes `message.model`
(the model that produced that message), `gitBranch` and `cwd` (the working
directory and git branch at that turn), and the conversation also records a
`system` line of subtype `turn_duration` with `durationMs` (turn latency). This
task surfaces that metadata in the conversation view. It is independent of the
statusLine work — it reads only the transcript and adds UI, so it can land on
its own.

This data is distinct from the statusLine "currently selected model": the
transcript model is **which model actually produced that message** (historical,
per message), whereas the status line reports the user's current selection. They
usually match and differ only right after `/model` before the next turn.

Note on scope of each field: `cwd` is effectively fixed for the lifetime of a
session, while `gitBranch` can change mid-session (e.g. a `git checkout` between
turns), so the branch is the value that meaningfully varies per message.

### Backend (parsing + wire)

The transcript parser under `backend/crates/gateway/delta-transcript/src/parse/`
currently reads only `role`/`content` from the embedded message and ignores
top-level fields. Extend it to also extract, per assistant message:
`model`, `gitBranch`, `cwd`, and the turn's response time (correlate the
`system` `turn_duration.durationMs` line to its turn). Carry these through to the
browser by adding optional fields to the message wire type so the frontend can
render them; run `make gen` and commit the regenerated TypeScript bindings. All
new fields are optional — older/other line shapes may not have them.

### Frontend (rendering)

In the conversation view (`frontend/packages/apps/web/src/features/transcript/`,
the message item component), add a small right-aligned metadata line beneath
each assistant message:

- **Latest message only** — two lines, right-aligned:
  - line 1: `<model> · <time> · <info icon>`
  - line 2: `<cwd> · ⑂<branch>`
  The latest message's inline `cwd`/`branch` doubles as the "current working
  location" indicator (there is intentionally no separate session header).
- **Older messages** — a single right-aligned `<time> · <info icon>`.
- **Info icon hover** (all messages) — a small popover showing that message's
  **model, response time, cwd and branch**. Do not show token counts or cache
  ratios; keep the popover to those four facts.

`<time>` is the message timestamp; `<model>` is the message's own model (not the
current selection); response time comes from the correlated `turn_duration`
`durationMs`.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The transcript parser extracts `model`, `gitBranch`, `cwd`, and the
      turn's `durationMs` for assistant messages, asserted by a parser unit test
      over a transcript fixture that includes an assistant line and a
      `turn_duration` system line (the duration is correlated to the right
      turn).
- [x] The new optional metadata fields appear on the message wire type and the
      regenerated TypeScript bindings are committed (`make check` reports no
      `make gen` diff).
- [x] A component test asserts the latest assistant message renders the two-line
      meta (`model · time · info` and `cwd · branch`) while an older assistant
      message renders only `time · info`.
- [x] A component test asserts the info-icon hover popover content includes
      model, response time, cwd and branch, and does not include token/cache
      figures.
- [x] `make check` passes (backend build + tests + clippy, frontend build +
      typecheck + vitest + lint, wire-gen no-diff).

### Manual / on-hardware (verified by a human before merge)

- [ ] In `make mock` (or against a live session), the per-message meta line and
      the info-icon hover render correctly and read well in the conversation
      view. (Visual/UX confirmation the unit suite cannot make.)
- [ ] A Playwright e2e spec is added under `packages/apps/web/e2e/` that drives
      mock mode with a transcript carrying the new metadata and asserts the
      latest-message two-line meta and the info-icon hover popover. It passes
      under `make e2e` (run by a human / separate job — not part of the
      `make check` pipeline gate).

## Out of scope

- Token counts, cache hit ratios, cost — deliberately excluded from the
  per-message meta.
- statusLine-derived data (context %, rate limits) — separate tasks.
- A session-level header bar (rejected in design in favour of the
  latest-message meta line).
</content>
