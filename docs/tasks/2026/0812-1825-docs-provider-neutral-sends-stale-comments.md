---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: 'cd backend && cargo fmt --all -- --check && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && ! grep -rn "gh search prs" backend/crates/domain && grep -q "ascending" backend/crates/gateway/delta-wire/src/endpoint/table.rs && ! grep -q "turn_started" backend/crates/apps/delta-server/src/ws.rs && BEFORE=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && make gen && AFTER=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && [ "$BEFORE" = "$AFTER" ] && cd frontend && pnpm -r build && pnpm -r typecheck && pnpm -r test && pnpm -r lint && cd .. && make e2e'
assignee: null
branch: task/0812-1825-docs-provider-neutral-sends-stale-comments
created_at: 2026-08-12T18:25:00Z
updated_at: 2026-08-12T21:16:00Z
---

# docs: provider-neutral send wording and stale doc-comment fixes

## Overview

Four small corrections surfaced (and deliberately deferred as out-of-scope or
outside the file set) by the recent API documentation passes. Each is
independent and mechanical; together they are one focused docs/comments PR.
No behavior changes.

1. **`docs/guides/api/sends.md` describes the send path in Claude-only
   terms.** The Overview says a send is "dispatched into its tmux pane as
   keystrokes", and the `POST /api/sends` summary repeats it — unconditional,
   although Codex sessions have no tmux pane. The rest of `docs/guides/api/`
   (README's adapter overview, `live-channels.md`, `settings.md`) is
   provider-qualified. Reword the dispatch description to be
   provider-qualified: verify the actual Codex send path in the code first
   (the queued send is dispatched through the adapter's turn-start request on
   the `codex app-server` connection rather than typed keystrokes — confirm
   in `backend/crates/domain/delta-usecase/src/interactor/` and the
   `codex-agent` crate; do not invent semantics). Claude's keystroke wording
   may stay where it is explicitly scoped to Claude.
2. **`backend/crates/gateway/delta-wire/src/endpoint/table.rs`: the
   `ListThreads` declaration's doc comment says "newest first", but the
   implementation orders threads by ascending `id`** (the store queries
   `ORDER BY id`; `docs/guides/api/sessions.md` documents "ordered by
   creation (ascending `id`)", which is correct). Fix the doc comment to say
   ascending/creation order.
3. **`backend/crates/domain/delta-usecase/src/interactor/pull_requests/list_pull_requests.rs`:
   the doc comment still says the listing "drives `gh search prs`"**, but the
   gateway (`backend/crates/gateway/gh-cli/src/gh.rs`) queries the GitHub
   search API via `gh api graphql` (and its comments explain why `gh search
   prs --json` is not usable). Update the doc comment to match.
4. **`backend/crates/apps/delta-server/src/ws.rs`: the module doc enumerates
   only a stale subset (8) of the 18 `SessionEvent` kinds.** Stop enumerating
   event kinds there — point at the union type (`WireSessionEvent` /
   `delta_usecase::SessionEvent`) and the API guide
   (`docs/guides/api/live-channels.md`) instead, so the comment cannot drift
   again when a variant is added.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] No file under `backend/crates/domain` mentions `gh search prs`
      (`! grep -rn "gh search prs" backend/crates/domain` is part of
      `check_command`).
- [x] The `ListThreads` doc comment in `endpoint/table.rs` describes
      ascending/creation order (`grep -q "ascending" …/endpoint/table.rs` is
      part of `check_command`), matching `sessions.md` and the store's
      `ORDER BY id`.
- [x] `ws.rs`'s module doc no longer enumerates individual event kinds
      (`! grep -q "turn_started" …/ws.rs` is part of `check_command`); it
      references the union type and the API guide instead.
- [x] The generated TypeScript bindings are byte-identical before and after
      (`make gen` hash comparison in `check_command`): comments and prose
      only, no wire-shape edits.

### Manual / on-hardware (verified by a human before merge)

- [x] `docs/guides/api/sends.md` no longer states unconditionally that a send
      is typed into a tmux pane: the dispatch description is
      provider-qualified and matches the actual Codex send path in the code.

## Out of scope

- The `PendingSend`-vs-`Send` naming divergence between the docs and the wire
  type (a rename decision, tracked separately).
- Any change to send behavior, routing, or wire shapes.
