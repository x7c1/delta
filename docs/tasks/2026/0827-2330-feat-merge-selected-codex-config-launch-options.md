---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience, error-type-design, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && ! grep -q "two_config_launch_options_are_still_rejected_in_a_worktree" backend/crates/gateway/codex-agent/src/tests.rs && grep -q "fn merge_config" backend/crates/gateway/codex-agent/src/adapter/config_merge.rs && ! grep -q "grant_deferral_reason" backend/crates/gateway/codex-agent/src/adapter/mod.rs && grep -q "LAUNCH_OPTION_REJECTED_CODE" backend/crates/apps/delta-server/src/api/api_error.rs'
assignee: null
branch: task/0827-2330-feat-merge-selected-codex-config-launch-options
created_at: 2026-08-27T14:16:00Z
updated_at: 2026-08-27T16:05:19Z
---

# feat(codex): merge every selected `config` launch option and union the worktree git grant into it

## Overview

A Codex launch option is a `(name, value)` pair that `thread_start_params`
(`backend/crates/gateway/codex-agent/src/adapter/mod.rs`, around line 548)
copies onto the `thread/start` params by name. Selecting the same name twice
is rejected (`UsecaseError::LaunchOptionRejected`, the `params.contains_key`
arm around line 563). For every field but one that is right. For `config` it
is the wrong contract: `config` is a single JSON object holding *many*
independent settings, so the shipped preset (`Config: reasoning summary`,
`launch_option_catalog.rs`) and a user's own `config` row — typically the one
carrying their machine-specific `sandbox_workspace_write.writable_roots` — are
mutually exclusive, and the catalog doc tells the user to duplicate the preset
and edit the copy instead.

That copy flow has a trap the same file documents: `apply_worktree_git_grant`
(`mod.rs` ~636) **defers** the `<repo-root>/.git` grant whenever the selected
`config` states anything at or under `sandbox_workspace_write`
(`grant_deferral_reason`, ~667), so the moment a user adds their own
`writable_roots` — the very key the doc points them at — the worktree session
loses the grant, git writes inside the worktree start raising approval
prompts, and the only notice is one `eprintln!` line. Two changes remove both
the exclusivity and the trap:

### 1. `config` selections are merged, not rejected

Put the merge in a new sibling module `adapter/config_merge.rs` with a
`pub(super) fn merge_config(...)` entry point (keep `mod.rs` from growing —
it is already the crate's largest file), and call it from
`thread_start_params` before `apply_worktree_git_grant` runs (it already runs
last, so the grant sees the merged object). Rules, in selection order (the
order `launch_option_ids` arrived in, which is what `resolve.rs` preserves):

- Every selected `config` value must parse (`thread_start_value`) to a JSON
  object; a non-object among two or more selections is rejected, naming the
  offending row's `label`/`name`. A single non-object `config` keeps today's
  behaviour (passed through; the grant defers as before).
- Objects merge **deep**: a key present on one side only is taken; two objects
  under the same key recurse; two scalars that are **equal** are fine, and so
  are two **equal** lists. The one list that is **unioned** rather than
  compared is `sandbox_workspace_write.writable_roots` (under either
  spelling): earlier selection first, dropping exact-duplicate elements — it
  is a set of paths, which is what a user means by selecting two rows that
  both state it. Any other list that differs between two selections is a
  conflict (an ordered list such as `mcp_servers.<name>.args` concatenated
  silently would launch a broken command line).
- Everything else is a **conflict and is rejected**, naming the full key path:
  two different scalars, scalar vs object/array, or the *same setting spelled
  two ways* — a dotted key such as `sandbox_workspace_write.writable_roots` on
  one side and the nested table `{"sandbox_workspace_write": {"writable_roots":
  …}}` on the other. Do not silently prefer one side ("last wins") — a typo'd
  duplicate must surface, and `is_sandbox_workspace_write_key` (~682) already
  shows how both spellings are recognised; generalise that recognition to any
  top-level dotted key vs nested path when detecting the clash. Do not try to
  normalise spellings either: the dotted form is the one the real-Codex canary
  asserts (see the module doc's "The worktree git-directory grant"), so keep
  whatever spelling the user wrote.
- Duplicates of any **other** field stay rejected exactly as today — keep
  `two_launch_options_naming_the_same_field_are_rejected` green.

Collect every conflict found across the merge and report them together in
one rejection rather than stopping at the first (validation errors are
returned as a batch, not one per round-trip).

### 2. The worktree git grant unions instead of deferring

Replace `grant_deferral_reason` with a union: after the merge, the grant
appends `<repo-root>/.git` to the `writable_roots` list the (merged) config
already states — under whichever spelling it uses (dotted key, or nested
table) — creating the list when the config states other
`sandbox_workspace_write` keys but not that one, and creating the whole key
when the config says nothing about the sandbox (today's behaviour). Skip the
append when the path is already listed. The only remaining reason to stand
aside is a `writable_roots` that is not an array (or a `config` that is not
an object): keep that as a logged no-op, but log it through the crate's
logging facility rather than a bare `eprintln!`, and fold the reason text into
the same message shape as today so an operator grepping for the old line
still finds it.

Re-read the module doc's "The worktree git-directory grant" section before
changing this: it records that on Codex 0.144.4 a leaf `writable_roots`
override *replaces* the user's global list rather than unioning with it, which
is exactly why appending Delta's path to the user's explicit list is the right
outcome (the user's list is what the thread ends up with, and it now includes
the worktree's git directory). Keep that doc accurate: it currently says the
grant is skipped when the user states the sandbox, and the catalog doc
(`launch_option_catalog.rs` ~31-47) says the preset and a user row are
mutually exclusive and that a copy "has to include that path itself" — both
paragraphs are now false and must describe the merge + union instead. Same for
the `thread_start_params` doc bullet about the same key twice (~527-547), the
`apply_worktree_git_grant` doc (~614-635), `UsecaseError::LaunchOptionRejected`'s
doc in `delta-usecase/src/error.rs` (~149-157), and the `api_error.rs` comment
beside its mapping (~262-265).

### 3. The rejection gets a stable error code

`LaunchOptionRejected` maps to 400 with `None` for the code
(`backend/crates/apps/delta-server/src/api/api_error.rs`, the
`Error::LaunchOptionRejected(_)` arm). Every other client-actionable rejection
in that file carries a `*_CODE` constant the browser can branch on; give this
one `LAUNCH_OPTION_REJECTED_CODE = "launch_option_rejected"` in the same
style, register it wherever the file's siblings are listed (the error-code
table in `docs/guides/api/` — find the guide that documents e.g.
`launch_option_builtin`), and keep the human message as the `message` field.
Check how the composer surfaces this 400 today (search the frontend for the
send-failure toast path) and make sure the merged-conflict message — which now
names a key path — is shown verbatim, not swallowed into a generic
"send failed".

### Tests

- Adapter unit tests in `backend/crates/gateway/codex-agent/src/tests.rs`
  (next to the existing `launch_options_map_onto_thread_start_fields_by_name`
  group, reusing its `option(name, value)` helper): delete
  `two_config_launch_options_are_still_rejected_in_a_worktree` and add, one
  per case — two disjoint objects merge; nested objects merge deep; two
  `writable_roots` arrays union without duplicates; two differing lists under
  any other key are rejected naming the path, two equal ones pass; equal
  scalars pass;
  differing scalars are rejected naming the path; scalar-vs-object is rejected;
  dotted-vs-nested spelling of the same setting is rejected naming both;
  a non-object among two selections is rejected naming the row; a non-`config`
  duplicate is still rejected; three selections merge in order. Assert
  rejections with `matches!` on the variant, never `assert_eq!` on the error.
- Grant tests in the same file: rename/rewrite
  `a_user_config_stating_the_sandbox_suppresses_the_grant` and
  `a_non_object_user_config_suppresses_the_grant` to pin the new outcomes —
  dotted `writable_roots` gets the `.git` path appended; nested
  `writable_roots` likewise; a config stating other sandbox keys gains the
  list; a path already present is not duplicated; a non-array `writable_roots`
  is left alone; a non-object config is left alone. Keep
  `a_user_config_without_a_sandbox_key_is_merged_with_the_grant` and the
  resume tests green.
- Full-loop test in `backend/crates/apps/fake-codex/tests/full_loop/launch_options.rs`:
  add a case that registers two `config` rows (one with a `writable_roots`
  path, one with an unrelated key), selects both for a worktree launch and
  asserts the observed `thread/start` `config` holds the union including the
  repo's `.git`; and one that selects two conflicting `config` rows and
  asserts the 400 carries `code: "launch_option_rejected"` and a message
  naming the key, with no session row left (mirror
  `a_codex_launch_option_overriding_a_delta_owned_field_fails_the_spawn`).
- The real-Codex canary `real_thread_start_honors_the_worktree_git_grant`
  (`codex-agent/tests/real_codex_canary.rs`) is not run by `make check`; keep
  it compiling and, if it drives `thread_start_params` with a user `config`,
  update its expectation to the union.
- Frontend: `e2e-fake/builtin-launch-option-copy.spec.ts`'s header comment
  states the exclusivity as the reason for the copy flow — rewrite the header
  (the copy flow itself still works and stays tested), and update
  `docs/guides/api/settings.md` (~68-90, built-ins) with one paragraph saying
  that several `config` rows may be selected together and are merged, with
  conflicts rejected. No picker change is required: `LaunchOptionsPicker.tsx`
  already lets two same-named rows be ticked.

Run `make check` and fix whatever it reports.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Two or more selected Codex `config` launch options reach `thread/start`
      as one deep-merged object (`writable_roots` unioned, earlier selection
      first; every other list must match exactly); differing scalars or
      lists, type clashes, and dotted-vs-nested spellings of one setting are
      rejected in a single 400 naming every conflicting key path — pinned by the adapter unit tests in
      `codex-agent/src/tests.rs` (the old
      `two_config_launch_options_are_still_rejected_in_a_worktree` is gone,
      gate appended) and the full-loop cases in
      `fake-codex/tests/full_loop/launch_options.rs`.
- [x] The merge lives in `codex-agent/src/adapter/config_merge.rs` (gate
      appended); `grant_deferral_reason` no longer exists (gate appended) and
      the worktree grant appends `<repo-root>/.git` to a user-stated
      `writable_roots` under either spelling instead of standing aside —
      pinned by the rewritten grant tests.
- [x] `LaunchOptionRejected` responds with the stable code
      `launch_option_rejected` (`LAUNCH_OPTION_REJECTED_CODE`, gate appended)
      documented beside the other codes, and the composer shows its message
      verbatim.
- [x] The adapter module doc, the catalog doc, the error docs and
      `docs/guides/api/settings.md` describe merge + union; no doc still calls
      the preset and a user `config` row mutually exclusive.
- [x] Duplicates of any non-`config` Codex field are still rejected
      (`two_launch_options_naming_the_same_field_are_rejected` passes).

## Out of scope

- Claude launch options: the CLI receives duplicated flags as-is and nothing
  in Delta enforces exclusivity there; unchanged.
- A picker-side hint about same-named rows.
- Merging the user's *global* Codex `writable_roots` (from `config/read`)
  into the thread override — the module doc names it as the way out if leaf
  replacement ever proves harmful; it has not.
- Editing built-in presets in place.
