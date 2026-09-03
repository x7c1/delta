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
via `make gen`), so the *types* the client compiles against cannot drift from
the implementation. This page is written by hand, so the *list* below is instead
held to that union by a `delta-wire` test that fails until every `kind` the enum
serializes appears here — presence only: what a bullet claims about an event is
not machine-checked.

The stream is process-wide: every connected browser receives every event. Almost
every event carries the `session_id` it concerns so the browser can route it to
the right session; the exception is the [repository-clone
events](#repository-clones), which announce a workspace-level job that has no
session behind it and are keyed by repository instead. Which session the user is
looking at (focus) is purely client-side; the server emits no focus event. There
is no client→server message protocol on this socket.

Nothing here is replayed, and delivery is not guaranteed. A socket that connects
mid-session receives only what happens from then on, and a subscriber that falls
behind is skipped forward rather than allowed to apply back-pressure: the server
holds a bounded buffer per subscriber, logs a warning when one lags, and drops
what it missed while leaving the socket open. So a client must treat these
events as prompts to update state it can also read, and rebuild after any gap —
a fresh connect, a reconnect, a lag — from
[`GET /api/sessions/{id}/sends`](sends.md#get-apisessionsidsends), which returns
the open sends plus the queryable live state (turn phase and thread, the pending
permission queue's head and depth, pending question, running subagents), and from
[`GET /api/sessions`](sessions.md#get-apisessions), whose per-session `open` flag
says what a missed `session_registered` / `session_opened` / `session_closed`
would have said. What no refetch can rebuild is the little that is never
persisted: a streaming preview, the latest `status_updated` snapshot, and the
*explanation* a `send_parked` carries — for status the client simply shows
nothing until the provider's next report, and a missed `send_parked` still
leaves its message in the open-send list, held for an explicit release, just
without the note saying why it is waiting. A missed `spawn_failed` leaves the
same kind of hole: the failed spawn's row is deleted — whether its launch
preparation failed or it came up and never bound — so the session stops being
listed in `GET /api/sessions`, observable on a refetch but with nothing to say
it was a failure. The `reason` a failed preparation reported goes with it, and
so does `unsent` — the text of every send that session had accepted but never
delivered: both ride the event alone, so no refetch can recover them. Delta's
own browser, which focused that session the moment its send was accepted, holds
no launch deadline of its own, so it keeps waiting on a session that is gone
instead of raising the Retry / Dismiss card; picking another session (or
reloading) is the way out.

The groups below are a reading aid only: they say nothing about the order in
which frames arrive, and a client must handle each event whenever it lands.

### Session lifecycle

```json
{ "kind": "session_registered", "session_id": "sess-1" }

{ "kind": "session_opened", "session_id": "sess-1" }

{ "kind": "session_closed", "session_id": "sess-1" }

{ "kind": "spawn_failed", "session_id": "sess-1", "pane_token": "delta-1",
  "reason": "git error: invalid reference: origin/nope", "cancelled": false,
  "unsent": [ { "send_id": 1, "text": "kick off a new conversation" },
              { "send_id": 2, "text": "and one more while it starts" } ] }

{ "kind": "spawn_failed", "session_id": "sess-2", "unsent": [], "cancelled": false,
  "reason": "agent error: failed to spawn app-server: No such file or directory (os error 2)" }

{ "kind": "spawn_failed", "session_id": "sess-3", "pane_token": "delta-3",
  "reason": "closed while starting", "cancelled": true,
  "unsent": [ { "send_id": 7, "text": "kick off a new conversation" } ] }
```

- `session_registered` — emitted when a freshly-spawned session's launch binds
  it: its row flips from `spawning` to `active`. Both providers emit it, at
  their own bind: a Claude session's first hook, a Codex session's
  `thread/start` returning and the adapter being held as the session's agent.
  The session was already listed (and focusable) before this — it is listed from
  the moment its first send was accepted, see [sessions.md](sessions.md) — so
  this is not the moment it becomes visible, it is the moment it becomes
  *usable*: only now is an agent bound to it, so a branch send to it stops
  being refused with `session_spawning`, and any plain send accepted as a
  `queued` row while it was starting becomes dispatchable. It also doubles as
  the "opened" signal for such a session, which never emits a separate
  `session_opened`.
- `session_opened` — a known, previously-closed session became live again
  (resumed by id via `POST /api/sessions/{id}/open`). A brand-new session never
  emits this.
- `session_closed` — an open session was closed (`POST /api/sessions/{id}/close`);
  its pane was torn down but its data remains. It is also emitted when the session
  closed *itself*: an adapter-backed provider whose process ended unexpectedly
  settles (`turn_interrupted` for an in-flight turn, `permission_resolved` for
  every pending request) and then reports the close, so a watching browser
  converges from events alone — see
  [sessions.md](sessions.md) for the recovery story. A close that instead
  *cancelled* a still-starting launch
  ([sessions.md](sessions.md#post-apisessionsidclose)) emits it too, right after
  the `spawn_failed` that reports the cancellation — and there the data does not
  remain, the row is gone. That order is deliberate, so a client that refetches
  the session list (and that session's open sends) on this event has already
  been told the session is finished and needs no special case.
- `spawn_failed` — a freshly-spawned session never came up, for **any**
  provider. Four producers emit it: the background launch when it fails (the
  worktree build, including one that landed on a path other than the one planned
  at accept time; for Claude the trust seed and the tmux launch; for an
  adapter-backed provider the connect and the `thread/start`; or the whole
  sequence outrunning its deadline — all of which run *after* the send was
  accepted, see [sends.md](sends.md)), the
  `SessionEnd` hook when a Claude launch exited while still unbound, the
  watchdog reaper when a launched Claude spawn outlived its bind deadline
  without ever registering, and `POST /api/sessions/{id}/close` on a session
  that is still starting, which cancels the launch (see
  [sessions.md](sessions.md)) — the one producer the user asked for. The
  contentless row is deleted, so the session stops being listed; without the
  event a launch that failed, crashed or hung on auth would leave the browser
  sitting on a session that silently vanished.

  `session_id` is the Delta-minted id the browser correlates with the session it
  focused on acceptance (and with its pending chip); it is the only key a client
  matches on. `pane_token` names the tmux session that was torn down, and is
  **absent entirely** for an adapter-backed (Codex) launch, which never had a
  pane. `reason` carries the cause when Delta can name it — the launch's error
  text (the only place that text reaches the user now that the send is accepted
  before the launch runs), or, for a cancelled launch, that the session was
  closed while starting; it is **absent entirely** from the two watchdog-shaped
  producers' frames, since a launch that exited or never bound says nothing
  about why. A client shows it as an extra line under its own headline wording
  and renders that wording alone when it is missing. That text is prose
  for display, not a code to match on: `cancelled` is what a client keys on to
  tell the two apart. It is `true` only for the close the user asked for and
  `false` for the three producers that report a launch which broke on its own,
  always present, and the only difference between the two frames. A client that
  wants to word a cancellation neutrally (Delta's browser says "Launch
  cancelled" and drops the error colouring, keeping the same Retry / Dismiss
  actions) branches on it.

  `unsent` lists every send the session had accepted but never delivered to an
  agent — the first prompt included — oldest first, each as its `send_id` and
  the `text` the user composed. It is always present (`[]` when the spawn had
  nothing outstanding), so a client reads it without a presence check. It exists
  because the deleted row takes its `send` rows with it: a session accepts sends
  as `queued` rows for as long as it is starting (see [sends.md](sends.md)), and
  this frame is the last place their text exists. **Nothing is re-sent.** A
  client puts the messages it does not already hold back in front of the user —
  Delta's browser appends them to the new-session composer draft, excluding the
  entry whose `send_id` is the spawn's own first prompt (its Retry chip already
  holds that one) — and the user decides whether to send them again. A client
  that holds nothing for the id restores the whole list, first prompt included:
  Delta's browser tracks its spawns in memory only, so a reload (or a second
  tab) meets this frame with no chip to hold anything back, and then raises the
  failure itself, since there is no chip to carry the `reason` either.

### Sends and turns

```json
{ "kind": "send_dispatched", "session_id": "sess-1", "send_id": 42 }

{ "kind": "send_parked",
  "session_id": "sess-1",
  "send_id": 42,
  "text": "read this" }

{ "kind": "turn_started",
  "session_id": "sess-1",
  "send_id": 42,
  "thread_id": 2,
  "matched_uuid": "uuid-1" }

{ "kind": "external_input", "session_id": "sess-1", "prompt": "typed in pane" }

{ "kind": "turn_completed",
  "session_id": "sess-1",
  "thread_id": 2,
  "stop_reason": null }

{ "kind": "turn_interrupted", "session_id": "sess-1", "thread_id": 2 }

{ "kind": "transcript_updated",
  "session_id": "sess-1",
  "thread_ids": [1, 4] }
```

- `send_dispatched` — a held (`queued`) send was promoted to `dispatched` and
  its keystrokes typed: the session went idle and the send took its turn. Lets
  the client refetch the open-send list at that transition instead of waiting
  for the next turn-lifecycle event. Pane-backed sessions only: an
  adapter-backed one (Codex) dispatches each send as it arrives (see
  [sends.md](sends.md) for the two dispatch paths), so it has no
  queued→dispatched transition to announce.
- `send_parked` — a dispatched send was given up on and put back in the queue,
  held for an explicit release. This is silence, not a mismatch: nothing came
  back at all, so the [echo deadline](sends.md#when-no-echo-ever-arrives)
  returned the send to `queued` to be re-typed once, and that retry vanished
  too. The row now carries `held_at`: it stays in the open-send list with
  explicit Send and Cancel actions, and never auto-dispatches. The event is what
  explains *why* that row is waiting; `text` repeats the composed message so a
  client can name it without refetching the open-send list, which is where the
  message itself now lives. Session-scoped, not thread-scoped. A client that was
  disconnected when the park happened misses the explanation but not the
  message: the next open-send refetch shows the held row. Pane-backed sessions
  only: parking is the echo-correlation path's failure mode, and an
  adapter-backed session matches on the turn id its provider returns instead.
- `turn_started` — a queued send was correlated with a transcript message,
  named by `matched_uuid`. `send_id` is the send that took the turn and
  `thread_id` the thread it took it on, so the running indicator lights on that
  exact thread rather than on the whole session. It is **not** emitted once per
  turn: it fires only when the send's own user line was already in the transcript
  as the prompt hook ran, and usually the line lands later — that turn then
  produces no `turn_started` at all and its `turn_completed` drives the refresh.
  Pane-backed sessions only, too: an adapter-backed session has no prompt hook
  to fire. So never wait for this event to learn that a turn is running; the
  answer that is always there is the `turn` in
  [`GET /api/sessions/{id}/sends`](sends.md#get-apisessionsidsends).
- `external_input` — a prompt that matched no outstanding send. Usually the user
  typed straight into the pane, but a dispatched send whose echo came back
  mangled also lands here: the text does not match, so the prompt looks
  external, while that send returns to `queued` for its one retry (and is parked
  after that). Session-scoped — it names no thread. Pane-backed sessions only:
  an adapter-backed session has no pane to type into and no echo to mismatch.
- `turn_completed` — a response finished (Claude's `Stop` hook, or a headless
  provider's turn-end frame). `thread_id` is the thread whose in-flight turn
  just ended, so the client clears the running indicator on the exact thread
  that ran — and bumps an unread badge when that thread is not focused; it is
  `null` only for the degenerate case of a `Stop` on a session Delta never
  registered. `stop_reason` is whatever the `Stop` hook reported, and `null`
  when the turn end carried no reason.
- `turn_interrupted` — the in-flight turn ended without a `turn_completed`: the
  user interrupted it (Escape / Ctrl-C), or it ended on an API error (a usage or
  rate limit, or any other API failure). Claude's `Stop` hook fires on neither,
  so the transcript tail instead detects the `[Request interrupted by user...]`
  marker line, or a synthetic `isApiErrorMessage` assistant line, and emits
  this, clearing the stuck send without a hook. A headless provider reports it
  straight from its turn-end frame when the turn was interrupted or failed,
  mirroring `turn_completed`'s two paths — and also when its agent process ended
  mid-turn, which produces no turn-end frame at all (see `session_closed`).
  `thread_id` is the interrupted turn's thread, `null` only when no thread is
  resolvable.
- `transcript_updated` — the background tail ingested new transcript lines
  between hooks. Claude Code often flushes the final assistant line to the JSONL
  *after* the `Stop` hook fires, so the hook sync misses it; a ~500ms poll picks
  it up and emits this so the browser refetches the affected `thread_ids`. Unlike
  `turn_completed`/`external_input` it carries no turn semantics — clients must
  only refetch those threads, never mutate the pending-send FIFO or unread
  badges.

### Permissions and questions

```json
{ "kind": "permission_requested",
  "session_id": "sess-1",
  "request_id": 1,
  "tool_name": "Bash",
  "tool_input": "{\"command\":\"rm -i x\"}" }

{ "kind": "permission_requested",
  "session_id": "sess-1",
  "request_id": 3,
  "tool_name": "file_change",
  "tool_input": "{\"itemId\":\"fc_1\"}",
  "file_change": {
    "changes": [
      { "path": "src/lib.rs", "kind": "update", "diff": "@@ -1 +1 @@\n-old\n+new" }
    ],
    "reason": "write access"
  },
  "grant_root": "/repo" }

{ "kind": "question_asked",
  "session_id": "sess-1",
  "request_id": 2,
  "thread_id": 2,
  "tool_input": "{\"questions\":[{\"header\":\"Pick\"}]}" }

{ "kind": "permission_resolved", "session_id": "sess-1", "request_id": 1 }
```

- `permission_requested` — a tool permission prompt is imminent. `tool_name` is
  the tool about to run and `tool_input` its input as JSON **text** (the client
  parses it only for display), so the notice can show what the tool is about to
  do next to its decision buttons. `request_id` is the row
  [`POST /api/permissions/{id}/decision`](sends.md#post-apipermissionsiddecision)
  decides. One per request, so a provider raising several approvals at once (a
  parallel tool-call fan-out) emits several: they form a FIFO queue whose arrival
  order is this broadcast order, and the client shows the first while the rest
  wait. The *same* event is re-emitted for whichever request is promoted to head
  when a resolution retires the previous one, so a client that only follows events
  always has a dialog on screen while approvals are pending; a client already
  tracking the queue treats the repeat as a no-op. See
  [the queue semantics](sends.md#the-pending-permission-queue).
  `file_change` is present **only** when the provider stated what allowing the
  request would write, which lets the client name the affected files instead of
  summarizing `tool_input`: `changes` lists each `path`, its `kind`
  (`add` / `update` / `delete`, or `null` for a kind Delta does not model) and
  its unified `diff`, and `reason` is the provider's own explanation (`null`
  when it gave none). The key is **absent** for every request that carries no
  such statement — every Claude permission, every command execution, and a file
  change whose detail could not be resolved — and the client falls back to
  `tool_input` there. Treat its absence as "nothing is known", never as
  "nothing would change".
  `grant_root` is a **separate and broader** ask, present only when the provider
  requested one: writes anywhere under that directory for the remainder of the
  session, not just the files `file_change` lists. It is independent of
  `file_change` — a request can carry `grant_root` with no `file_change` at all,
  which is what a file-change approval whose detail could not be resolved looks
  like — so render it as its own statement of scope rather than as another entry
  in `changes`. Absent when the provider asked for no root.
- `question_asked` — Claude Code's `AskUserQuestion` tool is presenting a
  multiple-choice question. `tool_input` is the raw `{"questions":[…]}` payload
  as JSON text, which the client parses to render the question card, and
  `thread_id` is the in-flight turn's thread, so the card shows only on the
  thread it belongs to. `request_id` is the row
  [`POST /api/sessions/{id}/questions/{request_id}/answer`](sends.md#post-apisessionsidquestionsrequest_idanswer)
  answers. Unlike a permission request it carries no Allow/Deny, and the
  assistant's preamble text is not available here — Claude flushes it to the
  transcript only after the question is answered.
- `permission_resolved` — a previously-requested permission, or a question, was
  settled. Emitted when the browser decides via
  `POST /api/permissions/{id}/decision` (with any decision value — the event
  carries no decision, only that the request is settled), or when the
  `tool_result` correlated with the open request is ingested — which is also how
  an answer given in the TUI clears the card. An auto-approved tool resolves
  almost immediately, while a genuine prompt yields no result until a human
  answers, so the notice persists until then. It settles exactly the named
  `request_id`: with several approvals pending, the others stay pending and
  answerable, and the next one is raised by the follow-up
  `permission_requested` described above. A session whose agent process ended is
  the one case where *every* pending request is settled at once — one of these
  each, with no promotion, since none of them can be answered any more (see
  [sends.md](sends.md#the-pending-permission-queue)).

### Streaming and subagents

```json
{ "kind": "assistant_streaming",
  "session_id": "sess-1",
  "thread_id": 2,
  "message_id": "msg-7",
  "index": 2,
  "final": false,
  "delta": "hello" }

{ "kind": "subagent_started",
  "session_id": "sess-1",
  "thread_id": 2,
  "tool_use_id": "toolu_01",
  "subagent_type": "general-purpose",
  "description": "Run ls and count entries",
  "background": false }

{ "kind": "subagent_finished",
  "session_id": "sess-1",
  "tool_use_id": "toolu_01" }
```

- `assistant_streaming` — a chunk of the in-flight turn's assistant message,
  delivered while it is still generating and before the transcript has it
  (Claude's `MessageDisplay` hook; a headless provider's streaming deltas).
  `delta` is the increment, not the running text: chunks sharing one `message_id`
  are assembled in `index` order rather than in arrival order, and a repeated
  `index` replaces that slot (the latest delivery wins) — exactly how the
  server's own buffer holds them. A chunk carrying a different `message_id`
  starts a fresh preview, superseding the previous one. `final` is `true` on the
  last chunk of a message — Codex never sends one, because its completed message
  arrives as a persisted transcript message instead. The preview is **not**
  persisted and cannot be id-joined to a transcript message (the `message_id` is
  the hook's or the provider's own item id, never a transcript id): it is
  provisional, attributed to `thread_id`. The client drops it when the turn
  ends (`turn_completed` / `turn_interrupted`), whether or not a `final`
  chunk ever arrived — an interrupted message never gets one — after which the
  persisted assistant message from the transcript sync takes over.
- `subagent_started` — a subagent started running. Its own transcript is never
  tailed, so this is the only live signal that one is running. Two kinds of
  launch produce it: the model calling the `Agent`/`Task` tool, and a **forked
  skill** — the background agent Claude Code itself starts for a slash command
  whose skill runs in the background (e.g. `/review-pr`). A forked skill involves
  no tool call at all, only a `<forked-skill-launch>` element on the command's
  transcript line, so its `tool_use_id` is synthetic: `forked-skill:<agentId>`,
  minted from that payload and used for the finish exactly like a real one. It
  also arrives **outside any in-flight turn**: a slash command fires no echo and
  no `Stop`, so its turn is already over by the time the launch is folded — a
  client must not require a preceding `turn_started`, nor treat "no turn
  running" as licence to drop the entry, or the row goes inert for the minutes
  the skill works. `tool_use_id` is the correlation key to its
  `subagent_finished`, `subagent_type` and `description` are display fields that
  are `null` when the launch carried none (a forked skill reports its skill name
  and the command as those), and `thread_id` is the thread that launched it — a
  background subagent outlives its launching turn, so the client needs the thread
  to keep that thread's running indicator lit (and its unread badge suppressed)
  until the finish arrives. `background` says which lifecycle applies: a
  foreground subagent finishes with its matching `PostToolUse`, while a
  `run_in_background: true` one — and every forked skill, which is always
  background — returns immediately at launch and finishes only when its
  completion is folded during transcript sync (its `<task-notification>`, or
  the parent's own `TaskOutput` retrieval of the result, which suppresses the
  notification), so the client must not sweep it at turn end.
- `subagent_finished` — the subagent correlated by `tool_use_id` finished. It
  carries no thread; the client maps the id back to the `subagent_started` that
  named one. A finish for an id that was never tracked, or was already cleared,
  is a no-op and emits nothing. Not every start is answered by one: when a turn
  ends the server drops its still-running *foreground* entries silently, so a
  foreground subagent whose `PostToolUse` never arrived produces no
  `subagent_finished` — the client must clear its own foreground entries on
  `turn_completed` / `turn_interrupted` or that indicator stays lit forever.
  Background entries survive the turn end and are always closed by an event:
  their own completion, or the sweep that runs when the session is closed or its
  process is found gone, which emits one `subagent_finished` per entry.

### Status

```json
{ "kind": "status_updated",
  "session_id": "sess-1",
  "snapshot": {
    "provider": "claude",
    "model_id": "claude-opus-4",
    "model_display_name": "Opus 4",
    "context_used_percentage": 42.5,
    "context_window_size": 200000,
    "context_current_usage": 85000,
    "total_input_tokens": 90000,
    "rate_limits": [
      { "duration_seconds": 18000,
        "used_percentage": 12.0,
        "resets_at": 1700000000 }
    ],
    "total_cost_usd": 0.1234,
    "current_dir": "/work"
  } }
```

- `status_updated` — the latest usage snapshot observed for a session: selected
  model, context-window occupancy, the account's rate-limit windows, and cost.
  It comes from whichever edge the provider exposes (Claude's `statusLine`
  command, a headless provider's pushed usage notifications) and none of it is
  persisted, so this event is the only way the client learns it. It is a "latest
  value" keyed by `session_id` (each snapshot supersedes the last), not an
  append, and it carries no turn or thread semantics.
  - `provider` names whose account and session the numbers describe. Rate limits
    are scoped to an account × provider, so the client keys them by this and
    never shows one provider's limits while another provider's session is
    focused.
  - Every other field is optional, because a snapshot need not be complete: a
    provider that reports token usage and account limits on separate frames
    emits one event per frame, each stating only what that frame said. A `null`
    therefore reads as "this frame says nothing", never as zero — so a
    token-usage frame cannot clear the windows an earlier account frame
    reported.
  - `rate_limits` distinguishes the two cases: `null` is "no statement", while an
    array replaces the account's windows wholesale — including `[]`, which is
    how a provider says the account has no windows to show. Each window is
    identified by `duration_seconds` (`18000` = 5h, `604800` = 7d) rather than
    by a provider-specific name, which is what lets the client label and pace a
    window it has never seen before; it is `null` when the provider reported a
    window without saying how long it is.
  - `context_used_percentage` is computed by the provider's own edge and
    forwarded verbatim, never recomputed here; it is `null` when the provider
    does not expose enough to say, which reads as "no bar".

### Repository clones

```json
{ "kind": "repository_clone_completed",
  "repo_owner": "x7c1",
  "repo_name": "delta",
  "clone_root": "/home/dev/projects",
  "destination_path": "/home/dev/projects/delta" }

{ "kind": "repository_clone_failed",
  "repo_owner": "x7c1",
  "repo_name": "delta",
  "clone_root": "/home/dev/projects",
  "destination_path": "/home/dev/projects/delta",
  "message": "could not resolve host github.com" }
```

The outcome of a
[`POST /api/repositories/clone`](workdirs.md#post-apirepositoriesclone) job.
That request answers `202` and returns long before the clone finishes, so these
are the only way a client learns what happened.

- **These two carry no `session_id`.** Cloning a repository is a workspace-level
  command with no session behind it, so a client that routes frames by session
  must special-case them rather than assume the field is there. They are keyed
  by `repo_owner`/`repo_name` — the same pair the request named.
- `repository_clone_completed` — the clone exists at `destination_path`. Because
  the clone is renamed onto that path atomically, its existence means a finished
  working tree, never a partial one. The client refetches the PR list and the
  repository list, whose `has_local_clone` and clone rows this flips, and can use
  `destination_path` directly as the working directory without waiting for that
  refetch.
- `repository_clone_failed` — the clone did not happen, and `message` is `gh`'s
  own words for why ("no such repository" and "no network" want different
  reactions from the user, and only the message separates them).
  `destination_path` does **not** exist when this arrives, so retrying is simply
  the same request again.
- `clone_root` is echoed back so a client that offered a choice of roots can tell
  which one the clone went to.
- Both are fire-and-forget like everything else here, and the job registry behind
  them is in-memory only: a client that misses one learns nothing until it
  refetches, and a server restart forgets in-flight jobs outright.

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
