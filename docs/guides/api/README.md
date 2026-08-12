# Delta server API

## Overview

The Delta server is a local process that wraps one or more AI agent sessions
and exposes them to a browser UI. Each provider is driven through its own
adapter: Claude Code runs in a tmux pane (driven via `send-keys`, observed
through JSONL transcripts and HTTP hooks), while Codex runs as a
`codex app-server` subprocess spoken to over JSON-RPC. The server serves three
kinds of traffic, each documented in its own file:

- **[rest.md](rest.md)** — the browser REST surface (`/api/*`):
  request/response queries and commands the browser issues to hydrate state
  and enqueue sends.
- **[live-channels.md](live-channels.md)** — the browser live channels: a
  WebSocket event stream (`/ws`) carrying `SessionEvent`s, a PTY bridge
  (`/pty`) attaching an xterm.js terminal to a session's tmux pane, and a
  comms log (`/comms`) streaming the frames Delta exchanges with a headless
  provider's transport.
- **[hooks.md](hooks.md)** — the control plane (`/hooks/*`): HTTP hooks
  Claude Code fires during a session.

The JSON shapes shared across those surfaces (`Session`, `Thread`, `Message`,
`ContentBlock`, `PendingSend`) live in **[shapes.md](shapes.md)**.

The server binds to `127.0.0.1` only; it is never exposed on a public
interface. All request and response bodies are JSON unless noted otherwise.

These documents are the source of truth for the browser↔server contract. Field
names match the JSON exactly. Every shape on this surface — the `/api/*`
request/response bodies and the `/ws` session events — is defined by the
backend's `delta-wire` crate, which also generates the frontend's
`@delta/wire-gen` TypeScript bindings (`make gen`), so the types cannot drift
from the implementation.

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
    ambiguous or contradictory (see [rest.md — `POST /api/sends`](rest.md#post-apisends)
    — JSON body).
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

## Health

### `GET /health`

Liveness probe. Returns **200 OK** with the plain-text body `ok`.
