---
status: completed
pipeline_phase: null
plan: null
base_ref: null
retries_remaining: 1
check_command: "make check && make e2e"
assignee: null
branch: task/0805-1539-feat-settings-launch-options-provider-filter
created_at: 2026-08-05T15:39:35Z
updated_at: 2026-08-06T06:39:07Z
---

# feat(web): scope the Settings launch-options list to the selected provider

## Overview

Dogfooding feedback on Settings → Launch options: the section has a provider
selector (claude code | codex) in the registration form, but the registered
list below it ignores that selector and always shows every provider's options
mixed together. The expectation is that switching the selector shows only the
selected agent's options — the same per-provider scoping the new-session
`LaunchOptionsPicker`
(`frontend/packages/apps/web/src/features/composer/LaunchOptionsPicker.tsx:53-56`)
already applies via `newSessionProvider`. Only the Settings side is missing
the linkage.

Make the provider selector the single provider context for the whole
section — both the registration form and the registered list:

- **Lift the provider selector out of the add-form card** to the top of the
  section content in `LaunchOptionsSection`
  (`frontend/packages/apps/web/src/features/settings/SettingsView.tsx:214-428`),
  above the form, so it visibly scopes everything beneath it. A selector
  buried inside the "add" card that also filters the list below would read
  as a surprising side effect; at the section top it reads as "you are
  viewing and managing this agent's launch options". It keeps driving the
  `provider` field of new registrations exactly as today
  (`SettingsView.tsx:229`, `:251`).
- **Filter the registered list client-side** to `option.provider ===
  provider`, mirroring the picker's approach (the list endpoint keeps
  returning every provider's options; no backend change).
- **Keep the selection stable after a successful add**: today `onSubmit`
  resets the provider to `DEFAULT_PROVIDER` on success
  (`SettingsView.tsx:259`). With the selector scoping the list, that reset
  would yank the view away from the option the user just registered — keep
  the provider as-is and reset only label/name/value/default-enabled.
- **Drop the per-row `ProviderName` prefix** in `LaunchOptionRow`
  (`SettingsView.tsx:860-863`): once the list is single-provider, repeating
  the same provider name on every row is noise; the selector above already
  states the context.
- **Scope the empty state to the provider**: "No launch options registered
  yet." (`SettingsView.tsx:397-400`) must not claim an empty registry when
  only the *selected* provider has none — reword to say no options are
  registered for the selected agent.
- Minor wording: the section intro (`SettingsView.tsx:268-274`) says
  "custom `claude` CLI flags", stale since options became per-provider —
  generalize it to the selected agent's CLI flags.

Frontend-only; no wire or backend change.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] With options registered for both providers, the registered list shows
      only the selected provider's options: it starts on the app default
      provider showing only that provider's rows, and switching the selector
      to the other provider swaps which rows are visible (component tests in
      `SettingsView.test.tsx`).
- [x] When the selected provider has no registered options but the other
      provider has some, the list shows the provider-scoped empty-state
      message instead of the other provider's rows (component test).
- [x] Registering an option keeps the current provider selected (no reset to
      the default provider) while clearing the label/name/value/default
      fields, and the new option appears in the still-filtered list
      (component test; the existing "registers a launch option for the
      selected provider" test extends to assert the post-submit selector
      state).
- [x] `LaunchOptionRow` no longer renders a per-row `ProviderName`; within
      the launch-options list no provider product name appears on rows
      (component test), while the default-enabled toggle and delete actions
      still work against the filtered rows (existing tests keep passing,
      updated for the filtered list).
- [x] `make check` passes (build, typecheck, unit tests, lint,
      dependency-cruiser) and `make e2e` passes.

### Manual / on-hardware (verified by a human before merge)

- [x] In the running app, Settings → Launch options reads as one
      provider-scoped view: the selector at the top of the section switches
      both the form target and the list contents, and the placement/visual
      hierarchy feels natural (selector above the add form, no per-row
      provider name).
- [x] Registering a Codex option while the selector is on Codex leaves the
      view on Codex with the new option visible; the list rows read cleanly
      without the provider prefix on light, dark, and sepia themes.

## Out of scope

- Backend or wire changes — the launch-options list endpoint keeps
  returning all providers' options; scoping stays client-side, matching
  `LaunchOptionsPicker`.
- The new-session `LaunchOptionsPicker` (already provider-filtered).
- An independent provider tab/filter for the list, separate from the form's
  selector — considered and rejected: two provider controls in one section
  would need their own sync story; a single selector scoping the whole
  section is the simpler model.
