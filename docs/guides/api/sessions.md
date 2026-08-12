# Sessions, threads and messages (`/api/*`)

## Overview

The REST routes that list and drive sessions, and that read a session's thread
tree and a thread's messages. Sends into a session are in
[sends.md](sends.md); shared JSON shapes are in [shapes.md](shapes.md);
conventions and error semantics are in [README.md](README.md).

Sessions are addressed by id. Open/closed is process-runtime state held by the
server (rebuilt empty on restart): a session that exists in the store but has no
live pane is *closed* and must be reopened before it can receive a send.

## Sessions

### `GET /api/sessions`

List known sessions, ordered by most recent activity (newest first), each
annotated with its live state and trunk thread. The recency key is a session's
last activity (`last_activity_at`), falling back to its own `created_at` when it
has no messages yet — so a brand-new session sorts near the top. This is the
browser's hydration surface: it shows every conversation — open or closed — so
the navigator can route into any of them.

Query parameters:

- `cursor` (optional) — the previous page's `next_cursor`, passed back
  unchanged. Opaque; a token the server cannot decode is a `400`.
- `limit` (optional) — page size, clamped to `[1, 100]`. Defaults to `30`.

Response:

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
    ],
    "next_cursor": null
  }
  ```

  `open` is `true` when the session currently has a live pane (resumable without
  `--resume`). `last_activity_at` is the ISO-8601 UTC timestamp of the session's
  most recent message (`MAX(message.created_at)`), or `null` when the session
  has no messages yet. `next_cursor` is an opaque token to fetch the following
  page, or `null` on the last page. Returns an empty list until the first
  session is registered.

- **400** — a malformed `cursor`.

### `POST /api/sessions`

Spawn a fresh session eagerly. Used by cold start (an empty session list) and the
"New" button: the server starts a tmux session running `claude --settings <file>`
in a fresh per-spawn working directory. The settings file (rendered with this
server's hook URLs) is Delta-owned and lives outside the working directory, so
Claude Code's hooks point back at this server without touching any
`.claude/settings.json` in the working directory.

This drives only the tmux/process lifecycle; the conversational session is still
registered later by the first `SessionStart`/`UserPromptSubmit` hook, so a
freshly created session has no `Session` row yet (it appears in
`GET /api/sessions` once registered). The call is idempotent while a spawn is
still live: a second call with a session already coming up reuses it.

Authentication is assumed: the server relies on a cached Claude Code token (or
`CLAUDE_CODE_OAUTH_TOKEN`) and does not perform interactive OAuth. If the session
never becomes usable, the user answers prompts in the embedded terminal (`/pty`).

- **200** — the spawn was accepted:

  ```json
  { "status": "ready" }
  ```

  `status` is `ready` when an existing session was reused, or `starting` when the
  session was just created and may still be coming up.

- **500** — preparing the working directory or starting the tmux session failed.

### `POST /api/sessions/{id}/open`

Resume a closed, known session. For a pane-backed (Claude) session this
re-launches `claude --resume <id>` and binds the new pane; for a terminal-less
(Codex) session it reconnects the adapter to the provider's thread — there is no
pane. Either way it broadcasts `session_opened`. Re-opening an already-open
session is a no-op.

- **204 No Content** — the session is now open.
- **404** — no session with that id.
- **409** — the session's transcript is gone, so it cannot be resumed (body
  `code: "resume_unavailable"`); it is left closed and no pane is spawned.
- **500** — for a Claude session, rewriting the session settings file, seeding
  git trust, or starting the tmux session failed; for a Codex session, the
  adapter's thread-resume call failed. The session's working directory is
  already known from the original spawn, so unlike `POST /api/sessions` there
  is no working-directory preparation step to fail here.

### `POST /api/sessions/{id}/close`

Tear down an open session's pane, keeping its data. Kills the live pane and drops
it from the registry; the conversation remains in the store and can be reopened.
Broadcasts `session_closed`. Closing a session that is not open is a no-op.

Closing also sweeps any lingering background subagent whose completion
notification can no longer arrive, broadcasting a `subagent_finished` for each so
live viewers' running indicators clear immediately.

- **204 No Content** — the session is closed (or already was).
- **404** — no session with that id.
- **500** — killing the tmux session failed.

### `POST /api/sessions/{id}/interrupt`

Abort the session's in-flight turn without closing it.

For a terminal-less (Codex) session this drives the adapter's interrupt on the
provider's wire; the resulting `turn_interrupted` settles over the WebSocket
event stream, so nothing is reported synchronously here. For a pane-backed
(Claude) session, a closed session, or an unknown id this is a well-defined
no-op: Claude's turn interrupt is TUI-driven (`Escape` in the pane) and this
route deliberately does not duplicate it.

- **204 No Content** — the interrupt was delivered, or there was nothing to
  interrupt.
- **500** — the provider's transport failed while delivering the interrupt.

## Threads and messages

### `GET /api/sessions/{id}/threads`

List a session's thread tree for the navigator, ordered by creation (ascending
`id`).

- **200**:

  ```json
  { "threads": [ /* Thread, ... */ ] }
  ```

- **404** — no session with that id. An unknown id is reported rather than
  answered with an empty list, so "no threads yet" is distinguishable from "no
  such session".

### `GET /api/threads/{id}/messages`

Return a thread's messages, ordered by `seq`. Thread ids are globally unique, so
this is not scoped by session.

- **200**:

  ```json
  { "messages": [ /* Message, ... */ ] }
  ```

- **400** — the `{id}` path segment is not an integer.
- **404** — no thread with that id.
