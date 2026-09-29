---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -q 'choice_group' frontend/packages/gateway/wire-gen/src/generated/LaunchOption.ts && grep -rq 'choice_group' docs/guides/api/settings.md && grep -rq 'rejects_creating_a_second_default_in_one_choice_group' backend/crates/domain/delta-usecase/src && grep -rq 'rejects_enabling_a_second_default_in_one_choice_group' backend/crates/domain/delta-usecase/src && grep -rq 'new_session_selecting_two_rows_of_one_choice_group_is_rejected' backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests && grep -rq 'codex_new_session_selecting_two_rows_of_one_choice_group_is_rejected' backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests && grep -rq 'role=\"radiogroup\"' frontend/packages/apps/web/src/features/composer/LaunchOptionsPicker.tsx && ! grep -q 'nothing enforces' backend/crates/gateway/claude-agent/src/launch_option_catalog.rs"
assignee: null
branch: task/0929-1741-feat-group-single-valued-launch-options-into-exclusive-choices
created_at: 2026-09-29T08:41:00Z
updated_at: 2026-09-29T12:05:00Z
---

# feat(launch-options): group single-valued options into exclusive choices, in the picker and at launch

## Overview

Delta ships three `--model` rows for Claude (Opus / Fable / Sonnet,
`backend/crates/gateway/claude-agent/src/launch_option_catalog.rs`), and a user
adds more (`--model E`, `--model F`, …) whenever a model Delta does not know
yet appears. Those rows are not independent options: they are **values of one
single-valued setting**, and a session can take at most one of them. Nothing
models that today. The registry row is a flat `(label?, name, value?)` record
(`backend/crates/domain/delta-model/src/launch_option.rs`), the composer picker
is a plain checkbox list in click order
(`frontend/packages/apps/web/src/features/composer/LaunchOptionsPicker.tsx`),
and the Claude launch pushes every selected pair into argv verbatim
(`backend/crates/gateway/claude-agent/src/lib.rs:306-314`), so two ticked
`--model` rows put the flag on the command line twice and leave the outcome to
the CLI — the catalog's own doc comment says so ("nothing enforces that").
Switching model therefore means unticking one box and ticking another, and
forgetting the first half is silent.

Codex already has the rule, but only inside its adapter: a `thread/start` JSON
field can be set once, so `thread_start_params`
(`backend/crates/gateway/codex-agent/src/adapter/mod.rs:546-552`, `:564-`)
rejects two selected rows with the same `name` at send time with
`LaunchOptionRejected` (→ 400), except `config`, which is deep-merged. The UI
does not know this and lets the user tick both, so the failure surfaces only on
send. The task is therefore not to invent a concept but to **lift one that
exists in one adapter into the domain and the UI**, for every provider.

### Design (decided — do not re-open)

Exclusivity has two layers, and each lives where its knowledge already lives:

- **Structural (domain):** rows sharing `(provider, name)` are candidate values
  of one setting. Provider-neutral; no vocabulary needed.
- **Cardinality (gateway):** whether that setting takes one value or several
  is the provider's vocabulary — Codex: everything is single-valued except
  `config`; Claude: `--model`, `--permission-mode`, … are single-valued while
  `--add-dir`, `--plugin-dir`, … are repeatable. This follows the existing
  "derived, never stored, answered per response by the gateway that owns the
  vocabulary" pattern of `dangerous` (`LaunchOptionDangerPolicy`,
  `backend/crates/domain/delta-usecase/src/agent/launch_option_danger.rs`;
  composition-root side at
  `backend/crates/libs/delta-bootstrap/src/launch_option_danger.rs`).

Rejected alternatives, for the record: a user-maintained `group` column (a
user's `--model E` should join the shipped `--model` group without the user
naming the group; also needs a migration and a Settings form field); a
two-level setting/value model (a large restructure that undoes the deliberate
flat pass-through design); shipping the single-valued name list to the client
as a capability (leaks vocabulary the client then has to match on — `dangerous`
ships the verdict, not the spellings).

### The change

1. **Generalize the classification port.** Rename `LaunchOptionDangerPolicy`
   to a port that classifies a launch option in the provider's vocabulary
   (e.g. `LaunchOptionVocabulary` / `LaunchOptionPolicy` — pick one name and
   use it everywhere), keeping `is_dangerous(provider, name, value)` and
   adding `cardinality(provider, name) -> LaunchOptionCardinality`
   (`Single | Multiple`; put the enum beside `LaunchOptionSpec` in
   `backend/crates/domain/delta-usecase/src/agent/`). Rename the null policy,
   the `Interactor` field and the `with_…` injector
   (`backend/crates/domain/delta-usecase/src/interactor/mod.rs:200-207`,
   `:351`, `:464-`) and the bootstrap wiring
   (`delta-bootstrap/src/launch_option_danger.rs` → a module named for the
   generalized role) accordingly. Update `lib.rs` re-exports. One port, wired
   once, is where the next classification also goes.

   The null policy keeps `is_dangerous = false` and answers
   `cardinality = Multiple` (today's behaviour: nothing is grouped, nothing is
   rejected), so domain tests and dev harnesses without the real policy behave
   as before — mirror the reasoning in the existing `NoDangerousLaunchOptions`
   docs.

2. **Per-provider cardinality, each adapter choosing its own default.**
   - Claude (`backend/crates/gateway/claude-agent/`): new module beside
     `launch_option_danger.rs`, e.g. `launch_option_cardinality.rs`, exposing
     `launch_option_cardinality(name)`. **Default `Single`**; a closed list of
     repeatable flags answers `Multiple`. Seed the list from `claude --help`
     (2.1.284): the variadic `<...>` flags `--add-dir`, `--allowedTools` /
     `--allowed-tools`, `--disallowedTools` / `--disallowed-tools`, `--betas`,
     `--file`, `--mcp-config`, `--tools`, and the flags documented
     "(repeatable: …)": `--plugin-dir`, `--plugin-url`. Document in the module
     why the default is `Single` (a wrongly grouped repeatable flag shows up
     as "cannot select two" and is fixed by extending this list; a `Multiple`
     default would require enumerating every single-valued flag and would
     lag forever) and that the list is a snapshot of the upstream CLI, like
     the danger spellings.
   - Codex (`backend/crates/gateway/codex-agent/src/adapter/`): `config` →
     `Multiple` (reuse `CONFIG_FIELD`), everything else `Single`. This is the
     rule `thread_start_params` already enforces; reference it from both
     places so the two cannot drift, and keep the adapter's own check — it
     still owns the `config` merge-conflict detection.
   - Bootstrap accessor dispatching per provider, as
     `is_launch_option_dangerous` does. Guard tests in the composition root:
     every shipped `--model` preset is classified `Single` (the catalog exists
     to be a radio group), and `codex:config-reasoning-summary` is classified
     `Multiple`.

3. **Wire: `choice_group` on every launch-option row.** Add
   `choice_group: Option<String>` to `WireLaunchOption`
   (`backend/crates/gateway/delta-wire/src/rest/launch_options_response.rs:21`)
   next to `dangerous`, derived per response: `Some(name)` when the provider
   classifies `name` as `Single`, `None` when `Multiple`. Extend
   `WireLaunchOption::new` (`:67`) to take it. The client groups rows by
   `choice_group` and never by `name` — the field exists so the grouping
   *rule* stays server-side and can later group differently-named rows
   without a client change; say so in the field's doc comment. Run
   `make gen`, commit the regenerated
   `frontend/packages/gateway/wire-gen/src/generated/LaunchOption.ts`.
   Document the field in `docs/guides/api/settings.md` (the row-field list at
   `:143-178`, plus a paragraph in the "Launch options" section replacing the
   Codex-only "one selection rule is provider-specific" wording with the
   provider-neutral rule and the per-provider cardinality facts).

4. **Domain enforcement at launch, for every provider.** In
   `resolve_launch_options`
   (`backend/crates/domain/delta-usecase/src/interactor/launch_options/resolve.rs`),
   after resolving the ids, reject with `Error::LaunchOptionRejected` when two
   resolved options share a `name` the policy classifies `Single` for the
   session's provider; the message names the `name` and both rows' labels (or
   name + value). Both spawn paths already go through this before anything is
   minted (`lifecycle/spawn_fresh.rs:179`,
   `lifecycle/adapter_session/spawn_adapter_session.rs:112`), so a Claude
   launch with two `--model` rows now fails the send the way a Codex one
   already does, instead of reaching argv twice. Confirm the REST mapping to
   400 `launch_option_rejected` applies on the Claude path too
   (`backend/crates/apps/delta-server/src/api/api_error.rs`). Update the
   Claude catalog doc comment (`launch_option_catalog.rs`) that currently says
   the exclusivity is unenforced — the negated grep in `check_command` pins
   this.

5. **Registry invariant: one default per choice group.** In
   `create_launch_option` and `set_launch_option_default_enabled`
   (`backend/crates/domain/delta-usecase/src/interactor/launch_options/crud.rs:59`,
   `:84`), refuse turning `default_enabled` on when another row of the same
   provider with the same `Single` name already has it — same
   `Error::LaunchOptionRejected` (→ 400 `launch_option_rejected`), message
   naming the row that holds the default. Clearing is always allowed. No
   side effects on other rows: the API never flips a sibling; the client does
   the two writes. Rows stored before this rule can still both say `true`, so
   readers must not trust the flag (see 6).

6. **Composer picker → radio groups.**
   (`LaunchOptionsPicker.tsx`)
   - Partition the provider's rows: rows with `choice_group !== null` are
     grouped by that value; the rest stay checkboxes. Order: groups and
     ungrouped rows interleave by the list position of their first row (the
     shipped `--model` rows come first today); rows inside a group keep list
     order (shipped rows first, then the user's in registration order).
     **A group holding a single row renders exactly like an ungrouped row**
     (a plain checkbox, in both the picker and Settings): a radio with one
     real option is worse than a checkbox, and exclusivity only becomes
     visible once a sibling exists. The grouping data is kept, so the row
     turns into a radio group the moment a second row with the same
     `choice_group` is registered.
   - Each group renders as `role="radiogroup"` with an `aria-labelledby`
     heading showing the group key (`--model`), one **native radio** per row
     (visually hidden or plain — match `ProviderTabs.tsx`'s radio pattern),
     and an explicit first option **"Agent default"** meaning "select none of
     these" — `--model` unset is a legitimate state (the agent's own default
     model). Do not implement "click the checked radio to uncheck"; do not
     fake radios with checkboxes.
   - Selecting a row in a group replaces any sibling in
     `newSessionLaunchOptionIds` (keep click order for the rest). Seeding:
     within a group take the **first** `default_enabled && !dangerous` row in
     list order and ignore the rest (legacy double defaults); ungrouped rows
     seed as today. Dangerous rows keep their badge and the inline warning.
   - Existing per-row `data-testid="launch-option-<id>"` stays on the radio
     input; add `data-testid="launch-option-group-<key>"` on the group and
     `launch-option-group-<key>-none` on the "Agent default" radio.
   - Update the component doc comment (it describes a checklist).
7. **Settings → one default per group.** In `LaunchOptionsSection` /
   `LaunchOptionRow` (`SettingsView.tsx:336`, `:1458`), render rows with the
   same `choice_group` together under the group key, and render their
   `default_enabled` controls as one radio group with an explicit "None"
   option. Switching from Fable to E issues **two** PATCHes in order: clear the
   current default, then set the new one (the server refuses the reverse
   order). Ungrouped rows keep their checkbox. The dangerous-row rules
   (`defaultLocked`, disarm hint) apply unchanged inside a group. Keep the
   registration form as is: the user types `--model` / `E` and the saved row
   joins the group on the next list fetch.

### Tests

- Domain (`interactor/launch_options/tests.rs`, lifecycle tests in
  `interactor/lifecycle/tests/`, one file per test, named after what they
  assert like the existing `new_session_with_launch_options_applies_flags_in_order.rs`):
  a fake policy classifying a name `Single`; a new session selecting two
  `Single` same-name rows is rejected before anything is minted (both spawn
  paths — name the pane-path test
  `new_session_selecting_two_rows_of_one_choice_group_is_rejected` and the
  adapter-path test
  `codex_new_session_selecting_two_rows_of_one_choice_group_is_rejected`);
  two `Multiple` same-name rows still pass; setting a second default in one
  choice group is refused on create
  (`rejects_creating_a_second_default_in_one_choice_group`) and on PATCH
  (`rejects_enabling_a_second_default_in_one_choice_group`); clearing is
  allowed; the null policy groups and rejects nothing. The `check_command`
  greps for these four names.
- Gateway: Claude cardinality unit tests (`--model` Single, `--add-dir` and
  `--plugin-dir` Multiple, an unknown flag Single); Codex (`config` Multiple,
  `model` Single); bootstrap guard tests from step 2.
- Wire: the exact-JSON tests at `launch_options_response.rs:96-` gain
  `choice_group`.
- REST (`backend/crates/apps/delta-server/src/app/tests/launch_options.rs`):
  a GET carries `choice_group: "--model"` on the shipped model rows and
  `null` on a `--plugin-dir` row; a second default in the group answers 400.
- Frontend: `LaunchOptionsPicker.test.tsx` (radio group renders with "Agent
  default", picking one deselects the sibling, seeding takes the first
  default only, ungrouped rows stay checkboxes, dangerous warning still
  appears), `SettingsView.test.tsx` (default radio issues clear-then-set),
  fixtures/handlers in `frontend/packages/testing/api-mocks/src/` gain
  `choice_group` (5 literals in `fixtures.ts:764-820`, `handlers.ts:807-885`,
  `SettingsView.test.tsx:519-536`). e2e-fake
  `builtin-launch-option-copy.spec.ts` / `settings-categories.spec.ts` may
  need the new control shape.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `LaunchOption` on the wire carries `choice_group` (`--model` on the
      shipped model rows, `null` on repeatable / `config` rows), regenerated
      TypeScript committed, documented in `docs/guides/api/settings.md`.
- [x] A new session selecting two rows that share a `Single` name is rejected
      with 400 `launch_option_rejected` on both the Claude and the Codex spawn
      path, before anything is minted; two `Multiple` same-name rows pass.
- [x] Turning `default_enabled` on for a row whose choice group already has a
      default is refused on create and on PATCH; clearing is always allowed.
- [x] Claude classifies `--model` Single and the listed repeatable flags
      Multiple with Single as the default; Codex classifies only `config`
      Multiple; the null policy classifies everything Multiple.
- [x] The picker renders each choice group of two or more rows as a
      `role="radiogroup"` with an "Agent default" option; choosing a row
      deselects its sibling; seeding takes the first eligible default only;
      ungrouped rows and single-row groups remain checkboxes.
- [x] Settings renders one default radio group per choice group and switches
      the default by clearing then setting.
- [x] The Claude catalog doc comment no longer describes the exclusivity as
      unenforced.
- [x] `make check` passes (build, lint, unit, e2e-fake, gen-check).

### Manual / on-hardware (verified by a human before merge)

- [ ] On a real instance, register `--model <custom slug>`; it appears in the
      `--model` radio group beside Opus / Fable / Sonnet; switching between
      them is one click; a session started with the custom row launches with
      that model and one started on "Agent default" launches with none.

## Out of scope

- Grouping rows with **different** names (e.g. `--permission-mode` values with
  `--dangerously-skip-permissions`). `choice_group` leaves room for it; no
  rule ships now.
- A user-editable override of a provider's repeatable-flag list.
- Refusing duplicate `(name, value)` rows in one provider.
- Any SQLite migration or change to the shipped catalogs' contents.
