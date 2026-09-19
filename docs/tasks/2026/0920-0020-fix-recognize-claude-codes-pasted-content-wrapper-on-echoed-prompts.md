---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -rq '</pasted_content id=' backend/crates/domain/delta-attribution/src/ && grep -rq 'pasted_content' backend/crates/domain/delta-usecase/src/"
assignee: null
branch: task/0920-0020-fix-recognize-claude-codes-pasted-content-wrapper-on-echoed-prompts
created_at: 2026-09-19T15:20:12Z
updated_at: 2026-09-19T16:03:16Z
---

# fix(attribution): recognize Claude Code's pasted-content wrapper on echoed prompts

## Overview

Delta types every send into the Claude Code pane as an xterm bracketed paste
(`backend/crates/gateway/tmux-driver/src/tmux/commands.rs`, `input_commands`),
so embedded newlines survive. Recent Claude Code builds (observed on 2.1.277
and 2.1.278, behind a server-side feature flag that is currently on) now wrap
pasted text whose trimmed length is 20 characters or more in a tag pair before
submitting it. Both the `UserPromptSubmit` hook's `prompt` and the transcript's
human line carry the wrapped form. Observed verbatim (Rust string literal):

```
"\n\n<pasted_content id=\"d626\">\nこれって今どこまで進んでますか\nそれともこれから開始するところですか\n</pasted_content id=\"d626\">\n"
```

for a send whose text was
`"これって今どこまで進んでますか\nそれともこれから開始するところですか"`. A
single-line send of 20+ characters is wrapped the same way:

```
"\n\n<pasted_content id=\"d626\">\n実装に着手してほしいのですが、その前に認証をこちらで済ませておかないといけない、という理解であっていますか\n</pasted_content id=\"d626\">\n"
```

Shape of the wrapper, as far as the upstream build shows: the opening tag
`<pasted_content id="XXXX">` and the closing tag `</pasted_content id="XXXX">`
carry the same id, which is 4 lowercase hex characters (a per-session value).
The opening tag is followed by a newline. A newline goes before the closing
tag unless the body already ends with one. The block is preceded by a newline.
Sends shorter than 20 trimmed characters are not wrapped.

Consequences today:

1. `claude_format::prompt_echoes_send`
   (`backend/crates/domain/delta-attribution/src/claude_format/mod.rs`) compares
   trimmed text for equality. It answers `false` for every wrapped echo. The
   `UserPromptSubmit` path (`interactor/hooks/on_user_prompt_submit.rs`) keys the
   locator-quote `additionalContext` on that verdict. So **a branch send made by
   quoting a passage loses its quote frame whenever the body is 20+ characters**:
   the model never learns which passage the user quoted. The server logs
   "UserPromptSubmit does not equal the outstanding send's text" and the
   transcript ingest warns "the transcript line consuming this send does not spell
   the send's own text" for every such send. The `attributed` flag on
   `Effect::SendMatched` is `false` for the same reason.
2. The conversation pane shows the human message with the raw
   `<pasted_content id="…">` tags around the user's text. The Claude Code TUI
   itself never shows the tags or the id.

Fix: treat the wrapper as transport, not content.

- Teach the echo comparison to recognize a wrapped echo. A prompt that is
  exactly one well-formed wrapper block (matching open/close ids, 4 lowercase
  hex, surrounding whitespace ignored) whose body equals the send's text after
  trimming echoes that send. Keep the comparison as conservative as the
  existing attachment rule. Any malformed or partial wrapper stays a mismatch.
  Combine the wrapper rule with the existing `[Image #N]` attachment rule
  where Claude Code could produce both on one prompt, or state in the doc
  comment why they cannot co-occur.
- Unwrap well-formed wrapper blocks when deriving a human message's displayed
  and stored text from a transcript user line, so the conversation pane shows
  what the user wrote. A block may sit next to text the user typed straight
  into the pane. Keep that typed text and remove only the tags and the
  newlines the wrapper added. Leave a malformed block as it is.
- Update the doc comments that describe the comparison and its rewrite
  catalogue (`prompt_echoes_send`, `Effect::SendMatched::attributed`, the
  comment block above the `attributed` computation in
  `on_user_prompt_submit.rs`) so they name the wrapper as a recognized rewrite.

Put the wrapper parsing in one place in `delta-attribution` and call it from
both paths. Do not add a second copy in `delta-usecase`.

Out of scope:

- Rows already stored with the wrapped text. No migration or backfill.
- Reversing Claude Code's in-body escaping. Claude Code rewrites a tag-like
  `<pasted_content` inside the pasted body to `<\pasted_content`, so a send
  whose own text contains that string still compares as a mismatch. That is
  the safe answer.
- Teaching `fake-claude` to emit the wrapper. It depends on an upstream
  feature flag and is tracked separately.
- Changing how Delta types sends. Bracketed paste stays.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `prompt_echoes_send` returns `true` for each of the two observed
      wrapped prompts in the Overview against their send texts. It returns
      `false` for: mismatched open/close ids, a non-hex or wrong-length id,
      a missing closing tag, a wrapped body that differs from the send, and
      a wrapped block followed by extra typed text. Each case is covered by a
      unit test in `delta-attribution`, and the parsing code names the
      closing-tag form (the `</pasted_content id=` grep gate in
      `check_command`).
- [x] An attribution-fold test shows that a wrapped human line consuming an
      outstanding send yields `Effect::SendMatched { attributed: true, .. }`,
      and that the message's text is the unwrapped body with no
      `pasted_content` tag.
- [x] A fold test covers a transcript human line that has typed text next to
      a wrapped block. The stored message text keeps the typed text and the
      pasted body, and drops the tags.
- [x] A `delta-usecase` hook test shows that a branch send with a locator
      quote, echoed by `UserPromptSubmit` in wrapped form, gets the
      locator-quote `additionalContext` injected, as an unwrapped echo does.
      The `pasted_content` grep gate over `delta-usecase/src/` in
      `check_command` confirms the test exists.
- [x] The existing tests for trimmed exact match and the image-attachment
      rewrite still pass unchanged (`make check`).

### Manual / on-hardware (verified by a human before merge)

- [ ] On a live Delta with Claude Code 2.1.277 or later, a quote-to-branch send
      of 20+ characters gets its quote frame injected
      (`UserPromptSubmit: additionalContext returned to Claude Code
      injected=true` in the server log), and neither rewrite warning appears.
- [ ] The conversation pane shows that send without `<pasted_content>` tags.
