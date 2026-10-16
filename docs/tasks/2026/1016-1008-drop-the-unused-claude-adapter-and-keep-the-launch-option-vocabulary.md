---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! git grep -nE 'ClaudeCodePtyHookAdapter|ClaudeLaunchConfig|ingest_hook|ingest_transcript_lines' -- backend/crates && ! test -e backend/crates/gateway/claude-agent/tests && ! test -e backend/crates/gateway/claude-agent/src/ingest.rs"
assignee: null
branch: task/1016-1008-drop-the-unused-claude-adapter-and-keep-the-launch-option-vocabulary
created_at: 2026-10-16T10:09:25Z
updated_at: 2026-10-16T11:58:35Z
---

# refactor(claude-agent): drop the unused adapter and keep the launch-option vocabulary

## Overview

`backend/crates/gateway/claude-agent` carries `ClaudeCodePtyHookAdapter`
(`src/lib.rs`, roughly lines 125–400), its ingestion seam (`src/ingest.rs`),
`ClaudeLaunchConfig` (`src/claude_launch_config.rs`), and two `cfg(test)`
suites that drive it: the provider-neutral contract run (`src/contract.rs`)
and the golden projection test (`src/golden.rs` with
`tests/golden/hook_transcript_mapping.json`). Nothing in production
constructs the adapter. The only adapter factory the composition root
registers is Codex's (`backend/crates/libs/delta-bootstrap/src/build.rs`,
around lines 127–145); Claude sessions are driven by the Interactor's own PTY,
hook and transcript paths in `backend/crates/domain/delta-usecase`. The
adapter's `ingest_hook` / `ingest_transcript_lines` have no production
caller either. The module comments still describe the adapter as the path the
core will be rerouted through, which is not what happened: the Interactor path
is the one that shipped.

Remove the adapter and everything that exists only for it: the adapter type,
`SessionChannel`, the flag constants it alone uses, `ingest.rs` with
`ClaudeHook` and its projections, `ClaudeLaunchConfig`, `contract.rs`,
`golden.rs`, the golden fixture directory, and the crate dependencies that
become unused (`async-trait`, `tokio`, `serde`, `serde_json`, and the
`agent-contract` dev-dependency; check each against the remaining code before
removing it). Keep the crate itself and what the composition root reads from
it: `CLAUDE_CAPABILITIES` (read by
`delta-bootstrap/src/provider_capabilities.rs`),
`CLAUDE_LAUNCH_OPTION_CATALOG`, `launch_option_cardinality` and
`is_dangerous_launch_option` (read by `delta-bootstrap/src/launch_option_catalog.rs`
and `launch_option_vocabulary.rs`). Rewrite the crate-level doc comment so it
describes what remains: Claude's static capability profile and launch-option
vocabulary. Leave `agent-contract` itself in place; `fake-codex` runs it.

Before deleting the two test suites, go through every case in `contract.rs`
and `golden.rs` and find where the production path already verifies the same
behaviour: the Interactor's hook tests under
`backend/crates/domain/delta-usecase/src/interactor/hooks/tests/`, the
permission tests (`interactor/agent_permission.rs` and its neighbours), the
spawn and resume tests under `interactor/lifecycle/`, the `PaneTokenMinter`
tests, and the fake-Claude e2e suite (`make e2e-fake`). Where a case has a
production counterpart, delete it. Where a case covers behaviour the
production path does not test at all (for example a launch command shape or a
turn-end projection that only the adapter suite pinned), add the missing
assertion to the production path's existing test module rather than keeping
the adapter alive for it. Record the case-by-case mapping in your final
report; it becomes the review material for the PR. Do not port tests of the
adapter's own mechanics (its channel, its correlation state, its minted
request ids): that machinery is being deleted and has no production
counterpart to protect.

Code that the deletion makes unused elsewhere (a `delta-usecase` re-export
only the adapter imported, an `AgentEvent` variant nothing else emits) is in
scope only if `cargo` or clippy reports it; do not go looking for more.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `ClaudeCodePtyHookAdapter`, `ClaudeLaunchConfig`, `ingest_hook` and
      `ingest_transcript_lines` no longer appear anywhere under
      `backend/crates` (`git grep` gate appended to `check_command`).
- [x] `backend/crates/gateway/claude-agent/src/ingest.rs` and the crate's
      `tests/` directory are gone (`test -e` gates appended to
      `check_command`).
- [x] `CLAUDE_CAPABILITIES`, `CLAUDE_LAUNCH_OPTION_CATALOG`,
      `launch_option_cardinality` and `is_dangerous_launch_option` are still
      exported and `delta-bootstrap` still compiles against them (the
      workspace build in `make check`).
- [x] The Claude fake e2e suite and the Interactor's hook, permission and
      lifecycle tests pass unchanged or extended; no existing production test
      is deleted or weakened (`make check`).
- [x] The crate's `Cargo.toml` lists no dependency the remaining code does not
      use (clippy and the build in `make check`).

## Out of scope

- Deleting the `claude-agent` crate or moving the launch-option vocabulary
  elsewhere.
- Removing `agent-contract` (the Codex fake still runs it).
- Any change to how the Interactor drives Claude.
