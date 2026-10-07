# Providers, launch options, prompt templates, version and storage (`/api/*`)

## Overview

The REST routes behind the Settings screen and the provider selector: which
agent providers this host can launch and what each of them can do, the registry
of custom launch options a session can be started with, the registry of prompt
templates the composer inserts from, the server's own version string for the
browser footer, the newer published release it last found and the desktop
app's download of it, the inventory of where the server keeps its files, the
cleanup of leftover worktrees and migration snapshots (removing old sessions
in bulk, the Storage category's third cleanup, is in
[sessions.md](sessions.md#post-apisessionsprune)), and erasing everything Delta
left on the machine, which stops the server.
Applying a launch option to a session is part of a `new_session` send
([sends.md](sends.md#post-apisends)); conventions and error semantics are in
[README.md](README.md).

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

A few options do not configure the agent so much as switch its own safety
mechanisms off — Claude's `--dangerously-skip-permissions` (or
`--permission-mode bypassPermissions`), Codex's `sandbox = danger-full-access`,
`approvalPolicy = never` (including the granular object form with its
sandbox/rules gates off), and the same two settings written inside a `config`
value, in either of Codex's spellings. Those rows are flagged **`dangerous:
true`** on every response that carries a launch option. They stay perfectly
registrable and selectable per session; what they may never be is *silent*:

- the server refuses to set `default_enabled` on such a row — both `POST` (with
  `default_enabled: true`) and `PATCH` (turning it on) answer `400`,
  `code: launch_option_rejected`. Turning it *off* is always allowed, so a row
  registered before this rule can be disarmed;
- no shipped built-in is dangerous, so startup reconciliation — which preserves
  `default_enabled` — can never resurrect a pre-checked bypass;
- a client is expected to mark the row and **never pre-check it** — including
  when a stored row still says `default_enabled: true` — and to offer its
  default-enabled control only as the way to clear such a stale flag, never as a
  way to set one.

`dangerous` is derived per response from `(provider, name, value)` read in the
provider's own vocabulary rather than stored, so a row registered before a
spelling was recognised starts being flagged as soon as the server learns it.

Rows that share a `(provider, name)` are candidate values of one setting, and
whether that setting takes one value or several is the provider's vocabulary.
When it takes one, its rows form an exclusive **choice group**, and every
response that carries a launch option says so in **`choice_group`** — the
group's key (today the shared `name`), or `null` for an independent option.
A client groups rows by `choice_group` and never by `name`: the grouping rule
stays on the server, which can later group differently-named rows without a
client change. The server enforces what a group implies:

- a session start selecting two rows of one group fails the send (`400`,
  `code: launch_option_rejected`, naming both rows) before anything is created,
  on every provider;
- a group holds at most one default: `POST` with `default_enabled: true` and a
  `PATCH` turning it on both answer `400`, `code: launch_option_rejected`
  (naming the row that holds the default) when another row of the group already
  has it. Clearing is always allowed, and the server never flips a sibling
  itself — switching a group's default is two requests, clear then set. Rows
  stored before this rule may still both say `default_enabled: true`, so a
  client takes the first one in list order.

Per provider:

- **Claude** — `name` is a CLI flag, and a flag is single-valued (`--model`,
  `--permission-mode`, …) unless it is one of the repeatable ones: `--add-dir`,
  `--allowedTools` / `--allowed-tools`, `--disallowedTools` /
  `--disallowed-tools`, `--betas`, `--file`, `--mcp-config`, `--tools`,
  `--plugin-dir` and `--plugin-url`. So the shipped `Opus` / `Fable` / `Sonnet`
  rows and any `--model` row you register form one `--model` group.
- **Codex** — `name` is a session-start request field, and a field can only be
  set once, so every name is single-valued except `config`, which is not one
  setting but a JSON object holding many: **several
  `config` rows may be selected together and are deep-merged** into the one object
the request carries: nested tables merge key by key, and
`sandbox_workspace_write.writable_roots` — a set of paths, where two rows each
naming their machine's roots mean both — is unioned in selection order without
repeating an entry. That is what lets Delta's shipped `Config: reasoning
summary` row be ticked alongside a row carrying your own machine-specific
writable roots. Only a genuine disagreement is refused — two different values
for one setting, a scalar against a table, two different lists under any key
*other* than `writable_roots` (an ordered list such as an MCP server's `args` is
a sequence, not a set, so splicing two together would produce a value neither
row asked for), or one setting written in both of Codex's spellings (the dotted
key `sandbox_workspace_write.writable_roots` and the nested table) — and then
the `400` names every conflicting key path at once, so the whole list can be
fixed in one pass.

Delta adds one entry of its own to that merged object. A Codex session running
in a worktree Delta created needs the worktree's real git directory to be
writable or git commands inside it raise approval prompts, so
`<repo-root>/.git` is appended to whatever `writable_roots` the merged `config`
states — under the spelling it used — and added as that key when it states
none. Everything you wrote is kept: the grant is one added path, never a
replacement, and it is skipped when the path is already listed.

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
        "builtin": false,
        "dangerous": false,
        "choice_group": null
      }
    ]
  }
  ```

  `label` and `value` are `null` when absent (a valueless option carries no
  `value`). `default_enabled` marks the option to start pre-checked in the
  session-start picker. `provider` is `claude` or `codex`. `builtin` is `true`
  for a row Delta ships and `false` for one the user registered; the catalog key
  behind a built-in is internal and never on the wire. `dangerous` is `true` for
  an option that switches the agent's own safety mechanism off (see above): mark
  it, never pre-check it, and let its default-enabled control clear a stale flag
  but never set one. `choice_group` is the exclusive group the row belongs to
  (see above) — `"--model"` on every Claude `--model` row, `null` on a
  repeatable row such as `--plugin-dir` or a Codex `config` row: render a group
  as one single choice, with an explicit "none of these" option, and group by
  this field, never by `name`. A group of one row can stay a plain toggle —
  unticking it already means "none of these" — while the rules above still
  apply to it once a sibling is registered.

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
    "builtin": false,
    "dangerous": false,
    "choice_group": null
  }
  ```

  `builtin` is always `false` here: anything registered through this endpoint is
  the user's own row. There is no way to create a built-in. `dangerous` and
  `choice_group` are the server's verdicts on the `(provider, name, value)` just
  registered.

- **400** — a blank `name`.
- **400** (`code: launch_option_rejected`) — `default_enabled: true` on an option
  that switches the agent's own safety mechanism off, or on a row whose choice
  group already has a default (see above). The same option with
  `default_enabled: false` (or omitted) is created normally.

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
- **400** (`code: launch_option_rejected`) — `default_enabled: true` on an option
  flagged `dangerous`, or on a row whose choice group has its default on another
  row (see above; clear that row first). `false` is always accepted, which is how
  such a row is disarmed.
- **404** — no launch option with that id. An unknown id answers `404` even for
  `default_enabled: true`: there is no row to classify.

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

### `GET /api/latest-release`

Return a published release newer than the running server, for the notice next
to the version in the browser footer. The answer comes from memory: the server
asks GitHub (`GET https://api.github.com/repos/x7c1/delta/releases/latest`,
which never returns drafts or pre-releases) in the background, a few seconds
after it starts and then every 6 hours, and keeps the last verdict. The request
itself never reaches GitHub.

A release counts as newer only when its tag is `v<semver>` and that version is
strictly greater, by SemVer precedence, than the server's base version (build
metadata dropped, so a debug build `v0.5.0+dev.<sha>` is told only about a
release above `0.5.0`). `version` is rendered like a release build's
[`GET /api/version`](#get-apiversion) (`v0.6.0`); `url` is the release's page,
always under `https://github.com/x7c1/delta/releases/`.

`newer` is `null` whenever there is nothing to tell: the server has not checked
yet, the last check found it up to date, the check is turned off
(`DELTA_RELEASE_FEED_URL` set to the empty string — see the
[development guide](../development/README.md#backend-backend)), or every check
so far failed. A failed check (offline, DNS, timeout, a non-2xx answer such as
GitHub's 403 rate limit, unparsable JSON, a tag that is not SemVer, a page
outside the prefix above) logs one `warn` naming the feed's URL and the
cause, and keeps the previous verdict; it is never an error here. Always
**200**.

`offer` says what the browser may show next to the notice, decided by who
launched the server, where it was built (`DELTA_BUILD_ORIGIN` at compile
time — see the [development guide](../development/README.md#build-origin)),
and whether `newer` has an asset this platform may download:

| `offer` | Server | The footer shows |
|---------|--------|------------------|
| `update` | the desktop app, built by the release workflow, when `newer` has an asset it may download (none of the `update_unsupported` cases below) | an **Update** control ([`POST /api/latest-release/download`](#post-apilatest-releasedownload)) |
| `rebuild` | the desktop app, built locally | a hint that it is updated by rebuilding it (`make desktop`) |
| `none` | the CLI server (the browser version), a desktop app built by the release workflow that could not set up its HTTPS client (`update_unavailable` below), or one whose `newer` has nothing this platform may download (`update_unsupported` below) | the notice and its link only |

`download` is the state of `newer`'s download once one has been asked for, and
`null` otherwise (including a download of an older release than `newer`):

- `{ "state": "downloading", "version", "received_bytes", "total_bytes" }` —
  the transfer is running; `total_bytes` is `null` when the answer did not
  state a size.
- `{ "state": "ready", "version" }` — downloaded and its sha256 verified,
  waiting for a later step to apply it.
- `{ "state": "failed", "version", "error" }` — nothing was kept; `error`
  names the cause with its whole chain.

- **200**:

  ```json
  {
    "newer": {
      "version": "v0.6.0",
      "url": "https://github.com/x7c1/delta/releases/tag/v0.6.0"
    },
    "offer": "update",
    "download": {
      "state": "downloading",
      "version": "v0.6.0",
      "received_bytes": 4194304,
      "total_bytes": 9437184
    }
  }
  ```

  or, with nothing to tell:

  ```json
  { "newer": null, "offer": "none", "download": null }
  ```

### `POST /api/latest-release/download`

Start downloading this platform's asset of the newer release
[`GET /api/latest-release`](#get-apilatest-release) reports, in the
background, and answer the download's state; the footer follows it by polling
`GET /api/latest-release`. Takes no body. This only downloads and verifies:
nothing is installed or replaced.

- **What is downloaded.** Exactly the release `GET /api/latest-release`
  announced, by the release workflow's asset names: on Linux x86_64
  `delta-desktop_<version>_amd64.deb`, on macOS aarch64
  `Delta_<version>_aarch64.dmg` (`<version>` without the `v`). Its download URL
  must be under `https://github.com/x7c1/delta/releases/download/` (GitHub's
  redirect from there to its asset host is followed).
- **Verification.** The sha256 is computed while the file streams in and
  compared with the asset's `digest` (`sha256:<hex>`) from GitHub's release
  answer. An asset without one is never downloaded (`update_unsupported`).
- **Where it goes.** `updates/` in the data directory. The file is written
  under `<asset name>.part` and renamed to the asset's name only once its
  digest matches, so a file under that name is always a verified one. When a
  download completes, every other file in `updates/` (other versions, partial
  downloads) is removed. A mismatch, a non-2xx answer, an interrupted or
  stalled transfer (no data for 30 s; connecting may take 15 s) or a write
  failure deletes the partial file and is reported as `failed`.
- **One at a time.** A request while a download runs joins it (no second
  transfer); a request after a failure starts over; a request after the same
  release is ready answers `ready` without downloading again. A partial
  download is never resumed.

- **202 Accepted**: a download is running (just started, or already running),
  with the state `GET /api/latest-release` reports as `download`:

  ```json
  { "state": "downloading", "version": "v0.6.0", "received_bytes": 0, "total_bytes": null }
  ```

- **200**: this release is already downloaded and verified:

  ```json
  { "state": "ready", "version": "v0.6.0" }
  ```

- **409** — refused by the server whatever the browser shows, with nothing
  fetched, `code` naming the case:
  - `update_cli_launcher` — the CLI server launched this server; only the
    desktop app can be replaced.
  - `update_local_build` — this desktop app was built locally (not with
    `DELTA_BUILD_ORIGIN=release`); the release would roll its tree back.
  - `update_unavailable` — the desktop app could not set up its HTTPS client
    (the platform trust store), so it cannot download.
  - `update_no_newer_release` — no newer release is known (not checked yet, up
    to date, the check turned off, or every check so far failed).
  - `update_unsupported` — the newer release has nothing this platform may
    download: no release asset is published for it (Windows, Intel Macs,
    Linux on ARM), the release lacks this platform's asset, the asset's URL
    is outside the prefix above, or it states no `sha256` digest. `error` says
    which. `GET /api/latest-release` offers no Update for such a release, so
    only a request that bypasses the footer meets this.

## Storage

### `GET /api/storage`

Return where this running Delta keeps its files, for the Settings dialog's
Storage category. Every path is absolute and is the one this process resolved at
startup: the data directory and every file derived from it, plus the worktree
base and the transcript root, which sit outside it on purpose. `version` is the
same string [`GET /api/version`](#get-apiversion) returns; `tmux_socket` is the
socket *name* passed to `tmux -L`, not a path; `session_settings` is the file
for the port this server listens on.

Sizes are read from disk on every request, never cached. Each `bytes` is a file
length as `stat` reports it, not the disk blocks `du` counts. `database.bytes`
sums `delta.db`, `delta.db-wal` and `delta.db-shm`, whichever exist, and leaves
out the snapshots. `snapshots` lists the migration runner's `delta.db.bak-v<N>`
copies beside the database in ascending `<N>`, and is empty when there are none.
A file that does not exist counts as zero bytes and is not an error. The
response never carries the hook secret or the auth token. Read-only: nothing
here deletes or moves a file. The snapshots are kept until the user deletes
them with [`DELETE /api/storage/snapshots`](#delete-apistoragesnapshots).

- **200**:

  ```json
  {
    "identifier": "io.github.x7c1.delta",
    "version": "v0.5.0",
    "data_dir": "/home/u/.local/share/io.github.x7c1.delta",
    "database": {
      "path": "/home/u/.local/share/io.github.x7c1.delta/delta.db",
      "bytes": 3250176
    },
    "snapshots": [
      {
        "path": "/home/u/.local/share/io.github.x7c1.delta/delta.db.bak-v3",
        "bytes": 1048576
      }
    ],
    "hook_state": "/home/u/.local/share/io.github.x7c1.delta/delta-hook-state.json",
    "sessions_dir": "/home/u/.local/share/io.github.x7c1.delta/sessions",
    "session_settings": "/home/u/.local/share/io.github.x7c1.delta/settings/7878.json",
    "tmux_conf": "/home/u/.local/share/io.github.x7c1.delta/tmux.conf",
    "tmux_socket": "io.github.x7c1.delta",
    "worktree_base": "/home/u/.delta/worktrees",
    "transcript_root": "/home/u/.claude/projects"
  }
  ```

### `GET /api/storage/worktrees`

List every directory directly under the worktree base, sorted by name, with
what Delta and git know of each:

- `path` — spelled under the configured worktree base, as Delta spells the
  worktrees it creates there.
- `in_use` — a session Delta still lists works in it: it is some session's
  working directory or requested directory, or some message's working
  directory — the check
  [`DELETE /api/sessions/{id}`](sessions.md#delete-apisessionsid) uses to keep a
  worktree another session works in. Such a worktree is cleaned up by removing
  its session, not from here.
- `repo_root` — the repository's main working tree as git reports it, or `null`
  when git no longer knows the directory as a worktree (its repository pruned
  it or is gone, or it was never one).
- `dirty` — whether `git status --porcelain` lists anything (modified, staged or
  untracked files), or `null` when git does not know the directory.

The directories nothing works in are the leftovers: worktrees kept because they
held work when their session was removed, or left by versions that did not
clean up. A worktree base that does not exist yet lists nothing.

- **200**:

  ```json
  {
    "worktrees": [
      {
        "path": "/home/u/.delta/worktrees/x7c1-delta-0198c0df-…",
        "in_use": false,
        "repo_root": "/home/u/src/delta",
        "dirty": true
      },
      {
        "path": "/home/u/.delta/worktrees/x7c1-delta-0198c0e0-…",
        "in_use": true,
        "repo_root": "/home/u/src/delta",
        "dirty": false
      }
    ]
  }
  ```

- **500** — the directory, the store or `git` could not be read.

### `DELETE /api/storage/worktrees`

Remove one directory under the worktree base that no listed session works in.

```json
{ "path": "/home/u/.delta/worktrees/x7c1-delta-0198c0df-…", "force": false }
```

`path` is spelled as [`GET /api/storage/worktrees`](#get-apistorageworktrees)
lists it. `force` (optional, default `false`) is the user's confirmation that
what is in the directory may be lost. A clean worktree is removed with
`git worktree remove` without `--force`, so git's own check still guards any
work that appeared since it was listed. With `force`, a worktree with changes is
removed with `git worktree remove --force`, and a directory git no longer knows
is deleted as a plain directory tree. The worktree's branch is never touched.
After the removal `git worktree prune` runs in the repository (when git named
one), and the `projects` entry Delta seeded for the path in `~/.claude.json` is
removed; a failure there is logged and does not fail the request.

- **204 No Content** — the directory is gone.
- **409** — refused, with nothing touched: the path is not a directory directly
  under the worktree base (`code: "worktree_outside_base"`); a listed session
  still works in it (`code: "worktree_in_use"`); or, without `force`, it has
  uncommitted or untracked changes (`code: "worktree_dirty"`) or git no longer
  knows it as a worktree (`code: "worktree_not_registered"`).
- **500** — `git` or the filesystem failed.

### `DELETE /api/storage/snapshots`

Delete one migration snapshot.

```json
{ "path": "/home/u/.local/share/io.github.x7c1.delta/delta.db.bak-v3" }
```

`path` must be one of the `snapshots` [`GET /api/storage`](#get-apistorage) lists
right now, spelled exactly as listed; any other path — the database itself, a
file beside it, a path that leaves the data directory — is refused and nothing
is deleted.

- **204 No Content** — the snapshot is gone.
- **404** — the path is not a listed snapshot.
- **500** — the file could not be deleted.

### `POST /api/storage/erase`

Erase everything this Delta created on the machine that holds no work, then
stop the server. Takes no body. The rule is the one removing a single session
follows, applied to everything: **Delta never destroys the user's work — it
removes what it created, and only when that holds no work.** There is no
`force`; a worktree kept here is removed, if the user wants that, one at a time
with [`DELETE /api/storage/worktrees`](#delete-apistorageworktrees) or with git.

In order:

1. Every session that is open or still starting is closed, as
   [`POST /api/sessions/{id}/close`](sessions.md#post-apisessionsidclose) closes
   one: a launch in progress is cancelled, a terminal-less agent's process
   stopped. Nothing is refused for being open.
2. Delta's tmux server is killed (`tmux -L <socket> kill-server`, where no
   server running is not an error) and the socket file tmux leaves behind,
   `${TMUX_TMPDIR:-/tmp}/tmux-<uid>/<socket>`, is deleted — before any worktree
   is touched, so no agent process still holds one.
3. Every session is removed as
   [`DELETE /api/sessions/{id}`](sessions.md#delete-apisessionsid) removes one: a
   clean worktree Delta created goes, one with uncommitted or untracked changes
   is kept; a `delta-<id>` branch is deleted when merged, and an unmerged one,
   or a branch Delta did not create, is kept.
4. Every directory left under the worktree base is removed as
   [`DELETE /api/storage/worktrees`](#delete-apistorageworktrees) removes one
   without `force`: a clean worktree git knows goes, a dirty one or one git does
   not know is kept. Their branches are never touched. The worktree base is then
   removed when it is empty, and so is `~/.delta` when the base is the default
   `~/.delta/worktrees`.
5. The `~/.claude.json` trust entry goes with each removed worktree; the file
   itself is Claude Code's and is never deleted.
6. In the data directory, `sessions/`, `settings/`, `tmux.conf`, `updates/`
   and the migration snapshots are deleted.
7. The response is sent, and the server stops serving (the `/ws` stream closes).
8. Once the store is closed, `delta.db`, `delta.db-wal`, `delta.db-shm`, the hook
   state file and the data directory itself are deleted — the directory only
   when nothing else is left in it. `delta-server` then exits `0`. The desktop
   app instead shows what was kept in a dialog and exits `0` when it is
   dismissed, removing its own files under its identifier as it quits (see
   [the install guide](../install/README.md#removing-everything)).

- **200**:

  ```json
  {
    "removed": {
      "sessions": 12,
      "worktrees": ["/home/u/.delta/worktrees/x7c1-delta-0198c0df-…"],
      "branches": ["delta-0198c0df-…"],
      "data_dir": "/home/u/.local/share/io.github.x7c1.delta"
    },
    "kept": [
      {
        "session_id": "0198c0e0-…",
        "kind": "worktree",
        "target": "/home/u/.delta/worktrees/x7c1-delta-0198c0e0-…",
        "reason": "dirty",
        "detail": null
      },
      {
        "session_id": null,
        "kind": "worktree",
        "target": "/home/u/.delta/worktrees/stray",
        "reason": "not_registered",
        "detail": null
      }
    ]
  }
  ```

  `removed.worktrees` lists the removed sessions' worktrees, then the leftovers;
  `removed.data_dir` is the data directory step 8 empties after this response.
  `kept` is shaped as the bulk session removal's
  ([sessions.md](sessions.md#post-apisessionsprune)): the items a removed session
  kept carry its `session_id`, a leftover under the worktree base carries
  `null`, and `reason` may also be `not_registered` (git does not know the
  directory as a worktree). A failure on one item never fails the erase: it is
  logged, and the item is kept with `reason: "failed"`.
- **409** — an erase is already running, or has run and the server is stopping
  (`code: "erase_in_progress"`); nothing is done twice.
- **500** — the sessions or the worktree base could not be listed; nothing
  after that step ran, and another erase may be tried.
