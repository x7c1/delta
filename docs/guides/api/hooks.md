# Control plane (`/hooks/*`)

## Overview

Claude Code fires these hooks even inside an interactive tmux session, which is
what makes them Delta's control plane: they are how the server learns that a
session started, that a prompt was submitted, that a tool wants permission, and
that a turn ended. Delta points them at itself by rendering the session's
settings file with this server's hook URLs, so no `.claude/settings.json` in the
working directory is touched. The browser-facing routes that pair with these
hooks are in [sends.md](sends.md); conventions and error semantics are in
[README.md](README.md).

Every hook returns **200 OK** on success — with a body only where noted below —
and **500** with a plain-text reason on failure. Every field shown in a request
example below is required unless the text calls it optional; `tool_input` is
the one exception, reading as `null` when absent. A payload missing a required
field never reaches the handler, because the framework rejects it first (the
`400`/`415`/`422` cases in [README.md](README.md), with a plain-text body).

Two payload fields recur and mean the same thing throughout:

- `session_id` is Claude Code's session id, which Delta uses as its own session
  key.
- `transcript_path` is the JSONL the hook is firing against. For work happening
  inside a nested subagent this is the *subagent's* transcript, not the parent
  session's, even though `session_id` still names the parent — Claude Code
  dispatches every hook under the parent's id. Delta compares the path against
  the session row's stored one to filter out hooks that belong to a conversation
  it does not track.

## Session lifecycle

### `POST /hooks/session-start`

Fires when a session's TUI is ready to accept input — the launch-readiness
signal. Behavior is gated on `source`:

- `"startup"` — binds and registers a matching fresh spawn (even a
  prompt-less one), so a session gets a row without waiting for its first
  prompt.
- `"resume"` — marks the resumed session ready, but does not dispatch its held
  first prompt from inside this hook: the hook blocks Claude Code until it
  returns, so a keystroke typed now would land while Claude Code is not yet
  accepting input and be silently lost. The held prompt is dispatched a beat
  later, once a background tick observes the session is ready.
- `"compact"` — fires mid-session once an auto- or manual `/compact` finishes.
  Not a no-op: the compaction may have swallowed a prompt typed at the same
  moment, so any send still `dispatched` behind that swallowed echo is
  re-typed into the pane.
- `"clear"` — fires mid-session when the user deliberately wipes the context.
  A genuine no-op: a clear is an intentional reset, so outstanding sends are
  left alone rather than resurrected.

Request:

```json
{
  "session_id": "sess-1",
  "source": "startup",
  "cwd": "/work/delta",
  "transcript_path": "/path/to/transcript.jsonl"
}
```

A bind typically broadcasts `session_registered`.

### `POST /hooks/session-end`

Fires when a session terminates. Delta uses it as a precise early failure
signal: if the ending session is a fresh spawn that never bound, the launch
failed before it could register, so the session is removed and `spawn_failed` is
broadcast. An already-bound session ending is a normal end and changes nothing.

Request:

```json
{ "session_id": "sess-1", "reason": "exit" }
```

`reason` is optional and carried for observability only.

## Prompts and turns

### `POST /hooks/user-prompt-submit`

Fires just before a prompt is processed. The first such hook registers the
session if `SessionStart` has not already. The prompt is matched against the head
of the pending-send FIFO: on a hit the send is marked matched and a
`turn_started` event is broadcast; on a miss it is treated as `external_input`.

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

### `POST /hooks/stop`

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
`last_assistant_message`) are ignored.

### `POST /hooks/message-display`

Fires repeatedly while an assistant message is being generated — before the
transcript JSONL is flushed and before any blocking tool prompt appears. Each
fire carries one chunk of the visible assistant text, and Delta buffers it as a
provisional live preview of the in-flight turn, broadcasting `assistant_streaming`
to the browser. The hook is deliberately passive (an empty 200), so it never
mutates the TUI.

Request:

```json
{
  "session_id": "sess-1",
  "message_id": "msg-1",
  "index": 0,
  "final": false,
  "delta": "Let me look at",
  "turn_id": "turn-1"
}
```

- `message_id` is stable across one message's fires; `index` increases 0, 1, 2, …
  within it; `final` is `true` only on the last chunk. Chunks are per display
  segment (a line or paragraph), not per token.
- `final` and `turn_id` are optional.
- The ids here are the hook's own and match no persisted transcript id, so a live
  chunk cannot be id-joined to the message that is eventually persisted — it is
  reconciled per turn instead.

## Tools and permissions

### `POST /hooks/pre-tool-use`

Fires for every tool call, before it runs. Delta records the request — the
payload carries the `tool_use_id` needed to resolve the resulting notice later —
and, for an `Agent`/`Task` call, syncs the transcript immediately so the
browser's running indicator lights up without waiting for the next ambient sync.
That parent-transcript ingest — not this hook — is what broadcasts
`subagent_started`, which is how a forked skill (a slash command's background
skill) lights the indicator too: its launch is no tool call, so it fires no
`PreToolUse` for the browser to learn about. This hook never returns allow/deny:
the TUI owns that decision, and the browser is notified by
[`POST /hooks/permission-request`](#post-hookspermission-request) instead — which
fires only when a dialog actually appears.

Request:

```json
{
  "session_id": "sess-1",
  "tool_name": "Bash",
  "tool_input": { "command": "ls" },
  "tool_use_id": "toolu_0166",
  "transcript_path": "/path/to/transcript.jsonl"
}
```

`tool_input` is an arbitrary JSON object. `tool_use_id` is the exact key Claude
Code later writes as `tool_use_id` on the matching `tool_result` transcript line,
which is how the recorded request is correlated with its completion and the
notice auto-cleared.

### `POST /hooks/post-tool-use`

Fires when a tool call completes, carrying the same `tool_use_id` its
`PreToolUse` carried. Delta acts on it only for the subagent case: a FOREGROUND
subagent's running window is closed and `subagent_finished` broadcast, while a
background launch — whose call returns immediately, so this fires long before the
work ends — instead has the `agentId` from its `tool_response` recorded as the
fallback correlation key its completion notification is matched by. Every other
tool's `PostToolUse` changes no runtime state.

Request:

```json
{
  "session_id": "sess-1",
  "tool_name": "Task",
  "tool_use_id": "toolu_0166",
  "tool_response": { "agentId": "agent-1" },
  "transcript_path": "/path/to/transcript.jsonl"
}
```

`tool_response` is the structured tool result and is optional — `null` and an
empty object are both valid "nothing useful here" shapes. For a background
`Agent` launch it carries the background task identifier (`agentId`), which Delta
records as a fallback correlation key.

### `POST /hooks/permission-request`

Fires only when an interactive permission dialog actually appears, so a human
answer is genuinely pending. Delta records the request row, registers a decision
waiter, and broadcasts `permission_requested` *before* blocking, so the browser
can show the Allow/Deny notice it is being asked to answer. Unlike `PreToolUse`
this payload carries no `tool_use_id`, so the row is recorded without one. A
dialog blocks the whole session until it is answered, so at most one such row
is ever pending; the next `tool_result` to arrive in this session settles it.

Request:

```json
{
  "session_id": "sess-1",
  "tool_name": "Bash",
  "tool_input": { "command": "rm -rf scratch" },
  "transcript_path": "/path/to/transcript.jsonl"
}
```

The response then blocks Claude Code until one of two things happens:

- **The browser decides** (via
  [`POST /api/permissions/{id}/decision`](sends.md#post-apipermissionsiddecision)),
  and the decision is returned in the only envelope Claude Code reads it from:

  ```json
  {
    "hookSpecificOutput": {
      "hookEventName": "PermissionRequest",
      "decision": { "behavior": "allow" }
    }
  }
  ```

  `behavior` is `allow` or `deny`, and the tool proceeds or is denied without the
  TUI prompt appearing.

- **The deadline passes** — 50 seconds, kept under Claude Code's own 60-second
  hook timeout so the fallback is Delta's passthrough rather than Claude
  abandoning the hook mid-wait. The waiter is discarded and the response is an
  empty **200** — there is no decision to report — so the tool call falls back
  to the interactive TUI prompt exactly as it would without this hook. The
  request row stays pending and the eventual `tool_result` resolves it; a
  browser decision arriving after this point is a `409`
  (`permission_not_pending`).

## Status line

### `POST /hooks/status-line`

Not a hook: this is the `statusLine` command Delta injects into the session
settings, which Claude Code invokes on every status-line refresh and pipes this
JSON to on stdin; the command `curl`s it back here, which is why it posts the
same way the hooks do.

The payload is the only source for the data it carries — the selected model,
context-window usage, rate limits and cost are nowhere in the transcript JSONL.
It is a "latest value" snapshot keyed by `session_id`, not an append: it mutates
no server state, so the handler broadcasts a `status_updated` event directly.

Request:

```json
{
  "session_id": "sess-1",
  "model": { "id": "claude-sonnet-4", "display_name": "Sonnet 4" },
  "context_window": {
    "used_percentage": 32.5,
    "context_window_size": 200000,
    "current_usage": {
      "input_tokens": 1200,
      "output_tokens": 400,
      "cache_creation_input_tokens": 0,
      "cache_read_input_tokens": 63000
    },
    "total_input_tokens": 64200
  },
  "rate_limits": {
    "five_hour": { "used_percentage": 12.0, "resets_at": 1767258600 },
    "seven_day": { "used_percentage": 40.0, "resets_at": 1767758600 }
  },
  "cost": { "total_cost_usd": 0.42 },
  "workspace": { "current_dir": "/work/delta" }
}
```

Every field is optional, and unknown fields are ignored so the payload stays
forward-compatible across Claude Code versions. In particular, before a
session's first API response `rate_limits` is absent entirely and
`context_window.used_percentage` / `current_usage` are `null`; `rate_limits` is
also absent on accounts without a Pro/Max subscription. A payload with no
`session_id` has nothing to key on and is dropped.

`used_percentage` is computed by Claude Code against the matching window size and
is forwarded verbatim — Delta never re-derives it from the token counts beside
it. The named windows are projected onto durations (`five_hour` → 5 hours,
`seven_day` → 7 days) at this edge, so nothing downstream knows Claude's window
names. `resets_at` is Unix epoch seconds.

Response: **200 OK** with an empty body.
