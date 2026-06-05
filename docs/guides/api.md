# Delta server API

## Overview

The Delta server is a local process that wraps a single Claude Code session and
exposes it to a browser UI. It serves three kinds of traffic:

- **Browser REST surface** (`/api/*`) — request/response queries and commands
  the browser issues to hydrate state and enqueue sends.
- **Browser live channels** — a WebSocket event stream (`/ws`) carrying
  `SessionEvent`s, and a PTY bridge (`/pty`) attaching an xterm.js terminal to
  the tmux pane.
- **Control plane** (`/hooks/*`) — HTTP hooks Claude Code fires during a
  session. Delta correlates them with queued sends and broadcasts events.

The server binds to `127.0.0.1` only; it is never exposed on a public
interface. All request and response bodies are JSON unless noted otherwise.

This document is the source of truth for the browser↔server contract. Field
names below match the JSON exactly (the wire shapes are the serialized domain
types).

## Conventions

- All timestamps are ISO-8601 strings.
- `thread_id` is an integer issued by the server. `session_id`, message `uuid`,
  and `prompt_id` are strings.
- Errors carry a JSON body `{ "error": "<message>" }`. Status codes:
  - `400 Bad Request` — a malformed domain value in the request.
  - `404 Not Found` — no session registered yet, or an unknown thread.
  - `500 Internal Server Error` — a store, transcript, or tmux failure.

## Shared JSON shapes

### `Session`

```json
{
  "id": "sess-1",
  "cwd": "/work/delta",
  "transcript_path": "/path/to/transcript.jsonl",
  "title": null,
  "status": "active",
  "created_at": "2026-01-01T00:00:00Z"
}
```

`status` is one of `active`, `ended`.

### `Thread`

```json
{
  "id": 1,
  "session_id": "sess-1",
  "title": "main",
  "parent_thread_id": null,
  "root_message_uuid": null,
  "created_at": "2026-01-01T00:00:00Z"
}
```

The trunk thread has the title `main`, no parent, and no root message. Child
threads carry `parent_thread_id` and the `root_message_uuid` they branch from.

### `Message`

```json
{
  "uuid": "uuid-1",
  "session_id": "sess-1",
  "thread_id": 1,
  "role": "user",
  "linear_parent_uuid": null,
  "semantic_parent_uuid": null,
  "prompt_id": "prompt-1",
  "seq": 0,
  "content_text": "what is a delta?",
  "content": [{ "type": "text", "text": "what is a delta?" }],
  "created_at": "2026-01-01T00:00:00Z"
}
```

- `role` is one of `user`, `assistant`, `system`, `other`.
- `linear_parent_uuid` is the transcript's model-context parent;
  `semantic_parent_uuid` is the `to:` reply edge, set only on branch messages.
- `content` is an ordered list of content blocks (see below). `content_text` is
  the flattened plain-text view of the text/thinking blocks, or `null`.

### `ContentBlock`

A tagged union keyed on `type`:

```json
{ "type": "text", "text": "..." }
{ "type": "thinking", "thinking": "..." }
{ "type": "tool_use", "id": "t1", "name": "Bash", "input": { "command": "ls" } }
{ "type": "tool_result", "tool_use_id": "t1", "content": "...", "is_error": false }
```

Any unmodelled block kind is preserved as `{ "type": "other" }`.

### `PendingSend`

```json
{
  "id": 1,
  "session_id": "sess-1",
  "thread_id": 1,
  "semantic_parent_uuid": null,
  "text": "what is a delta?",
  "locator_quote": "the main channel",
  "status": "pending",
  "matched_uuid": null,
  "created_at": "2026-01-01T00:00:00Z"
}
```

`status` is one of `pending`, `matched`, `cancelled`. `matched_uuid` is set once
the send is correlated with a transcript message.

## Browser REST surface

### `GET /api/session`

Hydrate the current session and its trunk thread.

- **200** — the session exists:

  ```json
  {
    "session": { /* Session */ },
    "main_thread_id": 1
  }
  ```

- **404** — no session has been registered yet (the first `UserPromptSubmit`
  hook registers it).

### `GET /api/threads`

List the thread tree for the navigator, ordered by creation (ascending `id`).

- **200**:

  ```json
  { "threads": [ /* Thread, ... */ ] }
  ```

  Returns an empty list when no session is registered.

### `GET /api/threads/:id/messages`

Return a thread's messages, ordered by `seq`.

- **200**:

  ```json
  { "messages": [ /* Message, ... */ ] }
  ```

- **404** — no thread with that id.

### `POST /api/sends`

Enqueue a send into the session. The send is written to the correlation FIFO
and then dispatched into the tmux pane as keystrokes. When
`semantic_parent_uuid` is present this is a branch send: a new unnamed child
thread is created off that message and the send is attributed to it.

Request:

```json
{
  "thread_id": 1,
  "text": "what is a delta?",
  "locator_quote": "the main channel",
  "semantic_parent_uuid": null
}
```

- `thread_id` (required) — the target thread, or the parent thread for a branch.
- `text` (required) — the text to send.
- `locator_quote` (optional) — injected as `additionalContext` on the matching
  turn so the model can locate the referenced text.
- `semantic_parent_uuid` (optional) — when set, makes this a branch send.

Response:

- **201 Created**:

  ```json
  { "send": { /* PendingSend */ } }
  ```

- **404** — no session registered, or no thread (or branch parent thread) with
  the given `thread_id`.

## Browser live channels

### `GET /ws` (WebSocket)

After upgrade, the server pushes JSON-encoded `SessionEvent`s to the browser as
text frames, one event per frame. Each event is a tagged union keyed on `kind`:

```json
{ "kind": "session_registered", "session_id": "sess-1" }

{ "kind": "turn_started",
  "session_id": "sess-1",
  "pending_send_id": 1,
  "matched_uuid": "uuid-1" }

{ "kind": "external_input", "session_id": "sess-1", "prompt": "typed in pane" }

{ "kind": "turn_completed", "session_id": "sess-1", "stop_reason": null }

{ "kind": "permission_requested",
  "session_id": "sess-1",
  "request_id": 1,
  "tool_name": "Bash" }
```

- `session_registered` — emitted on the first `UserPromptSubmit`.
- `turn_started` — a queued send was correlated with a transcript message.
- `external_input` — a prompt with no matching queued send (typed directly into
  the pane).
- `turn_completed` — a response finished (from the `Stop` hook).
- `permission_requested` — a tool permission prompt is imminent.

The stream is process-wide: every connected browser receives every event. There
is no client→server message protocol on this socket.

### `GET /pty` (WebSocket)

A raw terminal bridge. After upgrade the server runs `tmux attach-session`
against the configured pane inside a pseudo-terminal and shuttles bytes both
ways:

- **Server → browser**: PTY output as binary frames.
- **Browser → server**: input frames (binary or text) written into the PTY.

Used by the embedded xterm.js terminal, primarily so the user can answer
permission prompts in the TUI. This is a minimal attach; resize negotiation is
not yet modelled.

## Control plane (`/hooks/*`)

Claude Code fires these hooks even inside an interactive tmux session. Delta
uses them to register the session, correlate sends, and broadcast events. All
return **200 OK** on success and **500** with a plain-text reason on failure.

### `POST /hooks/user-prompt-submit`

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
{ "additionalContext": "the main channel" }
```

`additionalContext` is present only when the matched send carried a
`locator_quote`; it is injected into this prompt only. The field is omitted
otherwise.

### `POST /hooks/stop`

Fires when a response completes. Delta ingests any final transcript lines and
broadcasts `turn_completed`.

Request:

```json
{
  "session_id": "sess-1",
  "stop_reason": null
}
```

`stop_reason` is optional. Any additional fields Claude Code sends (such as
`last_assistant_message`) are ignored. Response: **200 OK** with an empty body.

### `POST /hooks/pre-tool-use`

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

## Health

### `GET /health`

Liveness probe. Returns **200 OK** with the plain-text body `ok`.
