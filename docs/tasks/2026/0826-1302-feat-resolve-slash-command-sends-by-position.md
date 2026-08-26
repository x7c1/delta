---
status: completed
pipeline_phase: null
plan: null
base_ref: feat/attribution-split
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && A=backend/crates/domain/delta-attribution && grep -q "fn is_slash_command_send" $A/src/claude_format/mod.rs && grep -q "is_slash_command_send" $A/src/attribute/thread_resolution.rs && grep -q "local_command_name_line_matches_send" $A/src/attribute/thread_resolution.rs'
assignee: null
branch: task/0826-1302-feat-resolve-slash-command-sends-by-position
created_at: 2026-08-26T13:02:22Z
updated_at: 2026-08-26T13:29:40Z
---

# feat(attribution): resolve a slash-command send from its command line by position, not by name

## Overview

A send that is a slash command (`/review-pr`, `/foo:bar 123`, a typo such as
`/revew-pr`) produces no `UserPromptSubmit` echo and no `Stop`: Claude Code
handles it client-side and records either a local-command group (whose first
line is the bare command name) or an unknown-command notice. Those transcript
lines are therefore the only signal Delta has that the dispatched send was
consumed, and `backend/crates/domain/delta-attribution/src/attribute/thread_resolution.rs`
resolves them by **command name**: the local-command branch compares bare
command names (`claude_format::local_command_name_line_matches_send`,
namespace-tolerant), the unknown-command branch compares the notice's command
with the send's first whitespace-delimited token. Only on a match does it pop
the head outstanding send, emit `Effect::SendMatched` and
`Effect::LocalCommandTurnEnded`; otherwise the send stays outstanding and the
queue waits for the echo deadline to requeue it — one more delivery of the same
command.

The two previous changes on this branch made the ordinary prompt path
positional: a human user line consumes the head outstanding send whatever its
text, and the text comparison only reports whether the echo was verbatim.
This task brings the two command branches to the same rule, with one guard the
prompt path does not need. For a prompt line, the `UserPromptSubmit` hook has
already consumed the send positionally, so the transcript line can only agree.
For a command line there is no hook: if the head outstanding send is a plain
prompt (`hello`) and a `/help` command line shows up, the send was **not**
what got submitted (something pre-empted it in the pane), and consuming it
would lose the message. So the positional rule applies when the outstanding
send is itself a slash command — then whatever command line Claude recorded
is that send's outcome, however Claude rewrote the name — and a plain-prompt
send is left alone exactly as today.

### What changes

1. **`claude_format::is_slash_command_send(text: &str) -> bool`** (new, in
   `claude_format/mod.rs`, with a unit test next to the existing
   `bare_command_name_*` tests): true when the trimmed text's first
   whitespace-delimited token starts with `/`. Keep `bare_command_name` and
   `local_command_name_line_matches_send`; they now feed the `attributed`
   flag instead of gating consumption.
2. **`thread_resolution.rs`, local-command branch.** Pop the head outstanding
   send when `is_slash_command_send(&send.text)`, regardless of whether the
   bare names agree; emit `SendMatched { attributed: local_command_name_line_matches_send(&send.text, trimmed) }`
   and `LocalCommandTurnEnded` as today. When the head send is not a slash
   command, or there is none, leave everything as it is (fold as `Meta`,
   inherit `carry_thread`). Rewrite the branch comment: the namespace story
   becomes an example of why the name is not trusted, not the rule.
3. **`thread_resolution.rs`, unknown-command branch.** Same rule:
   `is_slash_command_send` gates consumption; `attributed` is the old
   first-token comparison (`send.text.split_whitespace().next() == Some(command)`).
4. **`sync_transcript.rs`** already warns on `attributed == false`; make sure
   the warning text does not assume a human prompt line (it should read
   correctly for a command line too — adjust the wording minimally if it
   does not).
5. **Tests.** In `tests/fold.rs`: keep
   `a_namespaced_local_command_name_line_matches_a_short_form_send` and
   `an_unknown_command_notice_matches_a_send_carrying_args` (they now assert
   `attributed: true`); add a local-command test where the recorded bare name
   differs from the send's (`/review-pr` sent, `/example:audit` recorded)
   and the send is still consumed with `attributed: false` and the turn
   ended; add the unknown-notice analogue (`/review-pr 123` sent, notice for
   `/revew-pr`); add one test per branch that a **plain-prompt** outstanding
   send is left outstanding by a command line (no `SendMatched`, no
   `LocalCommandTurnEnded`, carry inherited). Regenerate corpus goldens with
   `UPDATE_GOLDEN=1 cargo test -p delta-attribution --test corpus`; no
   assignment is expected to change (`local_command_no_turn` is the case to
   watch) — if one does, explain it in the PR body or treat it as a bug. The
   usecase tests `interactor/sync/tests/local_command_unsticks_turn_and_folds_to_meta`
   and `unknown_command_unsticks_turn` must stay green as they are.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `claude_format::is_slash_command_send` exists and gates consumption in
      both command branches of `thread_resolution.rs`, while
      `local_command_name_line_matches_send` is still called there (only for
      the `attributed` flag) — gates appended to `check_command`.
- [x] A local-command name line whose bare name differs from the outstanding
      slash-command send still consumes it (`SendMatched { attributed: false }`
      + `LocalCommandTurnEnded`) — pinned by a new fold test; the unknown-notice
      analogue likewise.
- [x] A command line never consumes a plain-prompt outstanding send — pinned
      by one fold test per branch.
- [x] Every corpus case replays to its golden and `tests/replay_properties.rs`
      is green after regeneration.
- [x] `make check` is green (backend fmt / build / test / clippy `-D warnings`,
      generated-bindings freshness, frontend, both Playwright suites).

### Manual / on-hardware (verified by a human before merge)

- [x] On a real Claude Code session, send an unknown slash command from Delta
      and type extra characters into the tmux pane in the 250 ms gap between
      Delta's paste and its Enter (a script polling `tmux capture-pane` for the
      pasted text), so Claude records an unknown-command notice for a name that
      differs from the send. The send clears from the open list without the
      echo deadline firing, nothing is re-typed, and the server logs one
      `attributed=false` warning. Verified 2026-08-26 with `/<word>` rewritten
      to `/<word>zzz`: the notice line was attributed to the send's thread, the
      row was `matched` with the notice's uuid within the same second, and the
      "does not spell the send's own text" warning was logged once. (A built-in
      such as `/help` is not usable for this check: it opens a TUI dialog and
      records nothing in the transcript.) Observed alongside, pre-existing and
      not caused by this change: the local-command turn end is fed to the turn
      machine as `Stop` from `AwaitingEcho`, which logs an "anomalous
      transition" warning and a no-op requeue — a follow-up on this branch.

## Out of scope

- The human-prompt branch (already positional), the turn machine, the Codex
  adapter, the frontend.
- A browser-facing notice for an unattributed send, and merging
  `TurnInput::EchoMatched` / `ExternalPrompt` (follow-ups on this branch).
