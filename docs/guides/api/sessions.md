# Sessions, threads and messages (`/api/*`)

## Overview

The REST routes that list and drive sessions, and that read a session's thread
tree and a thread's messages. Sends into a session are in
[sends.md](sends.md); shared JSON shapes are in [shapes.md](shapes.md);
conventions and error semantics are in [README.md](README.md).

Sessions are addressed by id. Open/closed is process-runtime state held by the
server (rebuilt empty on restart): a session that exists in the store but has no
live pane is *closed* and must be reopened before it can receive a send.

A session can also become closed **without being asked to**: when an
adapter-backed provider's process ends unexpectedly (a killed `codex app-server`)
the session settles — its in-flight turn ends as `turn_interrupted`, its pending
permission requests are settled (see
[sends.md](sends.md#the-pending-permission-queue)) — and it reports `open: false`,
announced as `session_closed`. Delta does not respawn the process: a send to the
settled session resumes it, exactly as after a server restart.

## Sessions

### `GET /api/sessions`

List known sessions, **open-first**, each annotated with its live state and
trunk thread. Every live session comes before every closed one; within each
group the order is most recent activity first. The recency key is a session's
last activity (`last_activity_at`), falling back to its own `created_at` when it
has no messages yet — so a brand-new session sorts near the top of its group. A
closed session never outranks a live one, however recently its transcript was
touched. This is the browser's hydration surface: it shows every conversation —
open or closed — so the navigator can route into any of them.

The leading group is wider than `open: true`: a session whose spawn is still in
flight (`status: "spawning"`, not yet bound to a pane) is live too, so a
just-started session leads the list from the moment its first send is accepted.

Query parameters:

- `cursor` (optional) — the previous page's `next_cursor`, passed back
  unchanged. Opaque; a token the server cannot decode is a `400`.
- `limit` (optional) — how many **closed** sessions a page carries, clamped to
  `[1, 100]`. Defaults to `30`. It does not bound the whole page: the first page
  also carries the entire live group, which is small (bounded by the number of
  live panes). Later pages are closed sessions only.

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
  most recent message (`MAX(message.created_at)`), or `null` when the session has
  no messages yet. `next_cursor` is an opaque token to fetch the following page,
  or `null` on the last page — it advances through the closed sessions, since
  the live group rides on the first page alone.

  Liveness is snapshotted per request, so a session whose state flips partway
  through a page walk can be listed twice or not at all. One that **closes**
  between two fetches appears a second time in the closed portion of a later
  page: it led the first page as live and is no longer excluded from the closed
  stream. One that **opens** between two fetches is missed: it is now excluded
  from the closed stream, while the live group that would carry it rode on the
  first page. Clients need to handle neither — `session_opened` and
  `session_closed`, which are exactly the events that cause them, each
  invalidate the whole list, so the walk restarts from a fresh page 1 at once.

  A session is listed from the moment its first send is accepted, whatever its
  provider: the `POST /api/sends` that spawns it writes the row before the
  launch, and that row carries `status: "spawning"` until the launch binds it —
  the first hook for a Claude session, the adapter's `thread/start` for a Codex
  one. The same row then reads `status: "active"` (announced as
  `session_registered` either way). A spawning session is addressable like any
  other (its threads and open sends are queryable, so the browser can focus it
  and show its first prompt right away), but it is not open — nothing is bound
  to it yet, so it reports `open: false` and nothing can be *dispatched* into
  it. It counts as live for ordering all the same, so it leads the list. A plain
  send to it is still accepted, as a `queued` row typed once the launch binds;
  only a branch send is refused, with `409 session_spawning` (see
  [sends.md](sends.md)). A launch that fails, and a Claude spawn that came up
  but never bound (reaped at its bind deadline), both have their row deleted, so
  the session disappears from this list again and the client hears
  `spawn_failed`.

- **400** — a malformed `cursor`.

### `POST /api/sessions`

Spawn a fresh session eagerly. Used by cold start (an empty session list) and the
"New" button: the server starts a tmux session running `claude --settings <file>`
in a fresh per-spawn working directory. The settings file (rendered with this
server's hook URLs) is Delta-owned and lives outside the working directory, so
Claude Code's hooks point back at this server without touching any
`.claude/settings.json` in the working directory.

This drives only the tmux/process lifecycle; the conversational session is still
registered later by the first `SessionStart`/`UserPromptSubmit` hook. The row
itself is written before the launch, exactly as for a new-session send, so the
session is listed by `GET /api/sessions` straight away with `status: "spawning"`
and flips to `active` at registration. The response likewise returns before the
launch preparation runs — the same split
[`POST /api/sends`](sends.md#post-apisends) makes — so a preparation failure
arrives as a `spawn_failed` event with a `reason` rather than as a `500`, and
deletes the row. The call is idempotent across that whole window: a second call
made while the first session's launch is still being prepared reuses it instead
of starting a rival session.

Authentication is assumed: the server relies on a cached Claude Code token (or
`CLAUDE_CODE_OAUTH_TOKEN`) and does not perform interactive OAuth. If the session
never becomes usable, the user answers prompts in the embedded terminal (`/pty`).

- **200** — the spawn was accepted:

  ```json
  { "status": "ready" }
  ```

  `status` is `ready` when an existing session was reused, or `starting` when the
  session was just created and may still be coming up.

- **500** — the synchronous part failed: probing tmux for a free session name,
  or writing the session row. Writing the settings file and starting the tmux
  session are no longer part of this response (see above); they fail as a
  `spawn_failed` event instead.

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

Close a session. This has two outcomes, decided by whether the session ever
bound an agent.

**A session that has bound is torn down and kept**: the live pane is killed and
dropped from the registry (a terminal-less Codex session is closed through its
adapter instead); the conversation remains in the store and can be reopened.
Closing a session that is already closed is a no-op.

Closing also sweeps any lingering background subagent whose completion
notification can no longer arrive, broadcasting a `subagent_finished` for each so
live viewers' running indicators clear immediately.

**A session that is still starting has its launch cancelled and its row
removed.** Such a session holds no conversation: its row was written eagerly
when the send was accepted (it is listed as `spawning`) and no transcript line
has been ingested against it, so there is nothing to tear down and keep. Delta
therefore reclaims whatever the launch has stood up so far — the launch
preparation is abandoned, an unbound pane is killed, a connected provider is
dropped — and deletes the row. The cancellation is reported on the live channel
as a [`spawn_failed`](live-channels.md#session-lifecycle) marked `cancelled`
(the key that tells a requested cancel from a broken launch), whose `reason`
names the close and whose `unsent` carries every send the launch never
delivered, so a client can put that text back in front of the user; the row is
gone from the next
`GET /api/sessions`. This is what makes a wedged launch recoverable: a `git fetch`
hanging past every deadline, or a `spawning` row stranded by a server restart
mid-launch — open/closed is runtime state rebuilt empty on restart, so no
watchdog is left to reap it — would otherwise leave a session the user could not
be rid of.

Either way `session_closed` is broadcast.

- **204 No Content** — the session is closed or its launch was cancelled (or it
  was already closed).
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
