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
branch: task/0809-0634-fix-send-echo-attachment-resend-loop
created_at: 2026-08-09T06:34:41Z
updated_at: 2026-08-09T08:40:13Z
---

# fix(send): stop the infinite re-dispatch loop when a send's echo can never match

## Overview

A message sent from Delta's composer with an **image attachment** was
re-typed into the Claude session **38 times in a row**, one re-send per
completed turn, until the user manually cancelled the send. Live-log
post-mortem (dev server, session log):

1. The composer send's `text` is the message body plus the attachment's
   absolute path on its own line (shell-escaped spaces). Delta types the
   whole text into the pane and the send reaches `dispatched`.
2. Claude Code recognizes the pasted path and converts it into an
   attachment; the `UserPromptSubmit` hook reports the prompt as
   `[Image #2]<body>` — not the raw text Delta typed.
3. The send⇄echo correlation is exact text equality
   (`send.text.trim() == hook.prompt.trim()` in
   `backend/crates/domain/delta-usecase/src/interactor/hooks/on_user_prompt_submit.rs`
   ~line 82), so the echo can **never** match. The mismatch arm logs
   "UserPromptSubmit does not echo the outstanding send", treats the prompt
   as external input, and the turn machine "converges on the safest
   outcome": it returns the send to `queued`
   (`backend/crates/domain/delta-usecase/src/turn.rs` ~line 126,
   `orphaned=Some(Requeue(..))`).
4. The turn completes, the session goes idle, and
   `backend/crates/domain/delta-usecase/src/interactor/enqueue/dispatch_queued.rs`
   re-types the same send. Back to step 2, forever. Each iteration burns a
   full model turn.

This is the send⇄echo text-match-dependence failure class already seen in
the slash-command incidents, but a **worse variant**: not a stuck turn but
an unbounded automatic re-send that only a human can stop. Fix it in two
independent layers:

**A. Requeue budget (the safety net).** A send returned to `queued` by the
echo-mismatch path may be re-dispatched **at most once**. On the second
mismatch of the same send, do not requeue again: park the send in a
terminal, user-visible state (reuse the existing send statuses if one fits,
e.g. `cancelled` with a surfaced reason — do not invent silent state) and
broadcast a `SessionEvent` so the browser shows *why* the message stopped
(the unknown-slash-command notice from the parser is the precedent for
surfacing a delivery problem instead of hanging). A runtime (in-memory)
counter keyed by send id is sufficient; it resets on server restart, which
merely grants one extra requeue before the loop stops — document that
trade-off where the counter lives. This net must catch **every** future
mismatch cause, not just attachments.

**B. Attachment-aware echo matching (the direct cause).** Make the
correlation recognize an attachment send's echo. Observed shapes: send
text = `<body>\n<absolute path with shell-escaped spaces>`; echoed prompt =
`[Image #2]<body>`. Match by comparing the send text with attachment-path
lines removed against the prompt with leading `[Image #N]` placeholder(s)
removed (support multiple attachments). Investigate the actual placeholder
grammar Claude Code emits before hardening the pattern, and keep the
comparison conservative: when in doubt, fall back to non-match — layer A
now bounds the damage. Plain-text sends must keep the existing exact-match
semantics unchanged, including the bare-command-name special case for
slash commands.

Out of scope: the structural replacement of text-based attribution (the
planned unstick/attribution redesign). Layer A is deliberately a narrow
forerunner of that work, not its implementation.

Operation × state coverage (echo-mismatch handling vs send state):

- First mismatch for a send → requeued once and re-dispatched on idle
  (today's behavior, preserved).
- Second mismatch for the same send → NOT requeued; parked visibly; the
  external prompt that arrived is still handled as external input exactly
  as today.
- Image-attachment send whose echo arrives as `[Image #N]<body>` → matched
  (layer B), turn attributed to the send's thread.
- Plain-text send → byte-identical behavior to today (exact match, slash
  command special case intact).
- Send cancelled by the user while queued → existing cancel path
  unaffected.
- Server restart between requeues → counter resets; the loop still
  terminates after at most one further requeue (documented).
- Harness task-notification prompts (`is_task_notification`) → still never
  surfaced as external input, regardless of the outstanding send's state.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Regression test reproducing the incident: a dispatched send whose
      echoes never match is re-dispatched at most once — the test drives
      two consecutive mismatched `UserPromptSubmit`s plus idle transitions
      and asserts no third dispatch of the same text occurs (red before the
      fix, green after).
- [x] When the requeue budget is exhausted, the send stops participating in
      dispatch AND the outcome is observable: a test asserts the parked
      status and the broadcast `SessionEvent` (never a silent drop).
- [x] Echo matching accepts the observed attachment shapes: unit tests
      cover body+path send text vs `[Image #2]`-prefixed echo, and a
      multi-attachment variant; paths in fixtures use the repo's
      established fictitious `/home/dev/...` convention.
- [x] Plain-text correlation is unchanged: existing send/echo and
      slash-command (bare command name) tests pass unmodified.
- [x] No sqlite schema change (`SCHEMA_VERSION` unchanged) and no change to
      the send table's status CHECK set unless a criterion above forces
      one — if it does, the migration is additive and covered by a
      back-compat test.
- [x] `make e2e-fake` passes unchanged (Claude pipeline byte-identical for
      plain-text sends).

### Manual / on-hardware (verified by a human before merge)

- [ ] Against a real Claude session driven by Delta: send a message with an
      image attachment from the composer — it is delivered once, answered
      once, the pending chip clears (send reaches `matched`), and no
      re-send occurs across subsequent idle periods.
- [ ] Force an unmatchable send (e.g. temporarily mangle the text after
      dispatch): the loop stops after one requeue and the browser surfaces
      the parked-send notice.
