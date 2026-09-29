---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0929-1143-test-pin-the-pasted-content-tag-path-in-the-hook-tests-and-the-fake-lane
created_at: 2026-09-29T11:43:33Z
updated_at: 2026-09-29T13:56:00Z
---

# test(paste): pin the `<pasted_content>` tag path in the hook tests and in the fake lane

## Overview

Claude Code 2.1.277+ (behind a server-side flag) wraps a paste of 20 or more
characters in `<pasted_content id="xxxx">` … `</pasted_content id="xxxx">`
in both the `UserPromptSubmit` hook's `prompt` and the transcript's user
line. delta#416 taught Delta to see through the wrapper — echo matching,
display, the quote frame of a branch-from-quote, and the `ExternalInput`
notice for a prompt pasted straight into the pane — via
`claude_format::unwrap_pasted_content`
(`backend/crates/domain/delta-attribution/src/claude_format/pasted_content.rs`,
whose doc comment is the contract: four lowercase hex chars in the id, a
newline after the opening tag and before the closing one, `<\pasted_content`
as the escape of a tag-like body). Two gaps were left:

1. the `ExternalInput` path (`on_user_prompt_submit.rs:227`) has no
   regression test — `interactor/hooks/tests/` was outside that PR's refine
   file set;
2. fake-claude never emits the wrapper (`fake-claude/src/transcript.rs:68`
   writes the raw text; its bracketed-paste parser only accumulates the
   bytes), so the fake e2e lane only ever exercises the untagged path and
   nothing end-to-end pins tagged echo matching or quote injection.

### Change

**Hook test.** Add
`interactor/hooks/tests/external_input_unwraps_pasted_content.rs` (one test
per file, registered in `tests/mod.rs`, built like
`unmatched_prompt_is_external_input.rs`): with nothing of Delta's
outstanding for the session, submit a prompt that is a well-formed
`<pasted_content>` block around a body of 20+ characters and assert the
emitted `SessionEvent::ExternalInput { prompt, .. }` carries the bare body.
A second assertion in the same test (or a sibling test if it reads better)
covers text typed next to the block being kept, since that is the
contract's other half.

**fake-claude.** Add a scenario-level option (`"wrap_pastes": true`, default
false, in the JSON scenario file — `fake-claude/src/scenario.rs`) under
which a prompt that arrived as a bracketed paste of 20 or more characters
is wrapped exactly as Claude Code does, in **both** places Claude Code puts
it: the `prompt` of the `UserPromptSubmit` hook it fires, and the
`type: "user"` transcript line. Generate a four-hex-char id, put the
newline after the opening tag and before the closing tag (adding one only
when the body does not already end with a newline), and escape a tag-like
`<pasted_content` inside the body as `<\pasted_content`. A typed (non-paste)
prompt and a paste under 20 characters stay bare. Keep the default off so
every existing scenario is byte-for-byte unchanged, and document the option
where the scenario format is described (the module doc in `scenario.rs` and
`docs/guides/development/e2e.md` if it lists scenario fields). Unit-test
the wrapping in fake-claude (id shape, newline placement, escape, the
20-char threshold, the untouched cases).

**e2e-fake.** Two specs (or two cases in one spec) against a new scenario
with `wrap_pastes` on:

- a Send whose body is 20+ characters is matched to its tagged echo — the
  message renders once, as the sent text without the tags, and the turn
  reaches idle without an `ExternalInput` notice;
- a branch from a quote (the `branch-defer` scenario's quotable reply is the
  model) whose injected prompt is tagged shows the quote frame on the branch
  thread, not the raw tag text.

Both are proofs that the fake now reaches the tagged path: if the wrapper
were silently not applied, assert on the transcript file (as other specs
do) that the tag is actually present.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A pasted-straight-into-the-pane prompt wrapped in a well-formed
      `<pasted_content>` block is reported as `ExternalInput` with the bare
      body (hook usecase test).
- [x] fake-claude wraps a 20+ character paste in the Claude Code tag form in
      both the hook prompt and the transcript line only when the scenario
      enables it (fake-claude unit tests; all existing e2e-fake specs still
      pass unchanged).
- [x] e2e-fake pins tagged echo matching and tagged quote injection against
      a scenario with wrapping on, and asserts the tag is present in the
      transcript it read.
- [x] `make check` is green.

### Manual / on-hardware (verified by a human before merge)

- [ ] Against the real Claude Code (2.1.277 or later): paste a 20+ character
      body into the composer, Send, and confirm the message renders once
      without tag text; then select a reply, branch from the quote, and
      confirm the branch shows the quote frame.

## Out of scope

- Changing `unwrap_pasted_content` or any production parsing (delta#416's
  behaviour is taken as correct).
- Emulating Claude Code's server-side flag: the fake's option is per
  scenario, not per version.
