---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure, error-type-design, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -rq "TMUX_BIN" backend/crates/gateway/tmux-driver/src/ && grep -rq "TMUX_BIN" backend/crates/libs/delta-bootstrap/src/'
assignee: null
branch: task/0912-1146-feat-refuse-to-start-without-tmux
created_at: 2026-09-12T11:46:14Z
updated_at: 2026-09-12T13:29:52Z
---

# feat(server): refuse to start when tmux is not on PATH

## Overview

Delta ships as a single `delta-server` binary and deliberately does not bundle
the host tools it drives. Two of them are already handled gracefully: the
`claude` and `codex` launch binaries are probed through the `BinaryDetector`
port (`backend/crates/domain/delta-usecase/src/ports/binary_detector.rs`) and
an un-installed provider is disabled in the new-session selector with a reason
(`interactor/provider_availability.rs`). The third, `tmux`, is not probed at
all. Every launch of every provider goes through the tmux driver
(`interactor/lifecycle/launch_prep.rs:202-204` calls `create_session`, and the
module doc at lines 15-21 explains the shell is shared by both providers), and
the driver runs `Command::new("tmux")` directly
(`backend/crates/gateway/tmux-driver/src/tmux/mod.rs:66`). On a host without
tmux the first launch fails with `Error::Spawn(io::Error)` carrying the raw OS
text — "No such file or directory (os error 2)" — which reaches the browser as
a spawn failure that never names tmux. The user has no way to tell that a
missing command, rather than Delta, is at fault.

Make the server refuse to start when `tmux` cannot be resolved, with one clear
line on stderr that names the missing command. Nothing more: no install
guidance, no in-app notice, no bundling. A host without tmux cannot run any
session, so failing at startup — the moment the binary is first tried after an
install — is the earliest and plainest place to say so, and it matches how the
server already treats the one other startup condition that deserves a
user-facing message rather than an `anyhow` trace (the refused store overlay in
`backend/crates/apps/delta-server/src/main.rs:35-56`).

### Design

1. **One name for the binary.** The tmux driver hardcodes `"tmux"` at
   `tmux/mod.rs:66`. Lift it into a `pub const TMUX_BIN: &str = "tmux"` in
   `backend/crates/gateway/tmux-driver/src/lib.rs` (re-exported from the crate
   root), used by the driver's `Command::new` and by the startup probe below,
   so the probe can never check a different name than the spawn runs. There
   is no environment override for the tmux binary (`DELTA_TMUX_SOCKET` picks
   the socket, not the program) and this task does not add one.
2. **Probe at the composition root.** In `delta_bootstrap::build`
   (`backend/crates/libs/delta-bootstrap/src/lib.rs:229`), before the store
   is opened, resolve `TMUX_BIN` through the same `PathBinaryDetector` that
   already serves the provider-availability endpoint (constructed at
   line 263 — construct it once, earlier, and reuse it for the interactor).
   When the probe answers `false`, return a new `Error` variant from
   `backend/crates/libs/delta-bootstrap/src/error.rs` that carries the binary
   name as data and whose `Display` reads
   `required command 'tmux' was not found on PATH` — the variant is the
   structural signal, the message is derived from it, in line with the
   crate's existing variants. Keep the probe in its own small function that
   takes `&dyn BinaryDetector` so it is unit-testable with a scripted
   detector; the crate has no fake of its own, so add a minimal one under
   `#[cfg(test)]` (the usecase crate's `FakeBinaryDetector` is `pub(crate)`
   and is not to be widened for this).
3. **Print it plainly.** In `main.rs:35-56` the store-overlay match already
   prints the inner error verbatim to stderr and exits 1. Extend that match
   so the new variant takes the same path: `delta-server: required command
   'tmux' was not found on PATH`, exit 1, no backtrace. Every other bootstrap
   failure keeps the default `anyhow` propagation.
4. **Do not touch** the provider-availability endpoint or the frontend. tmux
   is a host requirement, not a provider, and a server that is not running
   has no UI to show a notice in. Do not soften the tmux driver's own
   `Error::Spawn` either — once the server is up, tmux vanishing mid-run is a
   different failure and out of scope here.

### Session-state coverage

This adds no operation against a session; it is a startup precondition. The
one state that matters is "tmux absent at boot", which the Automated criteria
cover; "tmux removed while the server runs" is out of scope (see above).

### Pipeline notes

- Rust only (`tmux-driver`, `delta-bootstrap`, `delta-server` main). No wire
  change, so no `make gen`.
- `make check` needs tmux on the host, as it already does; CI installs it in
  both the backend and e2e-fake jobs, so the new probe does not change what
  CI requires.
- `make check` takes over ten minutes; the check phase is expected to run it
  through the driver's long-running path rather than a capped worker call.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The tmux driver and the startup probe both name the binary through a
      single `TMUX_BIN` constant exported by the `tmux-driver` crate (the two
      `grep -rq "TMUX_BIN"` gates appended to `check_command`, one per crate).
- [x] With a detector that reports `tmux` absent, the bootstrap probe returns
      the new error variant, and its `Display` names the command and says it
      was not found on PATH (cargo test, `delta-bootstrap`).
- [x] With a detector that reports `tmux` present, the probe returns `Ok`
      (cargo test, `delta-bootstrap`).
- [x] `delta-server`'s startup error match routes the new variant to the
      plain-stderr-and-exit-1 path alongside the store-overlay errors, and
      `cargo clippy` stays clean under `make check`.
- [x] Existing provider availability is unchanged: the `provider_availability`
      tests in `delta-usecase` and the `/api/providers` tests in
      `delta-server` still pass under `make check`.

### Manual / on-hardware (verified by a human before merge)

- [ ] On a shell where `tmux` is not on PATH (e.g.
      `PATH=/usr/bin:/bin delta-server` on a host with tmux under
      `/opt/homebrew/bin`), the server prints
      `delta-server: required command 'tmux' was not found on PATH` and exits
      non-zero without a Rust backtrace.

## Out of scope

- Installation guidance, in-app notices, or bundling tmux.
- Probing `tmux` again after startup, or handling tmux disappearing mid-run.
- An environment override for the tmux binary path.
- Any change to how `claude` / `codex` availability is reported.
