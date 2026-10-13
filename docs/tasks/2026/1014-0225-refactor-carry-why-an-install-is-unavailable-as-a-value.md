---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, error-type-design]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -rn 'Unavailable(format!' backend/crates && ! grep -rn 'cause.contains(' backend/crates/gateway/update-installer/src"
assignee: null
branch: task/1014-0225-refactor-carry-why-an-install-is-unavailable-as-a-value
created_at: 2026-10-13T17:25:00Z
updated_at: 2026-10-13T18:11:38Z
---

# refactor(update): carry why Delta cannot install an update as a value, not a string

## Overview

`InstallError::Rejected` and `InstallError::Failed`
(`backend/crates/domain/delta-usecase/src/ports/install_error.rs`) carry a
reason plus an optional `#[source]`, so a caller can match on the variant and
reach the upstream error. `InstallError::Unavailable(String)` still carries
only text, and the installers flatten whatever they know into it:

- `BundleInstaller::for_running_app`
  (`backend/crates/gateway/update-installer/src/bundle_installer.rs`) turns the
  `io::Error` from `std::env::current_exe` into
  `format!("could not tell where Delta runs from: {err}")`, and
  `replaceable_bundle` returns `Result<_, String>` for "not in a bundle",
  "translocated", "not writable" and "no parent directory".
- `PkexecInstaller::install`
  (`backend/crates/gateway/update-installer/src/pkexec_installer.rs`) builds
  `Unavailable(format!(..))` for a missing helper, a missing `pkexec`
  (dropping the `io::Error` it matched on), and "not authorized"; its tests
  tell the cases apart with `cause.contains("is not installed")`.
- `UpdateInstaller::unavailable`
  (`backend/crates/domain/delta-usecase/src/ports/update_installer.rs`)
  returns `Option<String>`, which `release_update/mod.rs` logs and stores as
  `InstallUnavailable::cause`.

Give `Unavailable` the same shape as its siblings — a reason this level
composes plus an optional boxed `#[source]`, with constructors that mirror
`rejected` / `rejected_with` — or, where the cases are a closed set the
callers or tests need to tell apart, a typed value they can match on instead
of text. Keep the upstream `io::Error` as the source where there is one. Have
`UpdateInstaller::unavailable` return the error value rather than a string,
and render it once, through `delta_usecase::with_causes`, where it becomes the
log field and `InstallUnavailable::cause`, so the text the user sees does not
change. `InstallError::Unrunnable` stays as it is.

Update every installer (`bundle_installer.rs`, `pkexec_installer.rs`, and any
other `UpdateInstaller` implementation including test doubles), the
`update-installer` crate docs in `src/lib.rs` that tabulate the outcomes, and
the server tests in `backend/crates/apps/delta-server/src/app/tests/` that
construct the variant.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] No installer builds `InstallError::Unavailable` from `format!` (grep in
      `check_command`), and the `update-installer` tests tell the unavailable
      cases apart by value, not by substring (`cause.contains(` is gone).
- [x] A test shows the `io::Error` behind "could not tell where Delta runs
      from" (or behind a missing `pkexec`) reachable through `source()` and
      downcastable to `std::io::Error`.
- [x] A test in `release_update` shows the cause the browser receives for an
      unavailable install naming the reason exactly once.
- [x] `make check` passes.
