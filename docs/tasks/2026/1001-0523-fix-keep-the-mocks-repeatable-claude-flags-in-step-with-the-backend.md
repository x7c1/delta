---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -rqF \"'--allowedTools'\" frontend/packages/testing/api-mocks/src/ && grep -rqF \"'--plugin-url'\" frontend/packages/testing/api-mocks/src/"
assignee: null
branch: task/1001-0523-fix-keep-the-mocks-repeatable-claude-flags-in-step-with-the-backend
created_at: 2026-09-30T20:23:49Z
updated_at: 2026-09-30T20:35:44Z
---

# fix(web): keep the mock's repeatable Claude flags in step with the backend

## Overview

Two small consistency problems around launch-option choice groups.

**The mock's repeatable-flag list is shorter than the backend's.**
`CLAUDE_REPEATABLE_FLAGS` in
`frontend/packages/testing/api-mocks/src/launchOptionChoiceGroup.ts` lists 3
flags (`--add-dir`, `--plugin-dir`, `--mcp-config`), while the backend's
`REPEATABLE_FLAGS` in
`backend/crates/gateway/claude-agent/src/launch_option_cardinality.rs` lists 11
(both spellings of the allowed/disallowed tool flags, `--betas`, `--file`,
`--tools`, `--plugin-url`, ...). Against the mock, registering two
`--allowedTools` rows puts them in a radio group, whereas the real server lets
both be ticked. Unlike the danger predicate's mock (`launchOptionDanger.ts`),
which deliberately keeps only headline spellings because the backend rule
reaches into Codex `config` values, this is a closed list of plain strings, so
there is no reason for the mock to differ.

Port the full list into the mock, and add an api-mocks test that reads
`REPEATABLE_FLAGS` out of the Rust source (resolve the path relative to the
test file) and asserts the two lists hold the same set, so a flag added on one
side fails the frontend test suite until the other side follows. Update the
doc comment on `CLAUDE_REPEATABLE_FLAGS` so it no longer says "the headline
entries", and the Rust module doc so it mentions the mock copy that must follow
it.

**The choice-group rationale is written three times.** The doc comments of
`partitionByChoiceGroup` (`frontend/packages/apps/web/src/launchOptions.tsx`),
`LaunchOptionsPicker.tsx` (`features/composer/`) and `LaunchOptionsSection` in
`SettingsView.tsx` (`features/settings/`) each explain why a one-row group is
rendered as a checkbox and why rows are grouped by `choice_group` rather than by
`name`. Keep that explanation once, on `partitionByChoiceGroup`, and have the
other two refer to it in a sentence, keeping only what is specific to their own
rendering.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `CLAUDE_REPEATABLE_FLAGS` in api-mocks contains every flag of the
      backend's Claude `REPEATABLE_FLAGS` (`'--allowedTools'` and
      `'--plugin-url'` gated in `check_command`).
- [x] An api-mocks test compares the mock list with the Rust `REPEATABLE_FLAGS`
      and fails when they differ (runs under `make check`).
- [x] An api-mocks handler test shows two `--allowedTools` rows registered
      through the mock get `choice_group: null`.

### Manual / on-hardware (verified by a human before merge)

- [ ] Reading the three doc comments, the choice-group rationale appears once
      (on `partitionByChoiceGroup`) and the other two point to it.
