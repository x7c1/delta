# Shared JSON shapes

## Overview

The JSON shapes shared across the API surfaces: `Session`, `Thread`,
`Message`, `ContentBlock`, and `Send`. The endpoint documentation in
this directory ([sessions.md](sessions.md), [sends.md](sends.md),
[live-channels.md](live-channels.md), [hooks.md](hooks.md)) refers to these by
name. Conventions (timestamps, id types, error bodies) are in
[README.md](README.md).

## `Session`

```json
{
  "id": "sess-1",
  "cwd": "/work/delta",
  "transcript_path": "/path/to/transcript.jsonl",
  "title": null,
  "status": "active",
  "created_at": "2026-01-01T00:00:00Z",
  "branch_at_launch": "main",
  "repo_root": "/work/delta",
  "repository_display_name": "x7c1/delta",
  "provider": "claude",
  "provider_session_id": null,
  "provider_thread_id": null
}
```

- `status` is one of `spawning`, `active`, `ended`, `failed`.
- `transcript_path` is empty while the session is still `spawning`, before the
  first hook reports it.
- `branch_at_launch`, `repo_root` and `repository_display_name` are spawn-time
  snapshots of the launch directory's git state, all `null` when it was not
  inside a git repository. They are never updated on resume or a later
  `git checkout`. On a Claude worktree spawn `cwd` and `branch_at_launch` are
  the accept-time *plan*: `branch_at_launch` is the branch the launch will put
  that worktree on, and while the session is still `spawning`, `cwd` may name a
  directory that does not exist yet — the launch creates the worktree there,
  unless it reuses one that already holds the branch.
- `provider` is `claude` or `codex`. `provider_session_id` and
  `provider_thread_id` carry the provider's own ids when the provider (not
  Delta) mints them, and are `null` for a Claude session.

## `Thread`

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

## `Message`

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
  "created_at": "2026-01-01T00:00:00Z",
  "model": null,
  "git_branch": "main",
  "cwd": "/work/delta",
  "response_time_ms": null,
  "provider_item_id": null
}
```

- `role` is one of `user`, `assistant`, `system`, `meta` (a harness-injected
  line such as a skill body or system reminder, not a human-authored turn),
  `compact_summary` (the synthetic user line `/compact` writes, carrying the
  prior conversation's summary), or `other`.
- `linear_parent_uuid` is the transcript's model-context parent;
  `semantic_parent_uuid` is the `to:` reply edge, set only on branch messages.
- `content` is an ordered list of content blocks (see below). `content_text` is
  the flattened plain-text view of the text/thinking blocks, or `null`.
- `model` is the model that produced this message (historical, per message),
  `null` for non-assistant lines and shapes that carry no model. Distinct from
  the user's *current* model selection reported by the status line (see
  [hooks.md — `POST /hooks/status-line`](hooks.md#post-hooksstatus-line)).
- `git_branch` and `cwd` are the transcript's per-turn `gitBranch` and `cwd`,
  `null` when absent. `git_branch` can change mid-session (e.g. a `git
  checkout` between turns); `cwd` is effectively fixed for a session's
  lifetime.
- `response_time_ms` is the turn's response time, correlated from the
  transcript's `system`/`turn_duration` line, `null` when no duration was
  recorded.
- `provider_item_id` is the provider's own id for the source item (Codex's
  `item.id`), `null` for Claude and for any message not sourced from a
  provider item. The browser uses it to id-join a streaming preview to its
  final message.

## `ContentBlock`

A tagged union keyed on `type`:

```json
{ "type": "text", "text": "..." }
{ "type": "thinking", "thinking": "..." }
{ "type": "tool_use", "id": "t1", "name": "Bash", "input": { "command": "ls" } }
{ "type": "tool_result", "tool_use_id": "t1", "content": "...", "is_error": false }
```

Any unmodelled block kind is preserved as `{ "type": "other" }`.

## `Send`

```json
{
  "id": 1,
  "session_id": "sess-1",
  "thread_id": 1,
  "semantic_parent_uuid": null,
  "text": "what is a delta?",
  "locator_quote": "the main channel",
  "status": "queued",
  "matched_uuid": null,
  "created_at": "2026-01-01T00:00:00Z",
  "held_at": null
}
```

- `status` is one of `queued` (waiting for the session to go idle), `dispatched`
  (handed to the agent, awaiting correlation), `matched`, or `cancelled`. The
  first two are the *open* statuses [`GET /api/sessions/{id}/sends`](sends.md#get-apisessionsidsends)
  reports. How a row walks that ladder depends on how the provider is driven
  (see [sends.md](sends.md) for the two dispatch paths): an adapter-backed
  session (Codex) goes `dispatched` → `matched` inside the enqueue call, so its
  rows are effectively never observed in an open status.
- `matched_uuid` carries the id the send was correlated with, once it was: the
  uuid of the transcript message it produced for a pane-backed session, the
  provider's own turn id for an adapter-backed one — so it is not always an id
  that [`GET /api/threads/{id}/messages`](sessions.md#get-apithreadsidmessages)
  returns. It is `null` while the send is still `queued` or `dispatched`, and
  stays `null` on a `matched` pane-backed row that was delivered but attributed
  to no message — no user line reached the transcript before that turn ended
  (see [sends.md](sends.md#overview)). So `null` never means "not delivered";
  `status` alone answers that.
- `held_at` marks a `queued` row as **held until the user releases it**: it
  never auto-dispatches and waits for
  [`POST /api/sends/{id}/release`](sends.md#post-apisendsidrelease) (or a
  cancel). Two paths set it, and the row looks the same either way — the boot
  reconcile, recovering a `dispatched` state a dead server process left behind,
  and the [echo deadline's park](sends.md#when-no-echo-ever-arrives), for a send
  whose keystrokes vanished twice running. `null` on the normal send path.
