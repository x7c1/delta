# Browser live channels

## Overview

The three WebSocket surfaces the browser holds open: the event stream (`/ws`)
every client consumes, plus one right-pane window per session whose kind
follows the provider's capabilities (see
[settings.md — `GET /api/providers`](settings.md#get-apiproviders)): a PTY bridge
(`/pty`) for a provider with an attachable terminal, or a comms log (`/comms`)
for a provider Delta drives headlessly. Shared conventions are in
[README.md](README.md).

## `GET /ws` (WebSocket)

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

## `GET /pty?session_id=<id>` (WebSocket)

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

## `GET /comms?session_id=<id>` (WebSocket)

An observability stream for one session: the JSON-RPC frames Delta exchanges with
a provider it drives headlessly (Codex's `codex app-server`). It is what a
session with no terminal has in place of `/pty`. The UI picks between the two
from the provider's `capabilities`
(see [settings.md — `GET /api/providers`](settings.md#get-apiproviders)): `has_terminal`
gets the terminal pane, `has_comms_log` gets this one.

- **Server → browser**: one `CommsFrame` per text frame, as JSON.
- **Browser → server**: nothing. The stream is one-way.

```json
{
  "seq": 4,
  "at_ms": 1767258600000,
  "direction": "from_agent",
  "kind": "notification",
  "method": "turn/completed",
  "payload_json": "{\"method\":\"turn/completed\",\"params\":{...}}"
}
```

- `seq` is a per-session monotonic counter minted as the frame is recorded. Order
  and de-duplicate on it, not on `at_ms` (two frames can share a millisecond).
- `direction` is `to_agent` (Delta wrote it) or `from_agent` (Delta read it).
- `kind` is `request`, `response`, or `notification`. A *server-originated*
  request is `from_agent` + `request`, not a fourth kind.
- `method` is `null` only on Delta's own answer to a server request, which names
  no method.
- `payload_json` is the frame as JSON **text**, to be parsed by the client only
  for display.

On connect the server replays what it still holds for the session (a bounded
in-memory ring buffer of recent frames) and then tails live on the same stream,
so a client connecting mid-turn sees the frames that already flew.

This channel is **observability only**, with three consequences worth relying on:

- frames are never persisted and never enter the conversation pipeline — they are
  not `SessionEvent`s and touch no database row;
- the buffer is per live session and is dropped when the session closes or the
  server restarts, so a reconnect after either shows an empty log;
- delivery is lossy by design: a client that stops reading is skipped forward
  rather than allowed to apply back-pressure, because nothing about an inspector
  may ever slow a turn down.

A session with no live wire — closed, dormant, or running on a provider that has
a terminal instead — is not an error: the socket opens and simply stays quiet, and
the client shows an idle state.

One log holds only the frames that can be attributed to its session: the
provider's own frames are attributed by the thread id they carry, so a frame
naming no thread (the shared app-server's connection-level notifications) appears
in no session's log. When diagnosing from this stream, an absent frame means "not
attributable to this session", never "not sent".
