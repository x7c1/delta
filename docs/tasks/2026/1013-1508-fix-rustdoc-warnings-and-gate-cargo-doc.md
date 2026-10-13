---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && make check-backend-doc && grep -rq 'cargo doc' .github/workflows/ && [ \"$(cd backend && cargo doc --workspace --no-deps 2>&1 | grep -c '^warning')\" -eq 0 ]"
assignee: null
branch: task/1013-1508-fix-rustdoc-warnings-and-gate-cargo-doc
created_at: 2026-10-13T06:08:00Z
updated_at: 2026-10-13T07:36:29Z
---

# fix(backend): resolve every rustdoc warning and gate `cargo doc`

## Overview

`cargo doc --workspace --no-deps` in `backend/` emits 72 warnings across 12
crates, and nothing runs it, so they accumulate unseen:

- 53 × `public documentation for X links to private item Y`
  (`rustdoc::private_intra_doc_links`) — for example
  `delta-usecase/src/release_check/release_check_error.rs` and
  `newer_release.rs` link to `RELEASE_PAGE_PREFIX`, which stopped being
  re-exported.
- 19 × `unresolved link to X` (`rustdoc::broken_intra_doc_links`) — doc links
  whose target was renamed or removed, i.e. documentation that is now wrong.
  Sites include `delta-sqlite/src/lib.rs:1`, `tmux-driver/src/lib.rs:1`,
  `agent-contract/src/lib.rs:5`, `codex-agent/src/lib.rs:101,105`,
  `claude-agent/src/lib.rs:121`, `fake-codex/src/main.rs:8`,
  `delta-usecase/src/interactor/open_cwd/mod.rs:38,52`,
  `delta-usecase/src/interactor/routing/erase_everything.rs:48`,
  `delta-usecase/src/launch_config.rs:55`,
  `delta-usecase/src/ports/comms_log.rs:146`, and
  `delta-attribution/src/attribute/{outstanding_send,state,subagent_launch}.rs`.
  Re-run the command for the authoritative list.

Fix every one at its cause. For an unresolved link, point it at the item it
meant today, or rewrite the sentence if that item no longer exists — do not
just drop the brackets to silence it. For a public-to-private link, decide per
site whether the link target should be reachable from the public item or the
prose should stop linking.

These crates are an application's internals, read by its maintainers, so the
gate should document private items too: add a `check-backend-doc` target that
runs `cargo doc --workspace --no-deps --document-private-items` with
`RUSTDOCFLAGS="-D warnings"` (and keep `--no-deps` so third-party docs are not
built). rustdoc still reports `private_intra_doc_links` under
`--document-private-items` (only its note changes), so this one invocation
catches every warning the plain `cargo doc --workspace --no-deps` would, plus
broken links in private items' docs; one invocation is enough. Add the target to `CHECK_STEPS` in the `Makefile`, update the gate graph
comment above `CHECK_STEPS`, and add the same step to a job in
`.github/workflows/ci.yml`. `cargo doc --workspace` compiles `delta-desktop`,
which needs the WebKitGTK libraries and the built frontend (`embed-web`), so
the target depends on both builds and the CI step runs in the job that already
has both (not the backend job). Record the target in
`docs/guides/development/README.md` where the other backend checks are listed.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make check-backend-doc` runs `cargo doc` over the whole backend
      workspace with `RUSTDOCFLAGS="-D warnings"` and passes.
- [x] `check-backend-doc` is part of `make check` (`CHECK_STEPS`) and a CI
      workflow runs `cargo doc` with warnings denied.
- [x] `cargo doc --workspace --no-deps` without `--document-private-items`
      also emits no warning (counted in `check_command`).

### Before merge (verified outside the check command)

- [ ] Each formerly unresolved link either resolves to the item the sentence
      means or the sentence was rewritten; none was fixed by only removing the
      link brackets (reviewed in the diff).
