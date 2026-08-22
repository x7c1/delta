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
branch: task/0822-1123-feat-prompt-templates-backend
created_at: 2026-08-22T11:23:00Z
updated_at: 2026-08-22T12:43:41Z
---

# feat(api): add the prompt-template registry (table, CRUD API, client hooks)

## Overview

Users keep retyping or pasting the same long instructions into the composer
("once CI is green, merge and then update the plan doc…"). A **prompt
template** is a named, provider-independent block of text the user registers
once and later inserts into the composer at the cursor — it works like a
reusable skill invocation that is the same whether the session runs on Claude
Code or Codex. This task lays the data and API foundation; the Settings editor
and the composer insertion UI are separate follow-up tasks and must **not** be
started here.

Build it as the mirror of the launch-option registry, layer by layer, with
two deliberate differences: templates are **global** (no `provider` column —
the text is provider-independent by design) and the `PATCH` endpoint updates
the **content** (`label`, `text`), not a toggle, so the row also carries an
`updated_at`.

### Backend (`backend/`)

- **Migration**: add a `prompt_template` subject to the schema ladder
  (`backend/crates/gateway/delta-sqlite/src/migrations/`, new file next to
  `launch_option.rs`, registered in `mod.rs` `SUBJECTS`), as one additive step
  at version 4, and bump `SCHEMA_VERSION` from 3 to 4 in the same change
  (`migrations/mod.rs:105`). Table:

  ```sql
  CREATE TABLE IF NOT EXISTS prompt_template (
    id         INTEGER PRIMARY KEY,
    label      TEXT NOT NULL,
    text       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
  ) STRICT;
  ```

  Session-independent like `launch_option` (no foreign key, never cascaded);
  say so in the module doc the way `migrations/launch_option.rs` does.
- **Model**: `PromptTemplate { id: i64, label: String, text: String,
  created_at: String, updated_at: String }` in `delta-model`
  (`backend/crates/domain/delta-model/src/`, new module beside
  `launch_option.rs`, re-exported from `lib.rs`).
- **Port**: extend the session-store port
  (`backend/crates/domain/delta-usecase/src/ports/session_store.rs`, see the
  launch-option methods at `:607-633` and their blanket forwarding impl at
  `:949-`) with `list_prompt_templates`, `create_prompt_template(label, text)`,
  `update_prompt_template(id, label, text) -> Option<PromptTemplate>`,
  `delete_prompt_template(id)`. Update the in-memory `fake_store`
  (`delta-usecase/src/interactor/testing/fake_store.rs`) accordingly.
- **SQLite store**: implement the four methods in `delta-sqlite`
  (`backend/crates/gateway/delta-sqlite/src/store/`, new module beside
  `launch_options.rs`, wired in `store/mod.rs`). List order is `created_at`
  ascending, `id` ascending as tiebreak. `update` sets `updated_at` to now and
  returns `None` for an unknown id; `delete` of an unknown id is a no-op. Add
  store tests beside `store/tests/launch_options.rs`.
- **Usecase**: a `prompt_templates` interactor module mirroring
  `delta-usecase/src/interactor/launch_options/crud.rs` (list / create /
  update / delete), with the validation rule: `label` and `text` are trimmed
  of surrounding whitespace **for the emptiness check only** — an empty
  trimmed `label` or an empty trimmed `text` is rejected as a bad request;
  the stored `text` keeps its original leading/trailing whitespace and
  newlines (a template may intentionally end with a newline). Unit tests beside
  `launch_options/tests.rs`.
- **Wire** (`backend/crates/gateway/delta-wire/src/rest/`): `WirePromptTemplate`
  (`#[ts(rename = "PromptTemplate")]`), `WirePromptTemplatesResponse`
  (`{ prompt_templates: [...] }`), `WireCreatePromptTemplateRequest { label,
  text }`, `WireUpdatePromptTemplateRequest { label, text }` — one type per
  file as the launch-option files are; re-export from `rest/mod.rs`; add each
  to `src/bin/export-ts.rs` so `make gen` emits the TS types.
- **Endpoints** (`delta-wire/src/endpoint/table.rs:188-200` for the pattern):
  `ListPromptTemplates: GET /api/prompt-templates`,
  `CreatePromptTemplate: POST /api/prompt-templates` (201),
  `UpdatePromptTemplate: PATCH /api/prompt-templates/{id}` (404 on unknown id),
  `DeletePromptTemplate: DELETE /api/prompt-templates/{id}` (204).
- **Handlers and routing**: handlers in
  `backend/crates/apps/delta-server/src/api/mod.rs` beside
  `list_launch_options`…`delete_launch_option` (`:494-578`), bound in
  `app.rs` beside the launch-option bindings (`app.rs:65-68`). Add app-level
  tests beside `launch_options_list_is_empty_on_a_fresh_store` /
  `create_then_list_and_delete_launch_option` (`app.rs:571-640`) covering
  list-empty, create→list→update→list→delete, and the 400/404 paths.
- **Docs**: add a "Prompt templates" section to
  `docs/guides/api/settings.md` after the launch-options section (`:67-`),
  same shape (description, each endpoint with request/response JSON and
  status codes).

### Frontend wiring (so the UI tasks are UI-only)

- Run `make gen` and commit the generated
  `frontend/packages/gateway/wire-gen/src/generated/PromptTemplate*.ts`
  (plus the `index.ts` re-exports the generator maintains).
- `@delta/api-client`: `getPromptTemplates` / `createPromptTemplate` /
  `updatePromptTemplate` / `deletePromptTemplate` on the HTTP client
  (`frontend/packages/gateway/api-client/src/http.ts:514-` for the
  launch-option methods), a `promptTemplates` query key
  (`query-keys.ts:50-51`), and `usePromptTemplatesQuery` /
  `useCreatePromptTemplateMutation` / `useUpdatePromptTemplateMutation` /
  `useDeletePromptTemplateMutation` hooks (`query-hooks.ts:426-500`), each
  mutation invalidating the list key exactly as the launch-option hooks do.
- `@delta/api-mocks`: `prompt_templates` handlers and store fixtures
  (`frontend/packages/testing/api-mocks/src/handlers.ts:762-820`,
  `fixtures.ts:624-`, `:740-770`) so component tests and e2e-fake specs in the
  follow-up tasks can drive the full CRUD without a backend. Seed two
  fixtures, one of them multi-paragraph (several lines, > 200 characters),
  so list rendering in later tasks is exercised against realistic length.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `SCHEMA_VERSION` is 4 and the ladder validates: the `prompt_template`
      step is the only version-4 step (migration registry tests pass, and the
      schema tests in `store/tests/schema.rs` that stamp/open a database keep
      passing at the new version).
- [x] A database written at version 3 (without `prompt_template`) opens and is
      migrated forward, and its pre-existing launch options survive (store
      test in the style of `store/tests/schema.rs`).
- [x] `GET /api/prompt-templates` returns `{"prompt_templates": []}` on a fresh
      store (app test).
- [x] `POST /api/prompt-templates` with `{label, text}` returns 201 and the
      row with `id`, `created_at`, `updated_at`; the row then appears in the
      list (app test).
- [x] `POST` with a whitespace-only `label` or a whitespace-only `text`
      returns 400; a `text` with leading/trailing newlines is stored verbatim
      (usecase unit test + app test).
- [x] `PATCH /api/prompt-templates/{id}` with `{label, text}` returns 200 with
      the updated row (re-stamped `updated_at`), and returns 404 for an
      unknown id (app test).
- [x] `DELETE /api/prompt-templates/{id}` returns 204 and the row is gone from
      the list; deleting an unknown id is a 204 no-op (app test).
- [x] The list is ordered by `created_at` ascending, `id` ascending on ties
      (store test).
- [x] `make gen` output is committed: `make gen-check` passes, and
      `frontend/packages/gateway/wire-gen/src/generated/PromptTemplate.ts`,
      `PromptTemplatesResponse.ts`, `CreatePromptTemplateRequest.ts`,
      `UpdatePromptTemplateRequest.ts` exist.
- [x] `@delta/api-client` exposes the four HTTP methods and four hooks, and
      the mutation hooks invalidate `queryKeys.promptTemplates` (unit tests
      beside the launch-option ones in `http.test.ts` / hook tests).
- [x] `@delta/api-mocks` serves `GET/POST/PATCH/DELETE */api/prompt-templates`
      backed by the mock store, with two seeded fixtures one of which spans
      multiple lines (handler test or fixture assertion).
- [x] `docs/guides/api/settings.md` documents the four endpoints.
- [x] `make check` passes (Rust fmt/build/test/clippy, `gen-check`,
      frontend build/typecheck/test/lint, e2e, e2e-fake). Pre-commit, the
      check phase runs the same stages with a before/after hash of
      `wire-gen/src` around `make gen` in place of `make gen-check`, which
      by construction only passes once the generated files are committed.

## Out of scope

- The Settings category that edits templates and the composer button /
  popover that inserts them — separate tasks. Do not add UI in this PR.
- Provider scoping, placeholders / variable expansion, ordering or tags,
  and any link to session-start templates.
