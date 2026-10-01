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
branch: task/1001-2210-fix-find-a-relocated-transcript-when-resuming-a-session
created_at: 2026-10-01T13:10:28Z
updated_at: 2026-10-01T13:33:26Z
---

# fix(session): find a relocated transcript when resuming a session

## Overview

Delta now follows a session's transcript when Claude Code moves it on entering
a worktree, but only once a hook for that session arrives (see
`hooks/hook_transcript.rs`, `admit_hook_transcript` /
`follow_relocated_transcript`). A session whose transcript moved while Delta
was not following it (for example, it was closed, or Delta was down or still
running an older build) never gets that hook. Its stored `transcript_path`
points at a file that no longer exists, so the resume gate in
`lifecycle/open_session.rs` refuses it with `Error::ResumeUnavailable` before
any pane or hook exists. The session cannot be reopened, and its history after
the move is never ingested.

Fix: when the stored transcript is missing at resume time, look for the
session's moved transcript before refusing.

1. Add a lookup to the `Transcript` port (and its `delta-transcript`
   implementation and the test fake) that finds `<session id>.jsonl` directly
   under any project directory of the Claude Code transcript root (the same
   root `validate_transcript_path` confines hook paths to). Ignore files under
   `subagents/`. If more than one candidate exists, prefer the one whose
   latest line is newest, and log the choice.
2. In the resume gate, if the stored path is missing and the lookup finds a
   candidate, validate it like a hook path, then re-point the session through
   the same code path as `follow_relocated_transcript` (path, cwd from the
   last `relocated` line, and the cursor rule). Then continue the resume:
   `claude --resume` must run in the relocated cwd (the worktree), or Claude
   Code will look in the old project directory. If no candidate is found,
   refuse as today.
3. Log the rescue at info with both paths.

Out of scope: scanning all sessions at startup.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A usecase test: resuming a session whose stored transcript is missing,
      while `<id>.jsonl` exists in another project directory with a
      `relocated` line, re-points the session (path and cwd) and launches the
      resume in the relocated cwd.
- [x] A usecase test: with no candidate found, the resume is refused as
      before (`ResumeUnavailable`), with no pane spawned.
- [x] A transcript-adapter test: the lookup finds the moved file, ignores
      `subagents/` files, and picks the newest when there are several.
- [x] `make check` passes.

### Manual / on-hardware (verified by a human before merge)

- [ ] A real session that entered a worktree while Delta was not following it
      can be resumed from Delta, and its post-move history appears.
