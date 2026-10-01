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
branch: task/1001-2053-fix-follow-a-sessions-transcript-when-claude-code-relocates-it
created_at: 2026-10-01T11:53:11Z
updated_at: 2026-10-01T12:41:55Z
---

# fix(session): follow a session's transcript when Claude Code relocates it

## Overview

When a Claude Code session enters a worktree (`EnterWorktree`), Claude Code
2.1.286 moves the session's JSONL transcript to the project directory of the
new cwd. The file is moved, not copied. It ends up under
`~/.claude/projects/<munged new cwd>/<session id>.jsonl` and contains a
`{"type":"relocated","relocatedCwd":...}` line. Every later hook POST then
reports the new `transcript_path`.

Delta keeps following the old path, so nothing after the move is ingested: no
new messages, no turn-end metadata (the per-reply footer with branch, date and
model never shows), and the session's `last_activity_at` freezes. This was
observed live: the old file no longer existed, while the session's DB row kept
the old `transcript_path` and its last message was the tool_result just before
the relocation.

Three things combine to make this permanent:

- The session upsert in `backend/crates/gateway/delta-sqlite/src/store/sessions.rs`
  only updates `transcript_path` while the row is `spawning`.
- `is_foreign_transcript` in `hook_transcript_guard.rs` (delta-usecase hooks)
  treats any hook whose `transcript_path` differs from the stored one as a
  nested subagent's hook. After a relocation, the session's own
  PreToolUse / PostToolUse / PermissionRequest hooks are therefore dropped
  (`on_pre_tool_use.rs`, `on_post_tool_use.rs`, `on_permission_request.rs`).
- Resume refuses the session because the stored transcript no longer exists
  (`lifecycle/open_session.rs`).

Fix:

1. **Recognise a relocation.** A hook for the session reports a
   `transcript_path` that differs from the stored one and is not a subagent
   transcript. Subagent transcripts live under
   `<session dir>/subagents/`; check the real layout and base the test on
   that, not on the paths simply differing. Validate the new path the same way
   hook paths are validated today (`validate_transcript_path`), then persist it
   as the session's `transcript_path` through a store method that works in any
   status, and switch the transcript follower/tail to the new file. Log the
   relocation at info with both paths.
2. **Keep the line cursor.** The new file is the old file moved whole, with
   the `relocated` line and later lines appended, so the per-session line
   cursor should carry over. Verify this against the real file shape and
   cover it with a test. If the cursor cannot be trusted (for example, the new
   file is shorter than the cursor), fall back to a safe re-sync that does not
   duplicate messages, and say how.
3. **Fix the subagent check.** Base `is_foreign_transcript` on the subagent
   location rather than on "differs from the stored path", so a relocated
   session's own hooks are handled again.
4. **Parse the new line kind.** Make sure the `relocated` line (and any other
   non-message line kinds seen next to it, such as `worktree-state` and
   `pr-link`) is parsed as a non-message and does not break ingestion. If
   `relocatedCwd` is useful, update the session's `cwd` too. Check what the UI
   shows for cwd/branch.
5. **Self-heal live sessions already stuck this way.** The first hook that
   arrives for such a session after this change re-points it, so a live session
   needs no migration. A stuck session that was already closed cannot heal this
   way (resume refuses it before any hook arrives); that is out of scope here and
   handled separately.
6. **fake-claude.** Add a scenario step that relocates the transcript (move
   the file to a new project dir, append a `relocated` line, report the new
   path in subsequent hooks). Add a fake-lane e2e spec in which a reply written
   after the relocation appears in the browser with its turn-end footer.

Out of scope: Claude-Code-originated `<task-notification>` prompts being
mistaken for the outstanding send. That was seen in the same session but did
no harm there; it is a separate fix.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A usecase test: a hook carrying a relocated (non-subagent)
      `transcript_path` for an active session updates the stored path, and
      later transcript lines from the new file are ingested without
      duplicates.
- [x] A usecase test: a subagent's hook (its transcript under the session's
      `subagents/` directory) is still treated as foreign and does not
      re-point the session.
- [x] A usecase test: after relocation, the session's own PreToolUse /
      PostToolUse hooks are processed, not dropped.
- [x] A store test: the new store method updates `transcript_path` for an
      `active` session.
- [x] A parser test: a `relocated` line is not ingested as a message.
- [x] A fake-lane e2e spec covers a relocation mid-session (runs under
      `make check`).

### Manual / on-hardware (verified by a human before merge)

- [ ] In a real Delta session, Claude Code entering a worktree keeps the
      conversation flowing in the browser, and replies keep their footer.
