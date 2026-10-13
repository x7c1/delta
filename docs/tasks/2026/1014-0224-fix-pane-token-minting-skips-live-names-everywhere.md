---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && [ -z \"$(grep -rln 'minter\\.mint()' backend/crates/gateway backend/crates/domain/delta-usecase/src/interactor)\" ]"
assignee: null
branch: task/1014-0224-fix-pane-token-minting-skips-live-names-everywhere
created_at: 2026-10-13T17:24:06Z
updated_at: 2026-10-13T18:02:15Z
---

# fix(usecase): make the pane token minter skip live tmux names for every caller, and start it in one place

## Overview

`PaneTokenMinter` (`backend/crates/domain/delta-usecase/src/pane_token_minter.rs`)
hands out `delta-<n>` tmux session names from a counter that restarts on every
server start. Sessions from an earlier run survive a restart, so a minted name
can already exist in tmux, and `tmux new-session` then fails with
"duplicate session". Today only one caller deals with that:

- The core's spawn path goes through `mint_free_token`
  (`backend/crates/domain/delta-usecase/src/interactor/lifecycle/mint_free_token.rs`),
  which probes `TmuxDriver::has_session` and skips a live name.
- `ClaudeCodePtyHookAdapter::launch` and `::resume`
  (`backend/crates/gateway/claude-agent/src/lib.rs`) call `self.minter.mint()`
  directly and create the tmux session under whatever name comes back. The
  adapter is not on the production spawn path yet, but its module docs say the
  spawn path converges onto it; when it does, the first spawn after a restart
  that left `delta-1` alive fails.

The docs claim more than the code does: `PaneToken::adopted`
(`backend/crates/domain/delta-usecase/src/pane_token.rs`) says "minting skips
every name tmux still knows", which `PaneTokenMinter::mint` does not. And the
minter has two different starting points: `#[derive(Default)]` starts the
counter at 0 (`delta-0`), `PaneTokenMinter::new()` at 1 (`delta-1`).

Make skipping a live name part of minting itself, so no caller can forget it:
give `PaneTokenMinter` an async method that takes the `TmuxDriver` port and
returns the next minted token whose tmux session does not exist (propagating a
probe error), move the loop and its rationale from `mint_free_token` there,
and use it from the core's spawn path and from both adapter methods. Decide
whether the plain `mint` still needs to be public; if nothing outside the
minter and its own tests needs a token without the probe, narrow it. Keep the
adapter-launch key (`PaneToken::for_adapter_launch`) as it is: it is never
handed to tmux.

Give the minter one starting point: replace the derived `Default` with one
that delegates to `new()` (or drop `Default` if nothing needs it and clippy
does not ask for it), so both start at `delta-1`.

Fix the docs to state what is true afterwards: `PaneToken::adopted`, the
module docs of `pane_token.rs` and `pane_token_minter.rs`, and the
`claude-agent` crate docs where they describe how the adapter mints.

The core and the adapter each own a minter. Until the spawn path is rerouted
through the adapter only the core's is used in production; do not merge the
two minters in this task.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A test shows the minter skipping a name the `TmuxDriver` reports as
      live and returning the next free one, and propagating a probe error.
- [x] A `claude-agent` test shows `launch` and `resume` creating their tmux
      session under a name the fake tmux reports as free when `delta-1` is
      live.
- [x] `PaneTokenMinter::default()` and `PaneTokenMinter::new()` mint the same
      first token, covered by a test.
- [x] No caller in `backend/crates/gateway` or in the core's interactor calls
      `minter.mint()` directly (grep in `check_command`); `make check` passes.

### Before merge (verified outside the check command)

- [x] The doc of `PaneToken::adopted` and the minter's docs describe the
      skipping where it actually happens (reviewed in the diff).
