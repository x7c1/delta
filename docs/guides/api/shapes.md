# Shared JSON shapes

## Overview

The JSON shapes shared across the API surfaces: `Session`, `Thread`,
`Message`, `ContentBlock`, and `PendingSend`. Endpoint documentation
([rest.md](rest.md), [live-channels.md](live-channels.md),
[hooks.md](hooks.md)) refers to these by name. Conventions (timestamps, id
types, error bodies) are in [README.md](README.md).

## `Session`

```json
{
  "id": "sess-1",
  "cwd": "/work/delta",
  "transcript_path": "/path/to/transcript.jsonl",
  "title": null,
  "status": "active",
  "created_at": "2026-01-01T00:00:00Z"
}
```

`status` is one of `active`, `ended`.

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
  "created_at": "2026-01-01T00:00:00Z"
}
```

- `role` is one of `user`, `assistant`, `system`, `other`.
- `linear_parent_uuid` is the transcript's model-context parent;
  `semantic_parent_uuid` is the `to:` reply edge, set only on branch messages.
- `content` is an ordered list of content blocks (see below). `content_text` is
  the flattened plain-text view of the text/thinking blocks, or `null`.

## `ContentBlock`

A tagged union keyed on `type`:

```json
{ "type": "text", "text": "..." }
{ "type": "thinking", "thinking": "..." }
{ "type": "tool_use", "id": "t1", "name": "Bash", "input": { "command": "ls" } }
{ "type": "tool_result", "tool_use_id": "t1", "content": "...", "is_error": false }
```

Any unmodelled block kind is preserved as `{ "type": "other" }`.

## `PendingSend`

```json
{
  "id": 1,
  "session_id": "sess-1",
  "thread_id": 1,
  "semantic_parent_uuid": null,
  "text": "what is a delta?",
  "locator_quote": "the main channel",
  "status": "pending",
  "matched_uuid": null,
  "created_at": "2026-01-01T00:00:00Z"
}
```

`status` is one of `pending`, `matched`, `cancelled`. `matched_uuid` is set once
the send is correlated with a transcript message.
