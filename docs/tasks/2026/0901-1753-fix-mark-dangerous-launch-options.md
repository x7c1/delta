---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -q "dangerous" frontend/packages/gateway/wire-gen/src/generated/LaunchOption.ts && grep -rq "rejects_creating_a_dangerous_option_as_default_enabled" backend/crates && grep -rq "rejects_enabling_default_for_a_dangerous_option" backend/crates && grep -rq "no_shipped_preset_is_dangerous" backend/crates && grep -rq "detects_danger_full_access_inside_a_config_value" backend/crates && grep -rq "does not auto-check a dangerous option" frontend/packages/apps/web/src && grep -rq "disables default-enabled for a dangerous option" frontend/packages/apps/web/src'
assignee: null
branch: task/0901-1753-fix-mark-dangerous-launch-options
created_at: 2026-09-01T17:53:40Z
updated_at: 2026-09-01T22:14:00Z
---

# fix(api): mark safety-bypassing launch options as dangerous and never default-enable them

## Overview

A launch option that disables the agent's own safety mechanisms (Claude's
`--dangerously-skip-permissions`, Codex's `danger-full-access` sandbox) is
today indistinguishable from any other option: the wire contract
(`backend/crates/gateway/delta-wire/src/rest/launch_options_response.rs:18-37`
`WireLaunchOption`) carries no risk marking, `POST /api/launch-options` only
checks that the name is non-blank
(`backend/crates/apps/delta-server/src/api/mod.rs:515-549`), such an option can
be registered with `default_enabled: true` and is then silently pre-checked on
every new session by the composer picker
(`frontend/packages/apps/web/src/features/composer/LaunchOptionsPicker.tsx:58-94`),
and the UI renders it like any benign flag. Dangerous options must stay
*usable* — but never *silent* and never *on by default*.

### The change

1. **Wire contract**: add `dangerous: bool` to `WireLaunchOption`. It is
   **derived, not persisted** — no SQLite migration, and the domain
   `LaunchOption` struct (`backend/crates/domain/delta-model/src/launch_option.rs:38-73`)
   does not gain a field. Run `make gen` and commit the regenerated TypeScript
   (`frontend/packages/gateway/wire-gen/src/generated/`) — `make gen-check`
   fails otherwise. The `From<LaunchOption>` impl at
   `launch_options_response.rs:39-52` can no longer be a plain `From`; pass the
   computed flag in (or construct explicitly where the provider is known).

2. **Danger predicate, per provider**: the vocabulary belongs to the agent
   gateway crates, exposed to the domain through the same port pattern as
   `validate_launch_options`
   (`backend/crates/domain/delta-usecase/src/agent/factory.rs:74-81`, default
   impl = nothing is dangerous). The predicate takes `(name, value)`:
   - **Claude** (`backend/crates/gateway/claude-agent/`): name
     `--dangerously-skip-permissions` (any value); name `--permission-mode`
     with value `bypassPermissions`.
   - **Codex** (`backend/crates/gateway/codex-agent/`): name `sandbox` with
     value `danger-full-access`; name `approvalPolicy` with value `never` (the
     granular object form counts as dangerous when it carries
     `sandbox_approval: false` or `rules: false`; if the value encoding makes
     that awkward, treating any non-string `approvalPolicy` value as dangerous
     is an acceptable conservative fallback — say so in a comment); name
     `config` whose JSON value contains `sandbox_mode = "danger-full-access"`
     or `approval_policy = "never"` at any path spelling — reuse the existing
     flatten in
     `backend/crates/gateway/codex-agent/src/adapter/config_merge.rs:260-` so
     dotted and nested spellings are both caught. First check whether `sandbox`
     / `approvalPolicy` are already rejected as Delta-owned thread fields
     (`DELTA_OWNED_THREAD_FIELDS`,
     `backend/crates/gateway/codex-agent/src/adapter/mod.rs:192`); mark only
     the forms a user can actually reach, and note the unreachable ones in a
     comment rather than dead code.
   - Update the "Delta validates neither names nor values" module docs at
     `backend/crates/domain/delta-model/src/launch_option.rs:1-32`, which this
     change partially reverses.

3. **Enforcement — dangerous options can never be default-enabled**, in the
   usecase layer
   (`backend/crates/domain/delta-usecase/src/interactor/launch_options/crud.rs`):
   - create (`crud.rs:25`): reject `default_enabled: true` for a dangerous
     option with the existing `Error::LaunchOptionRejected` (→ 400
     `launch_option_rejected`, mapping already exists at
     `backend/crates/apps/delta-server/src/api/api_error.rs:293`). Creating the
     option itself (with `default_enabled: false`) stays allowed.
   - `set_launch_option_default_enabled` (`crud.rs:40`): reject enabling
     default for a dangerous option the same way. Disabling stays allowed.
   - Guard test that no shipped built-in preset is dangerous (none is today —
     the catalogs at
     `backend/crates/gateway/claude-agent/src/launch_option_catalog.rs:44-73`
     and
     `backend/crates/gateway/codex-agent/src/adapter/launch_option_catalog.rs:56-78`
     ship only benign entries), so the reconcile's
     `default_enabled`-preserving upsert can never resurrect a dangerous
     default.

4. **Frontend — warn, and belt-and-suspenders the default**:
   - Registry row (`frontend/packages/apps/web/src/features/settings/SettingsView.tsx`,
     `LaunchOptionRow` at `:1399-1508`): render a `Badge tone="warning"` (e.g.
     "Dangerous") next to a dangerous option, and disable its
     `default_enabled` checkbox with an explanatory tooltip/title (the backend
     rejects the write anyway; don't offer a control that can only fail).
   - Composer picker (`LaunchOptionsPicker.tsx`): a dangerous row carries the
     same warning marker; **never auto-check a dangerous option** even if a
     pre-existing stored row still has `default_enabled: true` (rows created
     before this rule); when the user checks a dangerous option, reveal an
     inline `role="alert"` warning naming the option and stating that it
     disables the agent's safety mechanism. No new dialog primitive — inline
     alert is the established pattern (cf. `SettingsView.tsx:534` etc.).

### Test blast radius

- Wire tests with exact-JSON assertions break on the new field:
  `launch_options_response.rs:62-165` (4 tests).
- Frontend TS literals of `LaunchOption` gain `dangerous`:
  `frontend/packages/testing/api-mocks/src/fixtures.ts:764-820` (5 literals),
  `frontend/packages/testing/api-mocks/src/handlers.ts:807-885`,
  `frontend/packages/apps/web/src/features/settings/SettingsView.test.tsx:519-536`.
- Backend struct literals of domain `LaunchOption` do NOT change (no new domain
  field) — `fake_store.rs:985-1085` and
  `interactor/launch_options/tests.rs` stay shape-compatible; the round-trip
  test at `interactor/launch_options/tests.rs:180-197` already creates
  `--dangerously-skip-permissions` with `default_enabled: false` and must keep
  passing.
- REST integration tests: `backend/crates/apps/delta-server/src/app/tests/launch_options.rs`
  (5 tests; the PATCH test at `:188` flips default on a shipped benign option
  and must keep passing).
- fake-codex full-loop suite registers options via the real endpoint
  (`backend/crates/apps/fake-codex/tests/full_loop/support.rs:301-320`) — new
  create-side rule flows through it; its existing 4 launch-option tests use
  benign options and must keep passing.
- e2e-fake specs `builtin-launch-option-copy.spec.ts` and
  `settings-categories.spec.ts` hit `/api/launch-options` and the settings UI.
- API contract docs: `docs/guides/api/settings.md:70-232` (field lists and
  example bodies gain `dangerous`); add a short paragraph on the dangerous
  policy to `docs/guides/security.md`.

Session-state coverage: not applicable — this operation targets the launch-
option registry and the new-session composer, not a live session; coverage is
instead enumerated across the write paths (create / PATCH / reconcile upsert)
and the read paths (list wire shape / picker seeding), all listed above.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `WireLaunchOption` (and the generated `LaunchOption.ts`) carries
      `dangerous: bool`, derived per provider — `make gen-check` green and the
      committed `frontend/packages/gateway/wire-gen/src/generated/LaunchOption.ts`
      contains `dangerous` (grepped by `check_command`).
- [x] `POST /api/launch-options` with `default_enabled: true` for a dangerous
      option returns 400 `launch_option_rejected`; the same option with
      `default_enabled: false` is created — test name contains
      `rejects_creating_a_dangerous_option_as_default_enabled` (grepped).
- [x] `PATCH /api/launch-options/{id}` enabling default on a dangerous option
      returns 400; disabling stays allowed — test name contains
      `rejects_enabling_default_for_a_dangerous_option` (grepped).
- [x] The Codex predicate catches `sandbox_mode = "danger-full-access"` inside
      a `config` value in both nested and dotted spellings — test name contains
      `detects_danger_full_access_inside_a_config_value` (grepped).
- [x] No shipped built-in preset is dangerous — guard test name contains
      `no_shipped_preset_is_dangerous` (grepped).
- [x] The composer picker never auto-checks a dangerous option even when its
      stored row says `default_enabled: true` — frontend test titled
      `does not auto-check a dangerous option ...` (grepped).
- [x] The settings registry disables the default-enabled control for a
      dangerous option and shows a warning badge — frontend test titled
      `disables default-enabled for a dangerous option ...` (grepped).
- [x] Checking a dangerous option in the picker reveals an inline
      `role="alert"` warning (frontend unit test; covered by the frontend
      suite `make check` runs).

### Manual / on-hardware (verified by a human before merge)

- [ ] Live: registering `--dangerously-skip-permissions` in Settings shows the
      warning badge with the default toggle disabled; selecting it in the
      composer shows the inline warning; the session still launches with the
      flag applied. (Non-blocking for merge under the agreed CI-green
      autonomous policy; recorded for dogfooding.)

## Out of scope

- Blocking or removing dangerous options — they stay selectable per send;
  the remediation is marking + no-silent-default, not prohibition.
- Generalizing launch-option value validation or spelling normalization beyond
  the danger predicate.
- Persisting `dangerous` in SQLite (it is derived; no schema migration).
- A confirm-dialog primitive; the inline `role="alert"` warning is the agreed
  UI shape.
- Marking Codex surfaces a user cannot reach through launch options (e.g.
  fields rejected as Delta-owned) — note them in a comment instead.
