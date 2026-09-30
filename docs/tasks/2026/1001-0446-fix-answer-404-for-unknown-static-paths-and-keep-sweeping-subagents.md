---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure, error-type-design]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1001-0446-fix-answer-404-for-unknown-static-paths-and-keep-sweeping-subagents
created_at: 2026-09-30T19:46:01Z
updated_at: 2026-09-30T19:58:47Z
---

# fix(server): answer 404 for unknown static paths and keep sweeping subagents past a store error

## Overview

Two small backend behaviour fixes.

**Unknown non-HTML paths answer 401 instead of 404 in the packaged app.**
`StaticWeb::lookup` in
`backend/crates/apps/delta-server/src/app/static_web/mod.rs` hands every
request it does not serve — including an ordinary `GET` outside the reserved
prefixes that does not accept HTML — to the API router's own fallback, which
sits behind the bearer guard. A browser's automatic `GET /favicon.ico` (no
bearer token) therefore gets `401` rather than `404`. Change `lookup` so that a
`GET`/`HEAD` outside the reserved prefixes (`request_scope::RESERVED_PREFIXES`)
that matches no file and does not accept HTML answers `404` itself, without
going through the guarded fallback. Reserved-prefix paths and other methods
keep going to the API fallback exactly as today (`401` without a token). Update
the module doc's "Everything else — ..." paragraph to match. The Origin/Host
guard that already wraps the static surface still applies.

**A store failure mid-sweep drops every `SubagentFinished`.**
`sweep_running_subagents_on_process_gone` in
`backend/crates/domain/delta-usecase/src/interactor/sweep_running_subagents.rs`
drains the in-memory running-subagent set first and then calls
`clear_subagent_launch(...).await?` per entry. One failing clear returns the
error and discards the events of every entry already drained, so a live
viewer's indicator for those subagents never clears. Make the sweep keep
going: when clearing one launch row fails, log it with `tracing::warn!`
(session id, tool-use id, error) and still emit that entry's
`SubagentFinished` — the process is gone either way, and the in-memory entry is
already drained. Return `Vec<SessionEvent>` without an error if nothing else in
the function can fail, and simplify the three process-gone call sites
accordingly. Update the doc comment to state the new failure behaviour.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A `static_web` route test: `GET /favicon.ico` with no bearer token and an
      `Accept` that does not include HTML answers `404`.
- [x] A `static_web` route test: a `GET` under a reserved prefix with no bearer
      token still answers `401`.
- [x] A usecase test with a fake store whose `clear_subagent_launch` fails for
      one of three running subagents: the sweep returns a `SubagentFinished`
      for all three.
