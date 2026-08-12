# Control plane (`/hooks/*`)

## Overview

Claude Code fires these hooks even inside an interactive tmux session. Delta
uses them to register the session, correlate sends, and broadcast events. All
return **200 OK** on success and **500** with a plain-text reason on failure.
Shared conventions are in [README.md](README.md).

## `POST /hooks/user-prompt-submit`

Fires just before a prompt is processed. The first such hook registers the
session. The prompt is matched against the head of the pending-send FIFO: on a
hit the send is marked matched and a `turn_started` event is broadcast; on a
miss it is treated as `external_input`.

Request:

```json
{
  "prompt": "what is a delta?",
  "session_id": "sess-1",
  "transcript_path": "/path/to/transcript.jsonl",
  "cwd": "/work/delta"
}
```

Response (200):

```json
{
  "hookSpecificOutput": {
    "hookEventName": "UserPromptSubmit",
    "additionalContext": "The user is replying to this passage they selected from earlier in the conversation:\n\"the main channel\""
  }
}
```

Claude Code consumes injected context for `UserPromptSubmit` only from the
`hookSpecificOutput` envelope (a flat `additionalContext` is ignored), so the
framed quote is always wrapped there. The matched send's `locator_quote` is not
injected verbatim: it is wrapped in a short, authorship-neutral frame (shown
above) so the model treats it as provenance for the current message rather than
new content. This body is returned only when the matched send carried a non-empty
`locator_quote`, and it is injected into this prompt only. A blank or
whitespace-only quote is not framed, so the response is an empty `200 OK` with no
body.

## `POST /hooks/stop`

Fires when a response completes. Delta ingests any final transcript lines and
broadcasts `turn_completed`. Claude Code may flush the last assistant line to the
JSONL just after this hook fires; the background transcript tail (see
`transcript_updated` in [live-channels.md](live-channels.md)) catches those late
lines and refetches them, so the reply still appears without waiting for the
next hook.

Request:

```json
{
  "session_id": "sess-1",
  "stop_reason": null
}
```

`stop_reason` is optional. Any additional fields Claude Code sends (such as
`last_assistant_message`) are ignored. Response: **200 OK** with an empty body.

## `POST /hooks/pre-tool-use`

Fires when a tool permission prompt is imminent. Delta records the request and
broadcasts `permission_requested`. It never returns allow/deny — the TUI owns
that decision.

Request:

```json
{
  "session_id": "sess-1",
  "tool_name": "Bash",
  "tool_input": { "command": "ls" }
}
```

`tool_input` is an arbitrary JSON object. Response: **200 OK** with an empty
body.
