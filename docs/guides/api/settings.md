# Providers, launch options, prompt templates and version (`/api/*`)

## Overview

The REST routes behind the Settings screen and the provider selector: which
agent providers this host can launch and what each of them can do, the registry
of custom launch options a session can be started with, the registry of prompt
templates the composer inserts from, and the server's own version string for the
browser footer. Applying a launch option to a session is part of a
`new_session` send ([sends.md](sends.md#post-apisends)); conventions and error
semantics are in [README.md](README.md).

## Providers

### `GET /api/providers`

Report the launch availability and capability profile of every known agent
provider (Claude, Codex). The new-session provider selector disables an
unavailable provider and shows the reason, so a user cannot pick a provider that
would fail at spawn; the workspace reads the capability profile to gate
provider-specific surfaces (the terminal pane vs the comms-log pane, and the
vocabulary the Settings launch-option form tells the user to write in). Always
**200**: a missing binary is data (`available: false`), never an error.

- **200**:

  ```json
  {
    "providers": [
      {
        "provider": "claude",
        "available": true,
        "detail": null,
        "capabilities": {
          "has_terminal": true,
          "has_comms_log": false,
          "has_allow_for_session": false,
          "launch_option_style": "cli_flag"
        }
      }
    ]
  }
  ```

  - `available` reports whether the provider's configured launch binary is
    present on the server host (binary presence only). `detail` carries a
    human-readable reason when `available` is `false`, `null` otherwise.
  - `capabilities` is the provider's static, UI-relevant capability profile —
    present even for an unavailable provider:
    - `has_terminal` — the provider offers a terminal the browser can attach
      to; its sessions get the terminal pane (`/pty`).
    - `has_comms_log` — the browser can inspect the frames Delta exchanges
      with this provider; its sessions get the comms-log pane (`/comms`).
      Complementary with `has_terminal`, not independent — see
      [live-channels.md](live-channels.md).
    - `has_allow_for_session` — the provider understands a permission decision
      scoped to the whole session (`allow_for_session`), not just the one
      request being answered; its approval notices offer that extra button.
      Sending the value to a provider whose flag is `false` is a
      `400 permission_decision_unsupported` (see
      [sends.md](sends.md#post-apipermissionsiddecision)), so a client that
      cannot resolve the capability must not offer the control.
    - `launch_option_style` — how the provider reads a registered launch
      option's `(name, value?)` pair: `cli_flag` (`name` is a command-line
      flag, e.g. `--permission-mode`) or `request_field` (`name` is a field of
      the provider's session-start request, e.g. Codex's `model`).

## Launch options

A launch option is a flat `(label?, name, value?)` record naming one custom
agent startup setting. `name` and `value` are read in the provider's own
vocabulary — a CLI flag and its argument for Claude (`--plugin-dir
/opt/plugins`), a session-start request field and its value for Codex (`model`
= `gpt-5`) — which is what `launch_option_style` above tells the form to ask
for. The registry is provider-scoped: the session-start picker only offers
options whose `provider` matches the session being started.

Most rows are the user's own. Some are **built in**: Delta declares a short
catalog of the combinations in daily use per provider (Claude `--model opus`,
Codex `approvalsReviewer auto_review`, …) and materializes it into the registry
at startup, so those rows are already there the first time Settings is opened —
and come back by themselves after a database reset. A built-in is an ordinary
row in every way that matters to a client: an ordinary `id` that a session-start
selection carries like any other. What differs is ownership of its content —
`label`, `name` and `value` come from Delta's catalog — so it cannot be deleted
(`409`, below), while its `default_enabled` flag is the user's to set like any
other row's. `builtin` marks these rows. Building on one means duplicating it
into a row of your own; there is no endpoint for editing, adding to or hiding
the catalog.

### `GET /api/launch-options`

List the registered launch options for the Settings screen to manage: the rows
Delta ships first, in catalog order, then the user's own newest first. The
leading built-in block is fixed-length, so a built-in's position never moves as
the user adds or removes their own rows.

- **200**:

  ```json
  {
    "launch_options": [
      {
        "id": 1,
        "label": "plugins",
        "name": "--plugin-dir",
        "value": "/opt/p",
        "default_enabled": true,
        "created_at": "2026-01-01T00:00:00Z",
        "provider": "claude",
        "builtin": false
      }
    ]
  }
  ```

  `label` and `value` are `null` when absent (a valueless option carries no
  `value`). `default_enabled` marks the option to start pre-checked in the
  session-start picker. `provider` is `claude` or `codex`. `builtin` is `true`
  for a row Delta ships and `false` for one the user registered; the catalog key
  behind a built-in is internal and never on the wire.

### `POST /api/launch-options`

Register a launch option.

Request:

```json
{
  "label": "plugins",
  "name": "--plugin-dir",
  "value": "/opt/p",
  "default_enabled": true,
  "provider": "claude"
}
```

- `name` (required) — what the option is called in the provider's vocabulary.
  Must be non-blank. Validation stops there deliberately: what a name *means* is
  the provider's business, so the server neither parses it nor assumes a flag
  syntax.
- `label` (optional) — a human-friendly note for the row.
- `value` (optional) — the option's argument or value. Omit it for a valueless
  option.
- `default_enabled` (optional, default `false`) — start the option pre-checked.
- `provider` (optional) — `claude` or `codex`. Omitted means `claude`, keeping
  clients that predate per-provider launch options working unchanged.

`label` and `value` are stored verbatim apart from trimming surrounding
whitespace; an all-blank optional is treated as absent rather than as an empty
string.

- **201 Created** — the created record, so the client can render it without a
  refetch:

  ```json
  {
    "id": 1,
    "label": "plugins",
    "name": "--plugin-dir",
    "value": "/opt/p",
    "default_enabled": true,
    "created_at": "2026-01-01T00:00:00Z",
    "provider": "claude",
    "builtin": false
  }
  ```

  `builtin` is always `false` here: anything registered through this endpoint is
  the user's own row. There is no way to create a built-in.

- **400** — a blank `name`.

### `PATCH /api/launch-options/{id}`

Set a registered option's `default_enabled` flag in place. Updating in place
preserves the option's `id` and `created_at` (a delete-and-recreate would churn
both); `name`, `value`, `label` and `provider` are immutable through this
endpoint.

This applies to a built-in exactly as it does to the user's own row: ticking
`default_enabled` on a shipped option is the point of shipping it, and it is the
one field of a built-in that is not Delta's.

Request:

```json
{ "default_enabled": false }
```

- **200** — the updated record, in the same shape as the create response.
- **404** — no launch option with that id.

### `DELETE /api/launch-options/{id}`

Remove a registered launch option.

- **204 No Content** — the option is gone. Deleting an unknown id is a no-op, so
  this is idempotent and never answers `404`.
- **409 Conflict** (`code: launch_option_builtin`) — the option is **built in**
  (`builtin: true`) and stays registered. Delta's declared catalog owns those
  rows, so a removed row would simply reappear at the next startup; a built-in
  that does not suit is left unticked, and registering your own row is the
  supported way to differ.

## Prompt templates

A prompt template is a `(label, text)` record naming one reusable block of
instruction text — the long instructions a user would otherwise retype or paste
into the composer ("once CI is green, merge and then update the plan doc…").
`label` names it in the picker; `text` is what gets inserted into the composer at
the cursor.

Unlike launch options the registry is **global**: the text is prose addressed to
whichever agent is driving the session, not argv or a session-start request
field, so there is no `provider` and the same template is offered on every
session. Delta never interprets the text — there are no placeholders and no
variable expansion.

`text` is stored verbatim, including leading and trailing whitespace and
newlines: a template may deliberately end with a newline, and that is exactly
where insertion makes it matter. Trimming happens only to decide whether a
submitted `label` or `text` is blank.

### `GET /api/prompt-templates`

List the registered templates, oldest first (`created_at` ascending, `id`
ascending on ties). Registration order is stable, so editing a template never
moves it in the list.

- **200**:

  ```json
  {
    "prompt_templates": [
      {
        "id": 1,
        "label": "Merge when green",
        "text": "Once CI is green, merge and then update the plan doc.\n",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-02T00:00:00Z"
      }
    ]
  }
  ```

  `updated_at` equals `created_at` until the template is first edited.

### `POST /api/prompt-templates`

Register a prompt template.

Request:

```json
{
  "label": "Merge when green",
  "text": "Once CI is green, merge and then update the plan doc.\n"
}
```

- `label` (required) — what the template is called in the picker. Must be
  non-blank.
- `text` (required) — the body inserted into the composer. Must be non-blank.

Response:

- **201 Created** — the created record, so the client can render it without a
  refetch:

  ```json
  {
    "id": 1,
    "label": "Merge when green",
    "text": "Once CI is green, merge and then update the plan doc.\n",
    "created_at": "2026-01-01T00:00:00Z",
    "updated_at": "2026-01-01T00:00:00Z"
  }
  ```

- **400** — a `label` or `text` that is empty or nothing but whitespace. A body
  that is *surrounded* by whitespace is not blank and is accepted as written.

### `PATCH /api/prompt-templates/{id}`

Replace a registered template's content in place. Both fields are required —
this is a full replacement of the editable content, not a partial patch, so a
client cannot blank one by omitting it. The template's `id` and `created_at` are
preserved (a delete-and-recreate would churn both and move the row to the end of
the list), and `updated_at` is re-stamped.

Request:

```json
{ "label": "Merge when green", "text": "Merge once CI is green.\n" }
```

- **200** — the updated record, in the same shape as the create response.
- **400** — a blank `label` or `text`, as on the create.
- **404** — no prompt template with that id.

### `DELETE /api/prompt-templates/{id}`

Remove a registered prompt template.

- **204 No Content** — the template is gone. Deleting an unknown id is a no-op,
  so this is idempotent and never answers `404`.

## Server version

### `GET /api/version`

Return the Delta workspace version, pre-formatted for the browser footer. The
server owns the format so the browser never has to parse the base version and
the commit apart: release builds answer `v<version>`, debug builds
`v<version>+dev.<short-sha>` (`+dev` is SemVer build metadata, deliberately not
the `-dev` pre-release form).

- **200**:

  ```json
  { "version": "v0.2.1" }
  ```
