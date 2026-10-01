---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -rq 'an isMeta caveat line parsed as Role::Meta' backend/crates/apps/delta-server/tests/"
assignee: null
branch: task/1001-0335-fix-free-a-local-slash-commands-turn-when-claude-records-nothing
created_at: 2026-09-30T18:35:00Z
updated_at: 2026-09-30T19:12:17Z
---

# fix(session): free a local slash command's turn when claude records nothing for it

## Overview

Claude Code 2.1.286 no longer records anything in the transcript for a local
slash command such as `/cost`, `/model` or `/exit`. Earlier versions wrote a
three-line group sharing one `promptId` — a `<local-command-caveat>` user line
flagged `isMeta`, the bare command-name line and a `<local-command-stdout>`
line — and Delta relies on that group to end the turn of a send that was a
local command. Verified against the real CLI: a session where only `/cost` and
`/exit` were run produces no transcript file at all, and a session with a
normal prompt followed by `/exit` has no caveat, command-name or stdout lines.
Claude still fires no `UserPromptSubmit` and no `Stop` for a local command, and
`SessionEnd` still fires on `/exit`.

**User-visible effect today.** When the user sends `/cost` from Delta's
composer, the turn machine goes `Idle → AwaitingEcho` (`turn.rs`, the
`delta-usecase` turn machine). The path that used to end it —
`consume_slash_command_send` in
`backend/crates/domain/delta-attribution/src/attribute/thread_resolution/mod.rs`
emitting `SendMatched` + `LocalCommandTurnEnded`, turned into
`TurnInput::CommandResolved` by `sync_transcript.rs` — never fires, because the
command-name line never appears. Only the echo-deadline watchdog is left:
after `ECHO_DEADLINE` (60 s,
`interactor/session_actor/runtime/turn.rs`) the machine returns to `Idle` with
`Requeue`, which presses Escape and **types the same command again**
(`MAX_REQUEUES_PER_SEND = 1`), and after a second silent minute the send is
parked. So the session shows "In progress" for about two minutes, the command
runs twice, and queued sends behind it wait the whole time.

**Fix.** Treat silence after a slash-command send as the command having run:

1. In the turn machine, when the outstanding send is a slash command (the same
   predicate the attribution side already uses, `is_slash_command_send` /
   the `claude_format` slash check), an `EchoDeadline` for it settles the send
   as delivered and returns to `Idle` without `Requeue` — no re-type and no
   park. Other sends keep today's requeue-then-park behaviour.
2. Give slash-command sends a much shorter echo deadline than
   `ECHO_DEADLINE` (for example 10 s): a command that is really a prompt (a
   skill or a custom command) echoes `UserPromptSubmit` within seconds, so the
   shorter wait only decides how long a local command keeps the session busy.
   Keep it overridable the way `DELTA_ECHO_DEADLINE_MS` is, so the fake suite
   can use a short value. A late `UserPromptSubmit` arriving after the settle
   is handled by the existing `Idle` + `PromptSubmitted` path; say so in a doc
   comment.
3. Keep the transcript-driven `CommandResolved` path unchanged, so transcripts
   written by older claude versions (and resumed sessions) still end the turn
   the old way. Update the doc comments that describe the three-line group as
   current behaviour (`turn.rs` `CommandResolved`,
   `thread_resolution/mod.rs`, `delta-transcript/src/parse/mod.rs` where it
   calls the `local_command` lines the "current shape", the
   `local_command_unsticks_turn_and_folds_to_meta` test doc, and the comment
   in the attribution corpus case `local_command_no_turn/overlay.json`) so
   they say it is what claude wrote up to 2.1.285.
4. **fake-claude.** Add a scenario step (for example `local_command`) that
   consumes one prompt from the pane without firing any hook and without
   writing the transcript — like `swallow_prompt`, but named for this case —
   and a fake-lane e2e spec in which sending a slash command frees the
   session (it leaves "In progress" and a queued follow-up is dispatched)
   without the command being typed a second time.
5. **Real-claude canary.** In
   `backend/crates/apps/delta-server/tests/real_claude_canary.rs`,
   `prompt_turn_fires_hooks_and_streams_the_transcript` currently waits for an
   `isMeta` caveat line after `/exit` and so times out on 2.1.286. Replace that
   wait with the new contract: after `/exit` and `SessionEnd`, the transcript
   contains no `<local-command-caveat>`, no command-name line and no
   `<local-command-stdout>` line. Update the module doc's list of what the
   canary pins, and in `docs/guides/development/canary.md` drop "isMeta
   flagging" from the recorded contract where it refers to this group and add
   an entry to "Drift already pinned" describing the change (the local-command
   group stopped being written in 2.1.286; Delta now frees the turn on a short
   echo deadline).

Out of scope: detecting the TUI's idle state from the pane, and enumerating
known local commands.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A turn-machine unit test: `AwaitingEcho` on a slash-command send +
      `EchoDeadline` for that send → `Idle` with the send settled as
      delivered, and no `Requeue`.
- [x] A turn-machine unit test: the same deadline on a non-slash send still
      yields `Requeue` (today's behaviour).
- [x] An interactor test: a slash-command send with no transcript line and no
      hook frees the session after the slash-command deadline, is typed into
      the pane exactly once, and lets the next queued send dispatch.
- [x] The existing old-transcript tests
      (`local_command_unsticks_turn_and_folds_to_meta`, the
      `local_command_no_turn` corpus case) still pass unchanged in their
      assertions.
- [x] A fake-lane e2e spec covers sending a slash command end to end (runs
      under `make check`).
- [x] The real-claude canary no longer waits for the `isMeta` caveat line
      (`! grep -rq 'an isMeta caveat line parsed as Role::Meta'
      backend/crates/apps/delta-server/tests/` in `check_command`).

### Manual / on-hardware (verified by a human before merge)

- [ ] `cargo test -p delta-server --test real_claude_canary -- --ignored
      --test-threads=1` passes against the real, authenticated claude 2.1.286.
- [ ] In Delta, sending `/cost` from the composer frees the session within
      the short deadline and the command runs once.
