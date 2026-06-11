# Delta server API

## Overview

The Delta server is a local process that wraps one or more Claude Code sessions
and exposes them to a browser UI. It serves three kinds of traffic:

- **Browser REST surface** (`/api/*`) — request/response queries and commands
  the browser issues to hydrate state and enqueue sends. Sessions are listed,
  created, opened, and closed by id, and threads and sends are routed to a
  specific session.
- **Browser live channels** — a WebSocket event stream (`/ws`) carrying
  `SessionEvent`s, and a PTY bridge (`/pty?session_id=<id>`) attaching an
  xterm.js terminal to a named session's tmux pane.
- **Control plane** (`/hooks/*`) — HTTP hooks Claude Code fires during a
  session. Delta correlates them with queued sends and broadcasts events.

The server binds to `127.0.0.1` only; it is never exposed on a public
interface. All request and response bodies are JSON unless noted otherwise.

This document is the source of truth for the browser↔server contract. Field
names below match the JSON exactly. The `/ws` session-event shapes are defined
by the backend's `delta-wire` crate, which also generates the frontend's
`@delta/wire-gen` TypeScript bindings (`make gen`); the remaining shapes are
the serialized domain types.

## Conventions

- All timestamps are ISO-8601 strings.
- `thread_id` is an integer issued by the server. `session_id`, message `uuid`,
  and `prompt_id` are strings.
- Errors carry a JSON body `{ "error": "<message>" }`, except request-decoding
  failures rejected before a handler runs (the framework-level `400`/`415`/`422`
  cases below), which carry a plain-text body from the framework. Some errors
  also include a stable, machine-readable `code` (e.g.
  `{ "error": "...", "code": "resume_unavailable" }`) that clients can branch on
  instead of matching the human message; the field is omitted when there is no
  distinct code. Status codes:
  - `400 Bad Request` — a malformed request. Either rejected before the handler
    (a syntactically invalid JSON body, a missing `Content-Type:
    application/json` header, a missing required query parameter, or a path/query
    segment that cannot be parsed such as a non-integer thread id — plain-text
    body), or rejected by a handler for a structurally valid body whose target is
    ambiguous or contradictory (see `POST /api/sends` — JSON body).
  - `404 Not Found` — an unknown session id, or an unknown thread.
  - `409 Conflict` — the session exists but cannot be resumed because its local
    transcript file is gone, so `claude --resume <id>` has nothing to replay
    (body `code: "resume_unavailable"`). Returned by `POST /api/sessions/{id}/open`
    and by `POST /api/sends` when the target's session is closed and must be
    resumed first. The session is left closed; no pane is spawned and no send is
    enqueued.
  - `415 Unsupported Media Type` — a request body sent with a non-JSON
    `Content-Type`.
  - `422 Unprocessable Entity` — a syntactically valid JSON body that does not
    match the endpoint's schema (a missing required field or a field of the
    wrong type).
  - `500 Internal Server Error` — a store, transcript, tmux, or workspace
    failure.

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

Sessions are addressed by id. Open/closed is process-runtime state held by the
server (rebuilt empty on restart): a session that exists in the store but has no
live pane is *closed* and must be reopened before it can receive a send.

### `GET /api/sessions`

List every known session, ordered by most recent activity (newest first), each
annotated with its live state and trunk thread. The recency key is a session's
last activity (`last_activity_at`), falling back to its own `created_at` when it
has no messages yet — so a brand-new session sorts near the top. This is the
browser's hydration surface: it shows every conversation — open or closed — so
the navigator can route into any of them.

- **200**:

  ```json
  {
    "sessions": [
      {
        "session": { /* Session */ },
        "open": true,
        "main_thread_id": 1,
        "last_activity_at": "2026-01-01T00:01:01Z"
      }
    ]
  }
  ```

  `open` is `true` when the session currently has a live pane (resumable without
  `--resume`). `last_activity_at` is the ISO-8601 UTC timestamp of the session's
  most recent message (`MAX(message.created_at)`), or `null` when the session
  has no messages yet. Returns an empty list until the first `UserPromptSubmit`
  hook registers a session.

### `POST /api/sessions`

Spawn a fresh session eagerly. Used by cold start (an empty session list) and the
"New" button: the server starts a tmux session running `claude --settings <file>`
in a fresh per-spawn working directory. The settings file (rendered with this
server's hook URLs) is Delta-owned and lives outside the working directory, so
Claude Code's hooks point back at this server without touching any
`.claude/settings.json` in the working directory.

This drives only the tmux/process lifecycle; the conversational session is still
registered later by the first `UserPromptSubmit` hook, so a freshly created
session has no `Session` row yet (it appears in `GET /api/sessions` once
registered). The call is idempotent while a spawn is still live: a second call
with a session already coming up reuses it.

Authentication is assumed: the server relies on a cached Claude Code token (or
`CLAUDE_CODE_OAUTH_TOKEN`) and does not perform interactive OAuth. If the session
never becomes usable, the user answers prompts in the embedded terminal (`/pty`).

- **200** — the session is up:

  ```json
  { "status": "ready" }
  ```

  `status` is `ready` when an existing session was reused, or `starting` when the
  session was just created and may still be coming up.

- **500** — preparing the working directory or starting the tmux session failed.

### `POST /api/sessions/{id}/open`

Resume a closed, known session: re-launch `claude --resume <id>` and bind the new
pane. Broadcasts `session_opened`. Re-opening an already-open session is a no-op.

- **204 No Content** — the session is now open.
- **404** — no session with that id.
- **409** — the session's transcript is gone, so it cannot be resumed (body
  `code: "resume_unavailable"`); it is left closed and no pane is spawned.
- **500** — preparing the working directory or starting the tmux session failed.

### `POST /api/sessions/{id}/close`

Tear down an open session's pane, keeping its data. Kills the live pane and drops
it from the registry; the conversation remains in the store and can be reopened.
Broadcasts `session_closed`. Closing a session that is not open is a no-op.

- **204 No Content** — the session is closed (or already was).
- **404** — no session with that id.
- **500** — killing the tmux session failed.

### `GET /api/sessions/{id}/threads`

List a session's thread tree for the navigator, ordered by creation (ascending
`id`).

- **200**:

  ```json
  { "threads": [ /* Thread, ... */ ] }
  ```

- **404** — no session with that id.

### `GET /api/threads/{id}/messages`

Return a thread's messages, ordered by `seq`. Thread ids are globally unique, so
this is not scoped by session.

- **200**:

  ```json
  { "messages": [ /* Message, ... */ ] }
  ```

- **400** — the `{id}` path segment is not an integer.
- **404** — no thread with that id.

### `POST /api/sends`

Enqueue a send. The send is written to a session's correlation FIFO and dispatched
into its tmux pane as keystrokes. The session is determined by the request, not by
an implicit "current" session — the send either continues an existing session or
starts a new one:

- **Existing session** — set `thread_id`. The session is derived from the thread
  (threads belong to a session). When `semantic_parent_uuid` is also set this is a
  branch send: a new unnamed child thread is created off that message and the send
  is attributed to it. If the session is closed it is resumed first.
- **New session** — set `new_session: true` and omit `thread_id`. A fresh session
  is spawned and the text is deferred as its first prompt, landing on the new
  session's `main` thread once the spawn binds.

Request (existing session):

```json
{
  "thread_id": 1,
  "text": "what is a delta?",
  "locator_quote": "the main channel",
  "semantic_parent_uuid": null
}
```

Request (new session):

```json
{
  "new_session": true,
  "text": "start a fresh conversation"
}
```

- `thread_id` (optional) — the target thread, or the parent thread for a branch.
  Required unless `new_session` is set; mutually exclusive with it.
- `new_session` (optional, default `false`) — start a fresh session and land the
  message on its `main` thread. Cannot be combined with `thread_id` or
  `semantic_parent_uuid`.
- `text` (required) — the text to send.
- `locator_quote` (optional) — framed and injected as `additionalContext` on the
  matching turn so the model can locate the referenced text (see the
  `user-prompt-submit` hook below for the framing). Ignored for a `new_session`
  send: a brand-new session has no earlier passage to anchor, so the quote is
  dropped rather than carried onto the first prompt.
- `semantic_parent_uuid` (optional) — when set, makes this a branch send (only
  valid with `thread_id`).

Response:

- **201 Created**:

  ```json
  { "send": { /* PendingSend */ } }
  ```

  For a `new_session` send the returned `PendingSend` is synthetic: no row exists
  yet (the session id it would reference is unknown until the spawn binds), so its
  `id` is `0`, `session_id` is empty, and `thread_id` is `0`. The real,
  correlatable row is written on the new session's `main` thread at bind time. Its
  `locator_quote` echoes the request only as a courtesy; it is not anchored to the
  first prompt (see the field note above), so the persisted row carries no quote.

- **400** — the target is ambiguous or contradictory (a JSON body): neither
  `thread_id` nor `new_session` given, both given, or `new_session` combined with
  a branch (`semantic_parent_uuid`). Also the framework-level decode failures
  (malformed JSON, wrong/missing `Content-Type`, or a missing/wrong-typed field
  such as an absent `text`) reported as `400 / 415 / 422` with a plain-text body.
- **404** — no thread (or branch parent thread) with the given `thread_id`.
- **409** — the target's session is closed and cannot be resumed because its
  transcript is gone (body `code: "resume_unavailable"`). No send is enqueued and
  the session stays closed.

## Browser live channels

### `GET /ws` (WebSocket)

After upgrade, the server pushes JSON-encoded `SessionEvent`s to the browser as
text frames, one event per frame. Each event is a tagged union keyed on `kind`.
These shapes are defined by the `delta-wire` crate (`WireSessionEvent`), and
the frontend consumes TypeScript bindings generated from it (`@delta/wire-gen`
via `make gen`), so the union below cannot drift from the implementation:

```json
{ "kind": "session_registered", "session_id": "sess-1" }

{ "kind": "session_opened", "session_id": "sess-1" }

{ "kind": "session_closed", "session_id": "sess-1" }

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

{ "kind": "transcript_updated",
  "session_id": "sess-1",
  "thread_ids": [1, 4] }
```

- `session_registered` — emitted on the first `UserPromptSubmit` for a session
  id. This also doubles as the "opened" signal for a freshly-spawned session: a
  new spawn has no `session_id` until its first hook binds it, so its first
  liveness signal is this registration rather than a separate `session_opened`.
- `session_opened` — a known, previously-closed session became live again
  (resumed by id via `POST /api/sessions/{id}/open`). A brand-new session never
  emits this.
- `session_closed` — an open session was closed (`POST /api/sessions/{id}/close`);
  its pane was torn down but its data remains.
- `turn_started` — a queued send was correlated with a transcript message.
- `external_input` — a prompt with no matching queued send (typed directly into
  the pane).
- `turn_completed` — a response finished (from the `Stop` hook).
- `permission_requested` — a tool permission prompt is imminent.
- `transcript_updated` — the background tail ingested new transcript lines
  between hooks. Claude Code often flushes the final assistant line to the JSONL
  *after* the `Stop` hook fires, so the hook sync misses it; a ~500ms poll picks
  it up and emits this so the browser refetches the affected `thread_ids`. Unlike
  `turn_completed`/`external_input` it carries no turn semantics — clients must
  only refetch those threads, never mutate the pending-send FIFO or unread
  badges.

The stream is process-wide: every connected browser receives every event, and
every event carries the `session_id` it concerns so the browser can route it to
the right session. Which session the user is looking at (focus) is purely
client-side; the server emits no focus event. There is no client→server message
protocol on this socket.

### `GET /pty?session_id=<id>` (WebSocket)

A raw terminal bridge for a specific session. The `session_id` query parameter
names the session to attach to; the server resolves that session's pane and runs
`tmux attach-session` against it inside a pseudo-terminal, shuttling bytes both
ways:

- **Server → browser**: PTY output as binary frames.
- **Browser → server**: two frame kinds, distinguished by WebSocket frame type:
  - **Binary frames** are raw input bytes written into the PTY.
  - **Text frames** are JSON control messages. The only control message today is
    resize, which resizes the PTY (and therefore tmux and the pane program) to
    match the browser terminal's dimensions:

    ```json
    { "type": "resize", "rows": 40, "cols": 120 }
    ```

    Malformed or unknown control messages are logged and ignored; they never
    break the bridge.

Used by the embedded xterm.js terminal, primarily so the user can answer
permission prompts in the TUI.

If the named session is not open (no live pane — it was never opened, or it is
closed), there is nothing to attach to: the server accepts the upgrade and then
closes the socket cleanly without attaching. The client should open the session
(`POST /api/sessions/{id}/open`) before connecting.

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
`transcript_updated` under `/ws`) catches those late lines and refetches them, so
the reply still appears without waiting for the next hook.

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
