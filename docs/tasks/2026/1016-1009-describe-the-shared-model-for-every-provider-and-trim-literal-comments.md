---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! git grep -nE 'single Claude Code (TUI )?session' -- backend/crates/domain/delta-model/src && ! git grep -nE 'canonical 36-char|first 8 hex characters' -- frontend/packages/domain/model/src/display-branch.ts"
assignee: null
branch: task/1016-1009-describe-the-shared-model-for-every-provider-and-trim-literal-comments
created_at: 2026-10-16T10:09:25Z
updated_at: 2026-10-16T12:58:11Z
---

# docs(model): describe the shared session and send types for every provider and trim comments that restate code

## Overview

The domain model in `backend/crates/domain/delta-model` is shared by both
providers, Claude Code and Codex: a session carries a `provider`, the API
exposes it, and the conversation model was designed provider-neutral. Its doc
comments still describe the types as Claude-only. `src/lib.rs` (line 3) says
Delta drives a single Claude Code TUI session; `src/session.rs` (lines 1 and
7) calls `Session` the single Claude Code TUI session Delta wraps;
`src/session_id.rs` (line 6) defines `SessionId` as the Claude session id
from hooks; `src/session_status.rs` (lines 1 and 5) calls it the lifecycle of
a Claude Code session; `src/send.rs` (lines 1–6) defines every send as input
typed into a tmux pane and matched by the `UserPromptSubmit` hook. Those are
Claude's mechanics, not the model's meaning: a Codex session has no tmux pane,
no hook and no JSONL transcript.

Rewrite those comments so the shared types are defined as Delta's own concepts
— a session Delta started with one provider, its identifier, its lifecycle, and
a recorded user input awaiting correlation with the conversation — and state
Claude's mechanics (tmux, hooks, the JSONL transcript, the hook-minted id) as
the Claude-specific case where they still matter, or move them to the comment
of the adapter-side code that implements them. Check the `transcript_path`
comment on `Session` in the same pass: it currently reads as if the path is
`None` only while spawning, but a Codex session never has one; say so.
Apply the same test to the other files under `delta-model/src` whose comments
name Claude (for example `newtype.rs`, `content.rs`, `prompt_id.rs`,
`permission_request.rs`) and fix the ones that present a Claude detail as the
definition of a shared type. A comment that legitimately documents a
Claude-only field or variant (`agent_provider.rs`, `launch_option.rs`,
`role.rs`'s compact line) stays as it is. Do not rename any identifier, and do
not touch the database or wire names.

In the frontend, `frontend/packages/domain/model/src/display-branch.ts`
(lines 1–31) describes the regular expression and the string slicing
literally: the UUID's character count, the prefix, the first eight characters,
and a list of names that do not match. Keep the reasons a reader cannot
recover from the code — only Delta-managed branches are shortened, the stored
value never changes, and the caller is expected to expose the full name on
hover — and drop the restatement of the pattern and of the return value. The
table-driven tests already show the behaviour.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] No doc comment under `backend/crates/domain/delta-model/src` describes
      the model as a single Claude Code session (`git grep` gate appended to
      `check_command`).
- [x] The `display-branch.ts` comment no longer restates the pattern's
      character counts or the first-eight-characters slice (`git grep` gate
      appended to `check_command`).
- [x] The rustdoc gate in `make check` passes with the rewritten comments
      (every intra-doc link still resolves).
- [x] No Rust or TypeScript identifier, database column, or wire field is
      renamed (the generated wire types in `make check` are unchanged).

## Out of scope

- Renaming `Session`, `SessionId`, `Send` or any other identifier.
- Comments in `delta-usecase`, the gateway crates, or the Claude hook
  payloads; only the shared model crate and `display-branch.ts` are in scope.
