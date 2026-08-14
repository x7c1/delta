# Sends, permissions and questions (`/api/*`)

## Overview

The REST routes that put text into a session and answer what the agent asks
back: enqueueing and managing sends, resolving a tool-permission request, and
answering or cancelling an `AskUserQuestion`. Session lifecycle routes are in
[sessions.md](sessions.md); shared JSON shapes are in [shapes.md](shapes.md);
conventions and error semantics are in [README.md](README.md).

A send moves through `queued` → `dispatched` → `matched` (or `cancelled`): it is
written to the session's correlation FIFO, handed to the agent, and finally
correlated with the message it produced. *How* it is handed over depends on the
provider's adapter:

- **Pane-backed (Claude Code)** — the text is typed into the session's tmux pane
  as keystrokes once the session is idle, and matched when its echo comes back
  through the transcript. This is the path with a real `queued` stage: only one
  send may be outstanding per turn, so anything composed mid-turn waits.
- **Adapter-backed (Codex)** — no pane, no keystrokes: the text rides a
  turn-start request on the `codex app-server` connection and is matched to the
  turn id that request returns, so the row goes `dispatched` → `matched` within
  the same call.

Permission and question answers are not sends — they resolve a request the agent
raised mid-turn. Permission requests form a FIFO **queue** per session (an
adapter-backed provider can leave several outstanding at once), surfaced one
dialog at a time; see
[The pending-permission queue](#the-pending-permission-queue).

## Sends

### `POST /api/sends`

Enqueue a send. The send is written to a session's correlation FIFO and
dispatched the way the session's provider is driven (see the Overview). The
session is determined by the request, not by an implicit "current" session —
the send either continues an existing session or starts a new one:

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
  "text": "start a fresh conversation",
  "workdir": "/work/delta",
  "provider": "claude",
  "launch_option_ids": [1, 4],
  "worktree": { "start_point": { "kind": "head" } }
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

The remaining fields describe how to *launch* a session, so they are meaningful
only with `new_session: true` and are ignored on a thread send:

- `workdir` (optional) — the directory the fresh session starts in. Must be an
  existing directory. Defaults to the per-spawn directory.
- `provider` (optional) — `"claude"` or `"codex"`. Defaults to `"claude"`.
- `launch_option_ids` (optional) — ids of registered
  [launch options](settings.md#get-apilaunch-options) to apply, in selection
  order. Omitted or empty starts the session with no extra options.
- `worktree` (optional) — start the session in a per-session git worktree of
  `workdir`. `start_point.kind` is `head` (branch off the repository's current
  `HEAD`), `remote_branch` (branch off `origin/<name>`, fetched first), or
  `use_remote_branch` (work on `<name>` itself in the worktree); the latter two
  carry `name`, the branch short name with no `origin/` prefix.

Response:

- **201 Created**:

  ```json
  { "send": { /* Send */ } }
  ```

  For a `new_session` send the session row and its `main` thread are created
  eagerly, before the agent is launched, and the send is enqueued on that thread
  in the same call — so the returned `Send` already carries the real
  `id`, `session_id` and `thread_id` the session keeps once it binds, not a
  synthetic placeholder. `locator_quote` is dropped before it reaches the spawn
  (see the field note above), so it is `null` on both the response and the
  persisted row.

- **400** — the target is ambiguous or contradictory (a JSON body): neither
  `thread_id` nor `new_session` given, both given, or `new_session` combined with
  a branch (`semantic_parent_uuid`). Also a `workdir` that does not exist or is
  not a directory, a `worktree` requested without a `workdir` or for a directory
  that is not a git repository, and a selected launch option the provider's
  adapter refuses. A malformed body or a missing required field such as `text`
  is rejected earlier as one of the framework-level `400`/`415`/`422` cases in
  [README.md](README.md).
- **404** — no thread (or branch parent thread) with the given `thread_id`.
- **409** — the target's session is closed and cannot be resumed because its
  transcript is gone (body `code: "resume_unavailable"`). No send is enqueued and
  the session stays closed.

### `GET /api/sessions/{id}/sends`

Return the session's open (non-terminal) sends — status `queued` or
`dispatched` — oldest first, plus the session's queryable live state. This is
the source of truth for the browser's send strip, and the one refetch a client
uses to rebuild everything the WebSocket may have missed while it was
disconnected: events fired during the gap are never replayed.

- **200**:

  ```json
  {
    "sends": [ /* Send, ... */ ],
    "turn": { "state": "in_flight", "send_id": 7, "thread_id": 2 },
    "permission": {
      "request_id": 3,
      "tool_name": "Bash",
      "tool_input": "{\"command\":\"ls\"}"
    },
    "permission_count": 1,
    "question": {
      "request_id": 5,
      "thread_id": 3,
      "tool_input": "{\"questions\":[{\"header\":\"Pick\"}]}"
    },
    "running_subagents": [
      {
        "thread_id": 2,
        "tool_use_id": "toolu_01",
        "subagent_type": "general-purpose",
        "description": "Probe the codebase",
        "background": false
      }
    ]
  }
  ```

  - `turn.state` is `idle`, `awaiting_echo` (pane-backed only: a send was typed
    into the pane and its echo has not arrived), or `in_flight` — an
    adapter-backed session goes straight to `in_flight`, its turn-start request
    needing no echo. `send_id` names the send driving the turn — `null` while
    idle and for a turn started by input typed directly into the pane — and
    `thread_id` the thread it runs on (`null` while idle). Only an echo match
    names a send, so an adapter-backed session leaves `send_id` `null` on every
    turn: correlate its running indicator by `thread_id`.
  - `permission` is the **head** of the session's pending tool-permission queue
    — the dialog to show — or `null` when nothing is pending. Answer it with
    [`POST /api/permissions/{id}/decision`](#post-apipermissionsiddecision).
    `tool_input` is the tool's input serialized as JSON *text*.
  - `permission_count` is how many permission requests are pending in total,
    the head included (`0` when `permission` is `null`). It exceeds 1 when a
    provider raises several approvals at once — an adapter-backed provider runs
    tool calls in parallel, so one turn can leave N requests outstanding. The
    queue is FIFO and surfaces one dialog at a time: see
    [the queue semantics](#the-pending-permission-queue).
  - `question` is the `AskUserQuestion` currently presenting its options, or
    `null`. `tool_input` is the raw `{"questions":[…]}` payload as JSON text.
  - `running_subagents` lists the `Agent`/`Task` calls still running, oldest
    first; empty when none is. `background` is `true` for a
    `run_in_background` launch, which can outlive the turn that started it.

  A `queued` send may carry `restored_at` (see `Send` in
  [shapes.md](shapes.md#send)): it was recovered at boot from a dead
  server process's `dispatched` state and never auto-dispatches — the browser
  offers explicit Send
  ([`POST /api/sends/{id}/release`](#post-apisendsidrelease)) and Cancel actions
  on such a row instead of the waiting label.

- **404** — no session with that id, so a reaped spawn is distinguishable from
  "nothing pending".

### `POST /api/sends/{id}/cancel`

Abandon a send before its text reaches the transcript.

A send composed while a turn is in flight stays `queued` until the session goes
idle; cancelling drops it before that dispatch. A `dispatched` send the turn
machine is still awaiting (its echo never arrived — typically the user pressed
`Escape` in the TUI, discarding the composer buffer with no signal Delta can
observe) is cancelled by injecting a single `Escape` into the pane and dropping
the row; any send queued behind it then promotes through the normal idle flush. A
`dispatched` row the turn machine holds no claim on is cancelled as a pure state
transition — no keystroke is injected.

The row flips to `cancelled` in every success case and leaves the open-send list.
No event is broadcast: the browser refetches
[`GET /api/sessions/{id}/sends`](#get-apisessionsidsends) to clear the chip.

- **204 No Content** — the send is cancelled.
- **409** (body `code: "send_not_cancellable"`) — the send no longer exists, is
  already terminal (matched or cancelled), or is `dispatched` with its echo
  already arrived, so the turn carries it in flight and interrupting the turn is
  the right control instead: `Escape` in the TUI for a pane-backed session,
  [`POST /api/sessions/{id}/interrupt`](sessions.md#post-apisessionsidinterrupt)
  for an adapter-backed one (the route is a well-defined no-op on pane-backed
  sessions).

### `POST /api/sends/{id}/release`

Send a *restored* row: one the boot-time reconcile recovered from a `dispatched`
state a dead server process left behind. Such a row comes back as `queued` with
`restored_at` set and never auto-dispatches — the message may be days old, so
re-submitting it silently is deliberately not done.

The release first ensures the owning session is open (resuming it when closed —
the normal state right after the restart that created the row), then clears the
`restored_at` marker with a guarded update (so a race against a cancel is a clean
conflict) and runs the session's ordinary queued dispatch. If the session was
already open and idle the row types immediately and `send_dispatched` is
broadcast; if the release resumed the session the row is typed by the
resume-settle flush; otherwise (mid-turn) it waits as an ordinary queued send.

The sibling Cancel action is
[`POST /api/sends/{id}/cancel`](#post-apisendsidcancel) — a restored row's status
is still `queued`, so the guarded queued cancel already covers it.

- **204 No Content** — the row was released.
- **409** (body `code: "send_not_releasable"`) — the send is unknown, was never
  restored, is already released, or has since been cancelled.
- **409** (body `code: "resume_unavailable"`) — the session had to be resumed and
  its transcript is gone. The marker is untouched, so the release can be retried.

## Permissions

### The pending-permission queue

A session's outstanding permission requests form a FIFO queue, and the surfaces
above report it as **head plus depth**: `permission` is the request to show,
`permission_count` how many are pending in total.

- **Several can be pending at once.** A pane-backed provider (Claude) blocks its
  CLI inside the permission hook, so its queue never holds more than one entry.
  An adapter-backed one (Codex) runs tool calls in parallel: a single turn can
  emit N approval requests in the same instant, and all N are recorded and
  answerable.
- **Arrival never displaces the head.** A request that arrives while others are
  pending queues *behind* them, so the dialog a user is looking at is never
  swapped out from under them. Every request is broadcast as its own
  `permission_requested` ([live-channels.md](live-channels.md)), so a client that
  follows events can rebuild the same order.
- **Decisions are keyed by row id, not by queue position.**
  [`POST /api/permissions/{id}/decision`](#post-apipermissionsiddecision) answers
  any pending request, head or not. Resolving a non-head request removes only
  that entry and leaves the visible dialog alone.
- **Answering the head promotes the next.** The resolution broadcasts
  `permission_resolved` for the answered request and, when the queue is still
  non-empty, re-broadcasts `permission_requested` for the newly promoted head —
  so a client that only follows events always has a dialog on screen while
  requests are pending, with no refetch.
- **A refetch is always enough to keep answering.** The head and the depth are
  queryable state, so a client that missed every event rebuilds the dialog *and*
  its "N approvals pending" indication from one
  [`GET /api/sessions/{id}/sends`](#get-apisessionsidsends). Only the head's id
  is reported, though — the queued requests' ids come from their
  `permission_requested` events alone. So a client that missed those answers the
  queue front to back, one promoted head at a time, which drains it either way.
- **The queue cannot outlive its turn.** When the turn returns to idle (stop,
  interrupt, close) the whole queue is dropped: the provider has settled or
  abandoned those requests by then, and Delta only drops its mirror.
- **A session whose agent process dies settles the whole queue.** An
  adapter-backed provider's process can go away mid-turn (a killed
  `codex app-server`), and its pending requests can then never be answered — no
  decision can be written to a connection that no longer exists. So the queue is
  settled in one pass: one `permission_resolved` per request (so a live client's
  dialog clears with no refetch, and no promoted head is raised), and each row is
  recorded **denied** with the reason "the agent session ended before this request
  could be answered" — the tool never ran, no row is left pending, and the audit
  trail still distinguishes it from a user's Deny. The same pass settles the turn
  (`turn_interrupted`) and closes the session (`session_closed`); recovery is the
  next send, which resumes it. A decision that arrives for one of those requests
  is a `409` (see below).

### `POST /api/permissions/{id}/decision`

Answer a pending tool-permission request from the browser. `{id}` is the
`request_id` reported by the `permission_requested` event or by the `permission`
field of [`GET /api/sessions/{id}/sends`](#get-apisessionsidsends). Any pending
request can be answered, whether or not it is the queue head (see
[the queue semantics](#the-pending-permission-queue)).

For a pane-backed session, resolving the row wakes the blocked
[`POST /hooks/permission-request`](hooks.md#post-hookspermission-request) call,
which carries the decision back to the agent — so the tool proceeds (or is
denied) without anyone touching the TUI prompt. An adapter-backed one has no
hook to wake: its decision goes back over the provider connection instead.

Request:

```json
{ "decision": "allow" }
```

`decision` is `allow` or `deny`.

- **204 No Content** — the decision was recorded and handed to the agent. When
  the answered request was the queue head and others are still pending, the next
  one is raised as a fresh `permission_requested`.
- **409** (body `code: "permission_not_pending"`) — the request is no longer
  awaiting a browser decision: it was already decided, its hook wait timed out
  and the interactive TUI prompt owns it now, or its adapter-backed session
  ended (closed, or its agent process died and the settle resolved every
  pending request — see
  [the queue semantics](#the-pending-permission-queue)). A retry of a decision
  that already failed downstream (the **500** a dying agent connection can
  produce) answers the same 409: the server's claim on the request is taken
  before the decision is routed and is never restored, so the failed attempt
  already spent it. The browser replaces the decision buttons with guidance
  chosen by the provider's `has_terminal` capability (see
  [settings.md — `GET /api/providers`](settings.md#get-apiproviders)): a session
  that has a terminal is pointed at the prompt waiting there, while a
  terminal-less one — where the question survives nowhere the user can reach —
  is told it can no longer be answered, and offered only Dismiss.

## Questions

An `AskUserQuestion` tool call presents its options in the session's TUI. A CLI
hook cannot return the user's pick, so both routes below work by injecting the
exact keystrokes into the session's live pane. Neither broadcasts an event: the
eventual `tool_result` resolves the question's request row through the normal
transcript sync, which clears the card over the same `permission_resolved` path a
terminal-answered question takes.

### `POST /api/sessions/{id}/questions/{request_id}/answer`

Answer the pending question. `{request_id}` is the row id carried by the
`question_asked` event and by the `question` field of
[`GET /api/sessions/{id}/sends`](#get-apisessionsidsends).

Request:

```json
{ "selections": [[0], [2, 1]] }
```

`selections[q]` lists the 0-based option indices chosen for question `q`, in
question order — exactly one index for a single-select question, one or more for
a multi-select one.

- **204 No Content** — the keystrokes were injected.
- **400** — the selection could not be turned into a key sequence (out of range,
  or an unsupported combination).
- **409** (body `code: "question_not_pending"`) — the question is no longer
  pending: already answered, its turn ended, or the session has no live pane. The
  browser falls back to answering in the terminal.

### `POST /api/sessions/{id}/questions/cancel`

Cancel the pending question by injecting a single `Escape` into the session's
pane, which cancels the whole call; the TUI then writes an `is_error`
`tool_result`.

Unlike an answer, a cancel carries no selection — the only datum is which
question to cancel — so the `request_id` rides in the body rather than the path.

Request:

```json
{ "request_id": 5 }
```

- **204 No Content** — the `Escape` was injected.
- **409** (body `code: "question_not_pending"`) — the question is no longer
  pending: already answered or cancelled, its turn ended, or the session has no
  live pane.
