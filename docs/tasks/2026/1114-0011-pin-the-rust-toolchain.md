---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && test \"$(cd backend && rustc --version | cut -d' ' -f2)\" = 1.100.0 && grep -q 'package-ecosystem: rust-toolchain' .github/dependabot.yml && ! grep -rqE 'toolchain: *stable' .github/workflows"
assignee: null
branch: task/1114-0011-pin-the-rust-toolchain
created_at: 2026-11-13T15:11:15Z
updated_at: 2026-11-13T15:31:22Z
---

# build: pin the Rust toolchain so a new release arrives as a pull request

## Overview

CI installs the Rust toolchain as `stable`, a moving version. When Rust
1.100.0 was released, `main` started failing on a commit that changed no
code: the `backend` job's `cargo clippy --all-targets -- -D warnings` now
reports `unreachable call` in
`backend/crates/apps/fake-claude/tests/full_loop.rs` (the
`.map_err(|e| e.to_string())` that follows `oneshot(...).await`, whose
error type is `Infallible`). The required check `all-green` fails on `main`
and on every open pull request, including the release pull request.

Pin the toolchain in the repository and let Dependabot raise it. A new Rust
release then arrives as a pull request whose checks show what it breaks,
while `main` stays green.

What to do:

1. Add `rust-toolchain.toml` at the repository root, pinning
   `channel = "1.100.0"` with the components the checks use (`rustfmt`,
   `clippy`). The root is chosen so that every `cargo` call, in `backend/`
   or elsewhere, resolves the same version.
2. Make the whole gate pass on 1.100.0. Fix the two `unreachable call`
   sites in `full_loop.rs` by removing the unreachable conversion rather
   than allowing the lint, and fix whatever else 1.100.0 reports once clippy
   gets past that crate. Do not add `#[allow(...)]`, do not relax
   `-D warnings`, and do not skip or ignore tests.
3. In every workflow under `.github/workflows/` that installs Rust through
   `dtolnay/rust-toolchain` with `toolchain: stable` (`ci.yml`,
   `bundle.yml`, `create-release-pr.yml`, `release.yml`), install the
   version `rust-toolchain.toml` names instead, so the file is the only
   place the version is written. Prefer a plain `rustup toolchain install`
   step (rustup 1.28 and later installs the toolchain and components the
   file names when called without a toolchain argument; confirm this against
   the rustup documentation and the rustup version on the runner images used
   here, including `macos-26` and `ubuntu-22.04`). Keep `bundle.yml`'s
   per-target install (`rustup target add` for `matrix.target`). Keep the
   cargo cache keys meaningful: where a key relied on the toolchain version
   the action exported, key on `rust-toolchain.toml` instead. Every
   remaining `uses:` must still satisfy `scripts/check-workflow-pins.sh`.
4. Add a `rust-toolchain` entry to `.github/dependabot.yml` (directory
   `/`, the same weekly schedule as the other entries) and extend the
   header comment to cover it.
5. Update `docs/guides/development/README.md` (and `local-run.md` if it
   needs it): the Rust version is pinned in `rust-toolchain.toml`, rustup
   installs it on first use, and Dependabot raises it.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make check` passes with the pinned toolchain, with no lint allowed,
      no warning level relaxed and no test skipped to get there
- [x] `rustc --version` in `backend/` reports 1.100.0, taken from
      `rust-toolchain.toml` at the repository root
- [x] No workflow passes `toolchain: stable`; each installs the version the
      file names
- [x] `.github/dependabot.yml` has a `rust-toolchain` entry
- [x] The development guide says where the Rust version is pinned and how it
      is raised

### Before merge (verified outside the check command)

- [ ] On the pull request, the required check `all-green` passes and the
      `bundle` workflow's macOS and Linux jobs pass with the new install step
