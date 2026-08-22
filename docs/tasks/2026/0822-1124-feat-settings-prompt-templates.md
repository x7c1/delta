---
status: completed
pipeline_phase: null
plan: null
base_ref: task/0822-1123-feat-prompt-templates-backend
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0822-1124-feat-settings-prompt-templates
created_at: 2026-08-22T11:24:00Z
updated_at: 2026-08-22T13:25:36Z
---

# feat(web): add the Settings › Prompt templates category (list ⇄ editor)

## Overview

The prompt-template registry (`GET/POST/PATCH/DELETE /api/prompt-templates`,
`usePromptTemplatesQuery` and the three mutation hooks in `@delta/api-client`,
mock handlers in `@delta/api-mocks`) exists on the base branch. Give it a
management UI in the Settings dialog so a user can register, edit and delete
templates. This task is Settings-only: the composer button that inserts a
template is a separate task.

A prompt template is a named, provider-independent block of text that works
like a reusable skill invocation across Claude Code and Codex sessions, so the
**text can be long — many lines, several paragraphs**. Design the category
around that: never truncate a template to one line, and give the editor the
full pane.

### Category registration

- Add `'prompt-templates'` to `SettingsCategoryId`, `SETTINGS_CATEGORY_IDS`
  (`frontend/packages/apps/web/src/store/settingsStore.ts:12-36`), and one
  entry `{ id: 'prompt-templates', label: 'Prompt templates', render: … }` in
  the `SettingsView` category registry
  (`frontend/packages/apps/web/src/features/settings/SettingsView.tsx:70-96`),
  after `Launch options`. Mount the section's queries only while active,
  like `LaunchOptionsSection({ active })`.

### Right pane: two views, one at a time

- **List view** (default): a "New template" primary button at the top, then
  the registered templates as rows showing **the `label` only** plus `Edit`
  and `Delete` actions on the right. No body preview on the row. Empty state:
  "No prompt templates yet." with the same tone as the launch-options empty
  state (`SettingsView.tsx` `launch-options-empty`). Order is the API order
  (`created_at` ascending).
- **Editor view** (shared by New and Edit): replaces the list inside the same
  pane. Fields: `Label` (single-line input, required) and `Text` (a
  `<textarea>` that takes the remaining pane height and scrolls internally;
  monospace-leaning but readable — reuse the app's body text styling rather
  than inventing a new font stack). Buttons: `Save` (primary; disabled while
  the trimmed label or trimmed text is empty, or while the mutation is
  pending) and `Cancel`. `Save` issues `POST` for New or `PATCH {label,
  text}` for Edit, then returns to the list view. `Cancel` returns to the
  list view and discards edits without confirmation (v1). Leaving the
  category (or closing the dialog) while the editor is open also discards
  the draft — the editor state is local component state, not persisted.
- **Delete** from a list row opens a confirmation (use `Dialog` from
  `@delta/ui-kit`, as the rest of Settings does for overlays) naming the
  template's label; confirming issues `DELETE`. This deliberately differs from
  launch options (which delete immediately): a long template is costly to
  re-create.
- Surface mutation errors inline in the pane (same `ApiError` handling as the
  launch-options form), never silently.

### Tests

- Component tests in `SettingsView.test.tsx` alongside the launch-options
  ones, driven through `@delta/api-mocks`.
- Extend `frontend/packages/apps/web/e2e-fake/settings-categories.spec.ts`
  (or add a sibling spec) so the category appears in the rail and the
  New → Save → list round-trip works against the mock backend.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The Settings rail lists `Prompt templates` after `Launch options`, and
      selecting it renders only the prompt-templates section (the other
      categories unmount), with the selection persisted through
      `settingsStore` like the existing categories (component test +
      e2e-fake).
- [x] The list view renders one row per template showing its label and no
      body text — a seeded multi-line fixture's body does not appear in the
      list DOM — with `Edit` and `Delete` controls per row; the empty state
      shows "No prompt templates yet." when the registry is empty (component
      tests).
- [x] `New template` switches the pane to the editor with empty fields and
      `Save` disabled; filling label and a multi-line text enables `Save`;
      saving issues `POST /api/prompt-templates` with the exact text (newlines
      preserved, no trimming of the body), returns to the list, and the new
      row appears (component test, e2e-fake).
- [x] `Edit` on a row opens the editor pre-filled with that template's label
      and full text; saving issues `PATCH /api/prompt-templates/{id}` with
      `{label, text}` and the updated label shows in the list (component
      test).
- [x] `Cancel` in the editor returns to the list without any request and
      without changing the list (component test).
- [x] `Delete` on a row opens a confirmation naming the template; confirming
      issues `DELETE /api/prompt-templates/{id}` and removes the row;
      dismissing the confirmation issues no request (component tests).
- [x] A failed mutation (mock 400 / 500) shows an inline error in the pane
      and keeps the editor / list state intact (component test).
- [x] `make check` passes (frontend build/typecheck/test/lint, e2e, e2e-fake,
      plus the Rust gates which this task does not touch).

### Manual / on-hardware (verified by a human before merge)

- [ ] In the running app, a template of ~40 lines is comfortable to write and
      revise in the editor: the textarea fills the pane, scrolls internally,
      and nothing in the list view truncates or hints at the body on light,
      dark and sepia themes.
- [ ] The list ⇄ editor switch reads as one category (no layout jump of the
      rail; the "New template" button and row actions are where the eye
      expects them next to the launch-options category for comparison).

## Out of scope

- The composer-side button, popover and cursor insertion — separate task.
- Reordering, tags, search/filter, duplicate-label detection (duplicates are
  allowed; rows are distinguished by id), undo after delete, and any
  placeholder / variable expansion.
- Backend or wire changes (the API on the base branch is the contract).
