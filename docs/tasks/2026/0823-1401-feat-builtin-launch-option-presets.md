---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'cd backend && cargo fmt --all -- --check && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && BEFORE=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && make gen && AFTER=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && [ "$BEFORE" = "$AFTER" ] && cd frontend && pnpm -r build && pnpm -r typecheck && pnpm -r test && pnpm -r lint && cd .. && make e2e && make e2e-fake'
assignee: null
branch: task/0823-1401-feat-builtin-launch-option-presets
created_at: 2026-08-23T14:01:46Z
updated_at: 2026-08-24T01:31:00Z
---

# feat(api): ship built-in launch options and stop truncating long values

## Overview

A launch option is a complete pass-through: Delta validates neither `name` nor
`value`, and a typo is never caught on this side. Claude puts `name` straight
into argv (`ClaudeCodePtyHookAdapter::launch`), and Codex puts it straight in as
a `thread/start` JSON key (`thread_start_params` in
`backend/crates/gateway/codex-agent/src/adapter.rs`) — so a Codex field has to
be typed in its exact camelCase (`approvalsReviewer`), and
`approvals_reviewer` fails at the codex server with nothing Delta can say about
it. Meanwhile the combinations people actually use are a short, known list.

Ship that list. A **built-in launch option** is a `(label, name, value?)` record
Delta declares for a provider and materializes into the `launch_option`
registry at startup, so it is already there the first time Settings is opened.
It is a real row, not a synthetic one, and it carries a `builtin_key` that marks
it as ours: the UI badges it, the API refuses to delete it, and its
`default_enabled` flag stays entirely the user's business.

Two properties make this cheap and safe:

- **A real row keeps ids intact.** `launch_option_ids` is a `Vec<i64>` that runs
  from the composer store through the send path, `launch_options/resolve.rs` and
  into each adapter. Built-ins are rows in the same table, so their ids are
  ordinary ids and **nothing on the selection or launch path changes** — no
  second id space, no changes to `LaunchOptionsPicker`, `composerStore` or
  `resolve.rs`.
- **`PATCH` cannot edit content.** `WireUpdateLaunchOptionRequest` carries only
  `default_enabled`; `name`, `value` and `label` are immutable through the API.
  So the declared catalog can be the single source of truth for those three
  fields, and startup reconciliation can freely overwrite them without ever
  destroying something the user typed. Preserve `default_enabled` and only
  `default_enabled` across a reconcile.

The same change fixes a defect that this feature makes load-bearing: **a long
`value` cannot be read anywhere in the UI**. Both the Settings row
(`SettingsView.tsx`, `LaunchOptionRow` at `:1375`) and the composer picker
(`LaunchOptionsPicker.tsx:122`) `truncate` to one line, and the Settings row has
no `title` either, so a registered JSON `config` value is simply unreadable
after it is saved. One of the built-ins below is exactly such a value, and it is
meant to be copied and adapted, so it has to be readable and reusable.

The Settings row therefore shows every value in full, always — no collapsed
state, no reveal control, and no length threshold deciding between them. A
registry row's whole point is to say what it will pass to the agent, so hiding
that behind a click serves nothing, and a threshold only adds a rule that has
to be kept calibrated against the shipped catalog.

### The catalog

Deliberately small — these are options in daily use, not an inventory of
everything the providers accept. A `builtin_key` is stable and provider-scoped
(e.g. `claude:model-opus`); it is an internal identity, never shown in the UI.

**Claude** (`name` is a CLI flag):

| key | label | name | value |
| --- | --- | --- | --- |
| `claude:model-opus` | `Opus` | `--model` | `opus` |
| `claude:model-fable` | `Fable` | `--model` | `fable` |
| `claude:model-sonnet` | `Sonnet` | `--model` | `sonnet` |
| `claude:permission-mode-auto` | `Permission mode: auto` | `--permission-mode` | `auto` |

`--model` uses the documented aliases that track the latest model, so they do
not go stale; a concrete slug such as `claude-fable-5` is deliberately not
shipped. `--permission-mode` ships only `auto`; the CLI also accepts
`acceptEdits`, `bypassPermissions`, `manual`, `dontAsk` and `plan`, but listing
unused values would only make the picker longer.

**Codex** (`name` is a `thread/start` field):

| key | label | name | value |
| --- | --- | --- | --- |
| `codex:approvals-reviewer-auto-review` | `Auto review approvals` | `approvalsReviewer` | `auto_review` |
| `codex:approval-policy-on-request` | `Approvals: on request` | `approvalPolicy` | `on-request` |
| `codex:config-reasoning-summary` | `Config: reasoning summary` | `config` | `{"model_reasoning_summary": "auto"}` |

`approvalsReviewer` and `approvalPolicy` values come from the vendored
app-server schema's `ApprovalsReviewer` and `AskForApproval` enums
(`backend/crates/gateway/codex-agent/vendor/app-server-schema/codex_app_server_protocol.v2.schemas.json`).

Codex's `model` is **not** shipped: unlike Claude it has no aliases, so any
entry would be a dated snapshot of a concrete slug. `sandbox` and `personality`
are not shipped either — not in use.

The `config` entry is a **starting point to copy**, not something most users
will select as-is. `config` is a single `thread/start` field and the adapter
rejects the same field twice, so this built-in and a user's own `config` row are
mutually exclusive — which is the intended flow: real `config` values carry
machine-specific paths (`sandbox_workspace_write.writable_roots`), so the user
duplicates this row, adds their paths, and selects theirs. Shipping it means
they do not have to discover the JSON key names first. (A future adapter change
could merge several selected `config` options instead of rejecting them, which
would remove the exclusivity; out of scope here.)

## Implementation

### Backend

- **Preset type** — `LaunchOptionPreset { key: &'static str, label:
  &'static str, name: &'static str, value: Option<&'static str>, provider:
  AgentProvider }` in `delta-model`
  (`backend/crates/domain/delta-model/src/`, new module beside
  `launch_option.rs`, re-exported from `lib.rs` the way `LaunchOption` is, so
  `delta_usecase` re-exports it too and both adapter crates can reach it).
- **Catalog constants** — `CLAUDE_LAUNCH_OPTION_CATALOG` in `claude-agent`
  beside `CLAUDE_CAPABILITIES`, `CODEX_LAUNCH_OPTION_CATALOG` in `codex-agent`
  beside `CODEX_CAPABILITIES`. Each adapter declares its own provider's
  vocabulary, next to the `launch_option_style` capability that says which
  vocabulary it is — the same rule that already keeps the capability profile in
  the adapter that owns the behaviour. Do **not** add a field to
  `AgentCapabilities`: the catalog is data, not a capability shape, and it is
  not exposed on `GET /api/providers` (see "not on the providers payload"
  below).
- **Composition-root accessor** — `launch_option_catalog(provider) ->
  &'static [LaunchOptionPreset]` in `delta-bootstrap`
  (`backend/crates/libs/delta-bootstrap/src/lib.rs`) beside
  `provider_capabilities` at `:67`, as an exhaustive `match` so a new provider
  forces a decision here. Add `AgentProvider::ALL: [AgentProvider; 2]` in
  `delta-model` (the test at `agent_provider.rs:58` already hand-writes this
  array — use the new const there) so the caller can iterate every provider's
  catalog without a list that can silently fall behind.
- **Migration** — append one additive step at version 5 to
  `migrations/launch_option.rs` and bump `SCHEMA_VERSION` from 4 to 5 in
  `migrations/mod.rs`. A step's SQL is run as a batch, so both statements
  belong in the one step (SQLite cannot add a `UNIQUE` column via
  `ALTER TABLE`):

  ```sql
  ALTER TABLE launch_option ADD COLUMN builtin_key TEXT;
  CREATE UNIQUE INDEX IF NOT EXISTS ux_launch_option_builtin_key
    ON launch_option(builtin_key);
  ```

  Document the column in the module docs the way the existing ones are:
  `builtin_key` is `NULL` for a row the user registered and non-null for one
  Delta ships; it is both the marker and the reconciliation key.
- **Model** — add `builtin_key: Option<String>` to `LaunchOption`, with a doc
  comment saying what the two cases mean and that a non-null row's `name`,
  `value` and `label` are owned by the declared catalog.
- **Port + store** — extend the session-store port
  (`ports/session_store.rs:607-633`, blanket impl at `:972-`) and
  `delta-sqlite` (`store/launch_options.rs`) with what reconciliation needs:
  upsert-by-`builtin_key` (insert with `default_enabled = 0` when absent;
  otherwise update `label`/`name`/`value`/`provider` and **leave
  `default_enabled` untouched**) and delete-of-built-ins-not-in-a-given-key-set.
  Update `interactor/testing/fake_store.rs` to match. `list_launch_options`
  orders built-ins first (by `id`, which follows catalog insertion order), then
  user rows in the existing order — a fixed-length leading block means a
  built-in's position never moves as the user adds or removes their own rows.
- **Usecase** — `reconcile_builtin_launch_options(&[LaunchOptionPreset])` beside
  `interactor/launch_options/crud.rs`: upsert every preset, then drop built-in
  rows whose key is no longer declared. A dropped row's id may still sit in a
  saved selection; that is already handled ("a selected id that is no longer
  registered is skipped rather than aborting the launch"), so no extra work —
  say so in the doc comment. Must be idempotent.
- **Delete refusal** — `delete_launch_option` returns a new error variant for a
  built-in row (`error.rs`, beside `LaunchOptionRejected` at `:148`), mapped in
  `api/api_error.rs:246` to **409**, not 400: the codebase already draws that
  line — 400 is for a request value the server will not honour
  (`LaunchOptionRejected`, `PermissionDecisionUnsupported`), 409 for a target
  whose current state forbids the operation (`PermissionNotPending`). A
  built-in is the latter. Deleting an unknown id stays a 204 no-op.
  `PATCH` keeps working on built-ins — being able to tick `default_enabled` on
  one is the point.
- **Startup wiring** — call the reconcile from `delta_bootstrap::build()`
  (`lib.rs:159-`, right where `restore_all_dispatched` already runs its
  boot-time sweep), iterating `AgentProvider::ALL`. A side effect worth naming
  in `docs/guides/compatibility.md`: built-ins come back by themselves after
  `make reset`.
- **Wire** — add `builtin: bool` to `WireLaunchOption`, derived from
  `builtin_key.is_some()`. Expose the boolean, **not** the key: the UI only
  needs to know whether the row is ours.
- **Not on the providers payload** — do not add the catalog to
  `providers_response.rs`. Presets reach the browser as ordinary rows on
  `GET /api/launch-options`, so a second delivery path would be dead weight.
- **Guard tests** (these are the point of having a declared catalog):
  - No entry names something Delta sets itself — `cwd` for Codex
    (`DELTA_OWNED_THREAD_FIELDS` in `codex-agent/src/adapter.rs:180`) and
    `--settings` / `--session-id` / `--resume` for Claude. Codex rejects such an
    option at launch, but **Claude has no rejection path at all** and would
    break sessions silently, so assert both providers.
  - Every Codex value that has an enum behind it (`approvalsReviewer`,
    `approvalPolicy`) is a member of that enum in the vendored schema JSON, so
    raising the vendored schema catches a value that has disappeared upstream.
  - The `config` entry's value parses as JSON (`thread_start_value` falls back
    to passing a non-JSON value through as a string, which would be silently
    inert).
  - Every `key` is unique and every `provider` matches the catalog it is
    declared in.

### Frontend

- **`make gen`** and commit the regenerated `LaunchOption.ts`.
- **`@delta/api-mocks`** — carry `builtin` on the launch-option fixtures and
  handlers (`fixtures.ts`, `handlers.ts`); seed at least one built-in fixture
  whose value is a long JSON string, and make `DELETE` on a built-in fixture
  answer 409 so the UI path can be driven without a backend.
- **`SettingsView.tsx` / `LaunchOptionRow` (`:1375`)**:
  - A small `Built-in` badge on built-in rows, next to `name` (a preset may
    have no `label` to hang it off).
  - **No `Delete` button** on a built-in row — omitted, not disabled; a button
    that cannot be pressed only invites the question why.
  - The `default_enabled` checkbox stays live on built-in rows.
  - **A `Duplicate` action on every row** (built-in and user alike): fills the
    add-form below with the row's `label` / `name` / `value`. This is the
    supported way to build on a preset — safer than hand-copying a JSON value
    out of the page — and it also gives a built-in row's action area something
    to hold now that `Delete` is gone.
  - **Stop truncating the value**: render it in full, wrapped
    (`whitespace-pre-wrap break-all`), with a `max-height` and its own scroll
    for the rare very long value. The text is selectable, so it can be copied
    straight out of the row. **No collapsed state and no reveal control** —
    every row shows its whole value from the start, whatever its length, so
    there is no threshold constant to keep calibrated against the catalog.
- **Add-form value input** — a one-line `input` cannot hold the `config` JSON.
  Make it a `textarea` (`font-mono`, a few `rows`, internal scroll, resizable);
  keep submitting on the form's button, not on Enter. Do not import
  `features/composer/useAutoGrow` from `features/settings` — the Settings
  editor added for prompt templates already handles a multi-line field without
  it.
- **`LaunchOptionsPicker.tsx`** — leave the `truncate` as it is. The picker is
  for choosing, not reading, and it already carries the full text in `title`.
  No badge there either.
- **`docs/guides/api/settings.md`** — document `builtin` on the launch-option
  representation, the 409 on deleting a built-in, and that `PATCH` applies to
  built-ins.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `SCHEMA_VERSION` is 5 and the ladder validates: the `builtin_key` step is
      the only version-5 step, and the schema tests in `store/tests/schema.rs`
      keep passing at the new version.
- [x] A database written at version 4 opens, is migrated forward, and its
      pre-existing launch options survive with `builtin_key` null (store test).
- [x] Reconciliation inserts a declared preset that is absent, with
      `default_enabled` false (usecase test against the fake store).
- [x] Reconciliation is idempotent: running it twice leaves the same rows, ids
      included (usecase test).
- [x] Reconciliation **preserves `default_enabled`**: a built-in row ticked by
      the user stays ticked after a reconcile whose catalog re-declares it
      (usecase test — this is the property the whole design rests on).
- [x] Reconciliation updates a built-in row's `label` / `name` / `value` when
      the declared catalog changes them (usecase test).
- [x] Reconciliation deletes a built-in row whose key is no longer declared,
      and leaves user rows (`builtin_key` null) untouched (usecase test).
- [x] `DELETE /api/launch-options/{id}` on a built-in row returns 409 and the
      row survives; on a user row it still returns 204; on an unknown id it is
      still a 204 no-op (app tests).
- [x] `PATCH /api/launch-options/{id}` on a built-in row returns 200 with
      `default_enabled` flipped (app test).
- [x] `GET /api/launch-options` reports `builtin: true` for shipped rows and
      `builtin: false` for user-registered ones, and orders built-ins ahead of
      user rows (app test + store test).
- [x] No catalog entry names a field or flag Delta sets itself — `cwd` for
      Codex, `--settings` / `--session-id` / `--resume` for Claude (unit test
      over both catalogs).
- [x] Every Codex catalog value backed by a schema enum (`approvalsReviewer`,
      `approvalPolicy`) is a member of that enum as read from the vendored
      schema JSON, and the `config` entry's value parses as JSON (unit tests).
- [x] Catalog `key`s are unique and each entry's `provider` matches the catalog
      it is declared in (unit test).
- [x] `make gen` output is committed (`make gen-check` passes) and
      `LaunchOption.ts` carries `builtin`.
- [x] A `Built-in` row renders the badge and renders **no** `Delete` control,
      while its `default_enabled` checkbox is still operable; a user row
      renders `Delete` and no badge (`SettingsView` unit tests).
- [x] A row renders its value in full with no reveal control: the rendered text
      equals the whole value for both a short value and one far longer than the
      row's width, and no show/hide control is present in either case (unit
      tests). No length-threshold constant remains in `SettingsView.tsx`.
- [x] `Duplicate` populates the add-form's label / name / value inputs from the
      row, for a built-in row and for a user row (unit test).
- [x] The add-form's value control is a `textarea`, so a multi-line JSON value
      can be entered and submitted intact (unit test: type a value containing
      a newline, submit, assert the created body carries it verbatim).
- [x] An e2e-fake spec covers the copy-and-adapt flow end to end: the shipped
      Codex `config` row is visible with its full value, `Duplicate` fills the
      form, an edited value is registered as a new (non-built-in) row, and the
      shipped row cannot be deleted.
- [x] `make check` passes (Rust fmt/build/test/clippy, `gen-check`, frontend
      build/typecheck/test/lint, e2e, e2e-fake). Pre-commit, the check phase
      runs the same stages with a before/after hash of `wire-gen/src` around
      `make gen` in place of `make gen-check`, which by construction only
      passes once the generated files are committed.

### Manual / on-hardware (verified by a human before merge)

- [ ] Starting a real session with a shipped built-in selected actually applies
      it: a Claude session started with `Opus` runs on Opus, and a Codex
      session started with `Auto review approvals` routes approvals to the
      auto-reviewer. Only a live provider can confirm the value is accepted —
      Delta passes it through unvalidated by design.
- [ ] The Settings list reads well with the built-ins present: the badge is
      legible, the leading built-in block does not crowd out the user's own
      rows, and the always-visible `config` value is comfortable to read and
      select without making its row dominate the list.

## Out of scope

- Letting the user edit, add to, or hide entries in the shipped catalog. A
  built-in that does not suit is left unticked; registering your own row is the
  supported way to differ.
- Merging several selected `config` options in the Codex adapter instead of
  rejecting the duplicate field.
- Shipping values that are machine-specific (`--plugin-dir` paths, Codex's
  `sandbox_workspace_write.writable_roots`).
- Generating the catalog from the vendored schema or from `--help` output; it
  is hand-written, with the guard tests above as the safety net.
- Any change to the composer picker beyond leaving it as it is, and any change
  to the selection or launch path (`composerStore`, `resolve.rs`, the adapters'
  launch rendering).
