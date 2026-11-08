---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure, error-type-design]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -q 'name = \"pin-utils\"' backend/Cargo.lock && ! grep -q 'RUSTSEC-2017-0008' backend/deny.toml"
assignee: null
branch: task/1108-1800-upgrade-portable-pty-to-clear-the-audit
created_at: 2026-11-08T09:30:00Z
updated_at: 2026-11-08T10:40:00Z
---

# fix(deps): upgrade portable-pty to 0.9 so the dependency audit passes again

## Overview

CI's `audit` job (`make audit`, `cargo deny check advisories` against `backend/deny.toml`) has failed on `main` since 2026-11-05: RUSTSEC-2026-0441 reports `pin-utils` as unmaintained. `pin-utils` reaches the lockfile only through `nix` 0.25, which `portable-pty` 0.8 depends on. `deny.toml` already ignores RUSTSEC-2017-0008 (`serial` is unmaintained) for the same crate, with the reason that `portable-pty` 0.9 replaces `serial` with `serial2` but is a breaking upgrade no compatible update reaches.

While `audit` fails, every pull request shows a failing check, so none can be merged under the merge gate. The repository is also about to require an aggregated `all-green` check that includes `audit`.

What to do:

1. **Upgrade `portable-pty` from 0.8 to 0.9** in the workspace (`backend/Cargo.toml`) and adapt `backend/crates/apps/delta-server/src/pty.rs` (and anything else that uses it) to the 0.9 API, keeping the PTY behaviour unchanged: spawning the shell, resizing, reading and writing, and how the child's exit is observed
2. **Drop the advisories the upgrade resolves** — remove the RUSTSEC-2017-0008 entry from `deny.toml`, and make sure `pin-utils` is gone from `backend/Cargo.lock`. Do not add an ignore entry for RUSTSEC-2026-0441
3. **Keep the audit green** — after the upgrade, `make audit` passes. If the upgrade pulls in another advisory, fix it with a compatible update, or add an ignore entry that states why it cannot be fixed today, in the style of the existing entries
4. **Update the docs** that mention the `portable-pty` version or the `serial` exception, if any

Out of scope:

- Other dependency updates that the audit does not require

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make check` passes
- [x] `pin-utils` is no longer in `backend/Cargo.lock`
- [x] `deny.toml` no longer ignores RUSTSEC-2017-0008

### Before merge (verified outside the check command)

- [x] `make audit` passes on the development machine (it needs network access)
- [ ] The PTY still works end to end: run the server locally, open a session in the browser, type a command and see its output, resize the window and see the shell reflow, and exit the shell and see the session end
