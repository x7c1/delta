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
  through the transcript (and released by the
  [echo deadline](#when-no-echo-ever-arrives) when it never does). This is the
  path with a real `queued` stage: only one send may be outstanding per turn, so
  anything composed mid-turn waits. When Claude Code rewrites the prompt before
  recording it (a folded slash command, an `[Image #N]` prefix), the first user
  line of that turn still counts as the send's echo whatever it says: the send
  is never typed a second time, and the row is matched to that line's uuid.
  `matched_uuid` is left `null` only when no user line reaches the transcript
  before the turn ends — the row still settles `matched` at that turn's end:
  delivered, attributed to no message.
- **Adapter-backed (Codex)** — no pane, no keystrokes: the text rides a
  turn-start request on the `codex app-server` connection and is matched to the
  turn id that request returns, so the row goes `dispatched` → `matched` within
  the same call. The rows that do sit `queued` first are the ones a **new
  session** accepts while it is still starting: its first prompt, and anything
  composed after it before the launch binds. They are written when the send is
  accepted, before the provider thread exists; the first prompt reaches its
  turn-start call once the background launch has started the thread, and each
  row behind it goes out when the turn ahead of it ends (see the `201` under
  [`POST /api/sends`](#post-apisends)).

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
  "worktree": { "start_point": { "kind": "head" } },
  "pull_request_number": 138
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
- `pull_request_number` (optional) — the GitHub pull request the session is
  being opened from. Stored as a spawn-time snapshot on the session row (see
  [shapes.md — `Session`](shapes.md#session)) and never updated afterwards.
  Omitted for a session started from anywhere but the PR tab.

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

  The `201` is returned **before the launch**, for every provider: the session
  is *accepted*, not started. Building the git worktree (a `git fetch` plus a
  full checkout, seconds to tens of seconds on a large repository) and then
  standing the agent up — for `claude`, seeding workspace trust and launching
  the tmux pane; for `codex`, connecting to the provider (`codex app-server`
  plus its handshake) and starting its thread — all run in the background
  afterwards, so the response time no longer depends on the size of the
  repository or on how long the provider takes to come up. What stays
  synchronous is everything cheap and everything whose failure is the caller's
  to fix — validating `workdir`, the worktree gate, resolving
  `launch_option_ids` (and, for an adapter-backed provider, having its adapter
  vet them), and reading the repository's local git config — which is why those
  are still the `400`s below. The session is listed as `spawning` from
  this response until its launch binds it. A plain send arriving in that window
  is **accepted as a `queued` row** — nothing can be dispatched into a session
  with no pane and no provider thread, but the message is recorded and typed
  the moment the launch binds, so a user watching a slow checkout is not made
  to wait to compose their next message. A *branch* send is the one shape
  refused there (`session_spawning`, see the `409` below): the session has
  ingested no message to branch from.

  A failure of the background launch — a start point that does not exist on
  the remote, a `git worktree add` error, a tmux failure, a provider that will
  not connect or start a thread — therefore cannot be a response at all. It
  arrives on the live channel as a
  [`spawn_failed`](live-channels.md#session-lifecycle) event carrying the error
  text as its `reason`, and the eagerly-created row (with every send of the
  session, by cascade) is deleted, so the session stops being listed. Because
  those rows go, that event also carries `unsent`: the id and text of every send
  the launch never delivered, first prompt included, so a client can put the
  messages back in front of the user. Nothing is re-sent server-side. The
  preparation is also given up on if it has not finished within 10 minutes — a
  `git fetch` hanging on an unreachable remote or a credential prompt has no
  timeout of its own — and that gives the same `spawn_failed`, so a stuck
  session never sits `spawning` indefinitely. That deadline is overridable with
  `DELTA_LAUNCH_PREP_DEADLINE_MS` (milliseconds), the way the echo watchdog's is
  with `DELTA_ECHO_DEADLINE_MS`.

  One more failure belongs to this window. The launch directory is decided when
  the send is accepted (it is what the session row records as its `cwd`), and
  for a `use_remote_branch` start point that decision is "reuse the worktree
  already holding the branch, else create one" — so starting a second session on
  the same branch while the first is still checking it out can plan a path the
  build then never creates, because it finds the first session's worktree
  instead. Git forbids one branch in two worktrees, so there is nothing to
  build at the planned path and nothing to re-point the persisted `cwd` at: the
  launch fails with a `spawn_failed` whose `reason` names the branch and both
  paths. Retrying the send re-decides against the worktree that now exists and
  starts there.

  For `provider: "codex"` the first prompt sits in the session's send list as
  `queued` until the provider thread exists, then dispatches: unlike a Claude
  spawn — where the prompt rides on the launch command line, so launching *is*
  delivering it — there is nothing to hand it to until `thread/start` has
  answered.

- **400** — the target is ambiguous or contradictory (a JSON body): neither
  `thread_id` nor `new_session` given, both given, or `new_session` combined with
  a branch (`semantic_parent_uuid`). Also a `workdir` that does not exist or is
  not a directory, a `worktree` requested without a `workdir` or for a directory
  that is not a git repository, a non-positive `pull_request_number` (pull
  requests are numbered from 1), and a selected launch option the provider's
  adapter refuses (body `code: "launch_option_rejected"` — it names a field
  Delta sets itself, names the same field twice, or two selected Codex `config`
  rows disagree about one setting inside the object they merge into; the message
  names the offending key or key paths). A malformed body or a missing required
  field such as `text` is rejected earlier as one of the framework-level
  `400`/`415`/`422` cases in [README.md](README.md).

  The `launch_option_rejected` case is answered **before the session row is
  written**, even though it is the provider's adapter — not Delta — that decides
  it: rendering the selections is a pure function of the request, so the accept
  phase asks the adapter about them without connecting. No session is created
  and torn down again.
- **404** — no thread (or branch parent thread) with the given `thread_id`.
- **409** — the target's session is closed and cannot be resumed because its
  transcript is gone (body `code: "resume_unavailable"`). No send is enqueued and
  the session stays closed.
- **409** (body `code: "session_spawning"`) — a **branch** send
  (`semantic_parent_uuid`) whose target session is still starting: it is listed
  from the moment its first send was accepted, but its launch has not bound yet,
  so it has ingested no message to branch from. This covers the whole starting
  window, for either provider — the background launch and, for `claude`, the
  wait for the launched agent's first hook on top of it. No send is enqueued;
  the same request succeeds once the launch has bound the session (it is then
  `active`, announced as `session_registered`).

  A **plain** send to a starting session is not refused: it is accepted as a
  `queued` row (`201`) and dispatched when the launch binds — at the end of the
  turn the first prompt opens, for either provider, or immediately at the bind
  when no such turn starts (the spawn carried no first prompt, or that prompt
  was cancelled before the launch bound).

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
    `tool_input` is the tool's input serialized as JSON *text*. It also carries
    the optional `file_change` detail and `grant_root` the
    [`permission_requested`](live-channels.md#permissions-and-questions) event
    carries, with the same shapes and the same absent-means-unknown rule, so a
    client that missed the event rebuilds the identical dialog from this
    refetch.
  - `permission_count` is how many permission requests are pending in total,
    the head included (`0` when `permission` is `null`). It exceeds 1 when a
    provider raises several approvals at once — an adapter-backed provider runs
    tool calls in parallel, so one turn can leave N requests outstanding. The
    queue is FIFO and surfaces one dialog at a time: see
    [the queue semantics](#the-pending-permission-queue).
  - `question` is the `AskUserQuestion` currently presenting its options, or
    `null`. `tool_input` is the raw `{"questions":[…]}` payload as JSON text.
  - `running_subagents` lists the subagents still running, oldest first; empty
    when none is. Both launch kinds appear: the model's `Agent`/`Task` calls,
    and the forked skill a slash command runs in the background (whose
    `tool_use_id` is the synthetic `forked-skill:<agentId>` — see
    [`subagent_started`](live-channels.md#streaming-and-subagents)).
    `background` is `true` for a `run_in_background` launch and for every forked
    skill, either of which can outlive the turn that started it.

  A `queued` send may carry `held_at` (see `Send` in
  [shapes.md](shapes.md#send)): the row is **held in the queue until the user
  releases it** and never auto-dispatches, so the browser offers explicit Send
  ([`POST /api/sends/{id}/release`](#post-apisendsidrelease)) and Cancel actions
  on it instead of the waiting label. Two paths set the marker, and the row
  looks the same either way: the boot reconcile (recovering a dead server
  process's `dispatched` row) and the
  [echo deadline's park](#when-no-echo-ever-arrives).

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

Send a *held* row: one that came back to `queued` with `held_at` set,
whether the boot-time reconcile recovered it from a `dispatched` state a dead
server process left behind or the
[echo deadline parked it](#when-no-echo-ever-arrives). Such a row never
auto-dispatches — the message may be days old, or its keystrokes may be
disappearing into whatever swallowed them twice — so re-submitting it silently
is deliberately not done. The user decides.

The release first ensures the owning session is open (resuming it when closed —
the normal state right after the restart that created a restored row), then
clears the `held_at` marker with a guarded update (so a race against a cancel
is a clean conflict) and runs the session's ordinary queued dispatch. If the
session was already open and idle the row types immediately and `send_dispatched`
is broadcast; if the release resumed the session the row is typed by the
resume-settle flush; otherwise (mid-turn) it waits as an ordinary queued send.

The sibling Cancel action is
[`POST /api/sends/{id}/cancel`](#post-apisendsidcancel) — a held row's status is
still `queued`, so the guarded queued cancel already covers it.

- **204 No Content** — the row was released.
- **409** (body `code: "send_not_releasable"`) — the send is unknown, was never
  held, is already released, or has since been cancelled.
- **409** (body `code: "resume_unavailable"`) — the session had to be resumed and
  its transcript is gone. The marker is untouched, so the release can be retried.

### When no echo ever arrives

A pane-backed send is confirmed by its echo: the `UserPromptSubmit` hook coming
back for it, whatever text it reports. Sometimes nothing comes back at all —
Claude Code's TUI raises a dialog between turns and swallows the pasted text
whole, answering itself with the trailing Enter, or someone presses `Escape` in
the attached pane before the prompt submits. There is no signal to react to in
any of those cases:
no user message, no hook, no turn boundary. Left alone, the row would stay
`dispatched` forever behind a permanent "in progress", with everything composed
after it stuck `queued`.

So silence is bounded. A send that has been awaiting its echo for longer than
**60 seconds** is given up on by a background watchdog:

1. **First deadline — one retry.** The turn is released to `idle` and the send
   returns to `queued`, then re-types immediately: a single `Escape` into the
   pane first (dismissing a lingering dialog and discarding any half-landed
   composer draft), then the same text. If whatever swallowed the keystrokes has
   gone, the echo arrives and the send completes normally — the user sees only a
   delayed answer.
2. **Second deadline — parked.** A retry that is swallowed too spends the send's
   retry budget. The row goes back to `queued` with `held_at` set — the same
   held state the boot reconcile produces — so it stays in the open-send list
   with explicit Send ([`POST /api/sends/{id}/release`](#post-apisendsidrelease))
   and Cancel actions, and no automatic trigger ever re-types it. Anything queued
   behind it dispatches on the spot, past the held row. `send_parked`
   ([live-channels.md](live-channels.md)) goes out alongside, so the browser can
   say *why* the row is waiting; the message itself is server state now, and
   survives a reload or a session switch.

The deadline is deliberately far longer than the echo loop ever legitimately
takes, so a slow-but-healthy send is never disturbed; a send re-typed by the
auto-`/compact` recovery restarts the clock and spends no budget. It is
overridable with `DELTA_ECHO_DEADLINE_MS` (milliseconds), which the fake
end-to-end suite uses to exercise the retry-then-park path in seconds.
Adapter-backed (Codex) sessions are unaffected: their sends are matched inside
the turn-start call and never wait for an echo.

A **slash-command** send is normally resolved before that deadline can matter,
by a different route. A command Claude Code handles client-side (`/clear`, a
project command, a name it does not recognise) fires **no `UserPromptSubmit` and
no `Stop`** — the TUI answers it itself. What it does write is a line: the
command's own name line, or an `Unknown command: …` notice. The transcript
ingest consumes the outstanding send with that line positionally — whatever
command name the line ended up recording — so the row goes `matched` and the
degenerate turn it stood for ends there. Without that fold the session would sit
in "awaiting echo" until the watchdog fired and every send behind it would
defer; with it, the deadline never comes up.

Unlike an ordinary echo, that fold is guarded by kind: a command line consumes
the outstanding send only when that send is *itself* a slash command. A plain
send is consumed by its `UserPromptSubmit` the moment it submits, so a command
line arriving while one is outstanding means something else was typed into the
pane — Delta leaves the plain send waiting for its own echo (and, failing that,
for the echo deadline). A slash command whose keystrokes are swallowed outright
(a built-in that opens a TUI dialog, say) writes no line at all, and falls back
to the retry-then-park path above.

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

`decision` is `allow`, `allow_for_session`, or `deny`.

`allow_for_session` permits this request **and** comparable ones for the rest of
the provider's session, so a turn that raises a dozen approvals in a row need not
be clicked through one by one. It is accepted only by a provider whose
`has_allow_for_session` capability is `true` (see
[settings.md — `GET /api/providers`](settings.md#get-apiproviders)) — the scope
is a grant the provider holds inside its own session, and Delta neither records
nor replays it. On Delta's side the request row records only that the tool call
was permitted, exactly as for a plain `allow`.

- **204 No Content** — the decision was recorded and handed to the agent. When
  the answered request was the queue head and others are still pending, the next
  one is raised as a fresh `permission_requested`.
- **400** (body `code: "permission_decision_unsupported"`) — the request is still
  pending, but this session's provider has no meaning for the decision value
  sent: `allow_for_session` against a provider whose `has_allow_for_session` is
  `false`. Deliberately refused rather than downgraded to a plain `allow`, which
  would keep prompting a user who asked to stop being prompted with nothing said
  about why. **Nothing is mutated**: no decision reaches the agent, the row stays
  `pending`, and the same request is still answerable with `allow` or `deny` —
  so the browser drops just the control that produced the error and leaves the
  rest of the card usable.
- **409** (body `code: "permission_not_pending"`) — the request is no longer
  awaiting a browser decision: it was already decided, its hook wait timed out
  and the interactive TUI prompt owns it now, or its adapter-backed session
  ended (closed, or its agent process died and the settle resolved every
  pending request — see
  [the queue semantics](#the-pending-permission-queue)). A retry of a decision
  that already failed downstream (the **500** a dying agent connection can
  produce) answers the same 409: the server's claim on the request is taken
  before the decision is routed and is not restored, so the failed attempt
  already spent it. The **400** above is the sole exception — that verdict is
  reached before the decision is routed anywhere, so the claim is handed back
  and a retry with `allow` or `deny` still succeeds. On this **409** the browser
  replaces the decision buttons with guidance chosen by the provider's
  `has_terminal` capability (see
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
