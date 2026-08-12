# Delta server API

## Overview

The Delta server is a local process that wraps one or more AI agent sessions
and exposes them to a browser UI. Each provider is driven through its own
adapter: Claude Code runs in a tmux pane (driven via `send-keys`, observed
through JSONL transcripts and HTTP hooks), while Codex runs as a
`codex app-server` subprocess spoken to over JSON-RPC. The server handles three
kinds of traffic, and this directory has one page per area:

- The **browser REST surface** (`/api/*`) — the request/response queries and
  commands the browser issues to hydrate state and drive a session — split by
  area:
  - **[sessions.md](sessions.md)** — listing, creating, opening, closing and
    interrupting sessions, plus reading threads and messages.
  - **[sends.md](sends.md)** — enqueueing and managing sends, and answering the
    permission requests and questions the agent raises mid-turn.
  - **[workdirs.md](workdirs.md)** — the new-session dialog's sources: working
    directories, git detection, repositories and scan roots, pull requests, and
    opening a known directory in an external editor.
  - **[settings.md](settings.md)** — provider availability and capabilities, the
    launch-option registry, and the server version.
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
interface. It listens on port `7878` unless `DELTA_PORT` overrides it, so the
base URL every path below hangs off is `http://127.0.0.1:7878`. All request and
response bodies are JSON unless noted otherwise.

These documents are the source of truth for the browser↔server contract. Field
names match the JSON exactly. Every shape on this surface — the `/api/*`
request/response bodies and the `/ws` session events — is defined by the
backend's `delta-wire` crate, which also generates the frontend's
`@delta/wire-gen` TypeScript bindings (`make gen`), so the types cannot drift
from the implementation.

The routes those shapes travel over are declared in the same crate, as
`delta_wire::endpoint::ENDPOINTS`
(`backend/crates/gateway/delta-wire/src/endpoint/table.rs`) — one entry per
method and path, covering the live channels and the hook control plane as well
as `/api/*`. The server mounts every handler through that table and refuses to
build a router that disagrees with it, so the table is the complete and current
list of routes. Every route in it is documented here, and a `delta-wire` test
walks the real table against these files, so a new route cannot be declared
without a section describing it. What the table leaves out — query parameters,
status codes and error bodies — is documented here only.

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
    ambiguous, contradictory, or names something the server will not act on (see
    [sends.md — `POST /api/sends`](sends.md#post-apisends) — JSON body).
  - `403 Forbidden` — a path the server is not permitted to read, as opposed to
    one that does not exist. Returned by the directory browse
    ([workdirs.md — `GET /api/workdir/list`](workdirs.md#get-apiworkdirlist)).
  - `404 Not Found` — an unknown session id, thread, or launch option.
  - `409 Conflict` — the request cannot take effect against current state. The
    body's `code` says which case it is: `resume_unavailable` (the session's
    local transcript file is gone, so `claude --resume <id>` has nothing to
    replay), `permission_not_pending`, `question_not_pending`,
    `send_not_cancellable`, `send_not_releasable`, or `scan_root_duplicate`.
    Nothing is mutated in any of these cases.
  - `415 Unsupported Media Type` — a request body sent with a non-JSON
    `Content-Type`.
  - `422 Unprocessable Entity` — a syntactically valid JSON body that does not
    match the endpoint's schema (a missing required field or a field of the
    wrong type).
  - `500 Internal Server Error` — a store, transcript, tmux, git, `gh`, agent
    adapter, or workspace failure.

## Health

### `GET /health`

Liveness probe. Returns **200 OK** with the plain-text body `ok`, touching no
state.
