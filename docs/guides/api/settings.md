# Providers, launch options and version (`/api/*`)

## Overview

The REST routes behind the Settings screen and the provider selector: which
agent providers this host can launch and what each of them can do, the registry
of custom launch options a session can be started with, and the server's own
version string for the browser footer. Applying a launch option to a session is
part of a `new_session` send ([sends.md](sends.md#post-apisends)); conventions
and error semantics are in [README.md](README.md).

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

### `GET /api/launch-options`

List the registered launch options, newest first, for the Settings screen to
manage.

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
        "provider": "claude"
      }
    ]
  }
  ```

  `label` and `value` are `null` when absent (a valueless option carries no
  `value`). `default_enabled` marks the option to start pre-checked in the
  session-start picker. `provider` is `claude` or `codex`.

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
    "provider": "claude"
  }
  ```

- **400** — a blank `name`.

### `PATCH /api/launch-options/{id}`

Set a registered option's `default_enabled` flag in place. Updating in place
preserves the option's `id` and `created_at` (a delete-and-recreate would churn
both); `name`, `value`, `label` and `provider` are immutable through this
endpoint.

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
