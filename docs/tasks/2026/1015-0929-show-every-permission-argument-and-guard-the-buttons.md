---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1015-0929-show-every-permission-argument-and-guard-the-buttons
created_at: 2026-10-15T00:29:25Z
updated_at: 2026-10-15T01:40:26Z
---

# feat(permissions): show every argument of a permission request as it is, and guard the buttons when the next request takes its place

## Overview

The permission card (`frontend/packages/apps/web/src/features/transcript/PermissionNotice.tsx`)
is where the user decides whether an agent may run a tool. Make the card
show every argument of the request as it is, give Claude's file tools the
same change view Codex file changes get, and keep a quick second click from
answering a request the user has not seen.

### What the card shows

1. **All arguments.** Render every field of the tool input, each labelled,
   in a `pre-wrap` block that preserves whitespace and line breaks. Long values
   are shown expanded up to a generous limit; beyond it, show the head and the
   tail with the elided count in between, never just the head. Leading or
   trailing runs of whitespace or blank lines longer than a few characters are
   made visible (for example as a dimmed marker with the count), so nothing
   can be pushed out of view by padding.
2. **Claude Write / Edit / MultiEdit / NotebookEdit.** Give the card the same
   change view Codex file changes already get: Write shows the new content;
   Edit and MultiEdit show each `old_string` → `new_string` as a diff. Build
   the `file_change` from the hook's `tool_input` on the backend, reusing the
   existing wire type.
3. **Codex exec** shows its `cwd`.
4. **Invisible and direction-changing characters** (Unicode bidi controls
   U+202A–U+202E, U+2066–U+2069, zero-width characters U+200B–U+200D, U+2060,
   U+FEFF, and other C0/C1 controls except tab and newline) are rendered as a
   visible escape (`\u{202E}`) with a warning line on the card, everywhere the
   card shows request text, including the Codex heading that uses the command
   as the tool name.

### Guarding the buttons

5. When the card switches to a different request, its buttons stay disabled
   for 700 ms and the card visibly marks the change (for example a brief
   highlight and the text "New request"). The keyboard shortcuts, if any, obey
   the same delay.

### Timeouts and answers

6. `backend/crates/libs/delta-bootstrap/src/settings.rs` gives every HTTP hook
   `"timeout": 30`, shorter than Delta's 50-second wait for a permission
   decision (`launch_config.rs`). Give the `PermissionRequest` hook a timeout
   longer than that wait (for example 70 seconds) and keep the others as they
   are. Make the server abandon the pending decision when the hook request is
   dropped (a drop guard), so a disconnected wait does not linger.
7. `backend/crates/domain/delta-usecase/src/interactor/answer_question.rs`:
   refuse an AskUserQuestion answer while a permission request is pending for
   that session, and mark the question answered when the first key is sent so
   a repeated POST is refused.

Out of scope: separating the tmux server per session.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Frontend tests: a request with several fields shows each labelled; a
      value with long leading padding shows its tail and a visible padding
      marker; a value over the limit shows head, elided count
      and tail; a value containing U+202E shows `\u{202E}` and the warning.
- [x] Frontend tests: after the card switches to a new request, the buttons
      are disabled and become enabled after 700 ms (fake timers); a click in
      that window does nothing.
- [x] Backend tests: a Claude Write / Edit / MultiEdit permission request
      yields a `file_change` with the content or the old/new pairs.
- [x] A test asserts the generated session settings give `PermissionRequest`
      a timeout above the decision wait and the other hooks 30.
- [x] A test asserts a dropped permission-hook request abandons the pending
      decision.
- [x] Tests assert an AskUserQuestion answer is refused while a permission
      request is pending, and a second answer to the same question is refused.
- [x] `make check` passes.

### Before merge (verified outside the check command)

- [x] In the browser against `make dev` with a real Claude session, a Bash
      permission request and an Edit permission request show their full
      arguments and the edit diff on the card.
