# Browser REST surface (`/api/*`)

## Overview

The request/response queries and commands the browser issues to hydrate state
and enqueue sends. Shared JSON shapes are in [shapes.md](shapes.md);
conventions and error semantics are in [README.md](README.md).

Sessions are addressed by id. Open/closed is process-runtime state held by the
server (rebuilt empty on restart): a session that exists in the store but has no
live pane is *closed* and must be reopened before it can receive a send.

## Sessions

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

## Threads and messages

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

## Sends

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
  matching turn so the model can locate the referenced text (see
  [hooks.md — `POST /hooks/user-prompt-submit`](hooks.md#post-hooksuser-prompt-submit)
  for the framing). Ignored for a `new_session` send: a brand-new session has no
  earlier passage to anchor, so the quote is dropped rather than carried onto the
  first prompt.
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

## Providers

### `GET /api/providers`

Launch availability and capability profile for every known agent provider
(Claude, Codex). The new-session provider selector disables an unavailable
provider and shows the reason, so a user cannot pick a provider that would fail
at spawn; the workspace reads the capability profile to gate provider-specific
surfaces (the terminal pane vs the comms-log pane, and the vocabulary the
Settings launch-option form tells the user to write in). Always **200**: a
missing binary is data (`available: false`), never an error.

- **200**:

  ```json
  {
    "providers": [
      {
        "provider": "claude",
        "available": true,
        "detail": null,
        "capabilities": {
          "has_terminal": true,
          "has_comms_log": false,
          "launch_option_style": "cli_flag"
        }
      }
    ]
  }
  ```

  - `available` reports whether the provider's configured launch binary is
    present on the server host (binary presence only). `detail` carries a
    human-readable reason when `available` is `false`, `null` otherwise.
  - `capabilities` is the provider's static, UI-relevant capability profile —
    present even for an unavailable provider:
    - `has_terminal` — the provider offers a terminal the browser can attach
      to; its sessions get the terminal pane (`/pty`).
    - `has_comms_log` — the browser can inspect the frames Delta exchanges
      with this provider; its sessions get the comms-log pane (`/comms`).
      Complementary with `has_terminal`, not independent — see
      [live-channels.md](live-channels.md).
    - `launch_option_style` — how the provider reads a registered launch
      option's `(name, value?)` pair: `cli_flag` (`name` is a command-line
      flag, e.g. `--permission-mode`) or `request_field` (`name` is a field of
      the provider's session-start request, e.g. Codex's `model`).
