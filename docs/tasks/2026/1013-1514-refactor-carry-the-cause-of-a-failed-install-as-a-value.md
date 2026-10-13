---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, error-type-design]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1013-1514-refactor-carry-the-cause-of-a-failed-install-as-a-value
created_at: 2026-10-13T12:20:00Z
updated_at: 2026-10-13T15:10:39Z
---

# refactor(update): carry the cause of a rejected or failed install as a value, not a string

## Overview

`InstallError::Rejected(String)` and `InstallError::Failed(String)`
(`backend/crates/domain/delta-usecase/src/ports/install_error.rs`) can only
carry text. Where a structured cause exists, the installers flatten it into
that text: `backend/crates/gateway/update-installer/src/bundle_installer.rs`
(`copy_failure` renders a `CopyError` and its `io::Error` with `with_causes`)
and any other `InstallError::Failed(format!(..))` that embeds an error value.
Once flattened, a caller or a log can no longer inspect the cause (its
`io::ErrorKind`, the path), and it cannot be rendered differently later.

Give these variants a way to carry the cause as a value: a reason the
installer composes, plus an optional `#[source]` holding the upstream error
(`Box<dyn std::error::Error + Send + Sync>` — `delta-usecase` cannot name the
gateway's types), or separate variants for the cases that have an upstream
value. `Display` describes only this level; the places that log the error or
turn it into the user-facing `UpdateInstall::{Rejected, Failed} { cause }`
(`backend/crates/domain/delta-usecase/src/release_update/install.rs`) render
the chain once through `delta_usecase::with_causes`, so what the user and the
log see does not change.

The pkexec path (`backend/crates/gateway/update-installer/src/pkexec_installer.rs`)
receives only the update helper's stderr line across a process boundary; it
has no structured value, so it keeps a plain reason with no source.

Update `backend/crates/gateway/update-installer/src/lib.rs` docs and the
server tests in `backend/crates/apps/delta-server/src/app/tests/latest_release.rs`
that construct these variants.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `InstallError::Rejected` / `Failed` built from a `CopyError` (and from
      any other upstream error value) expose it through `source()`; a test
      matches on the variant and downcasts the source to the original type.
- [x] The rendered text a user sees for such a failure names the cause
      exactly once, covered by a test in `release_update/install.rs` or
      `bundle_installer.rs`.
- [x] No test compares `InstallError` values with `assert_eq!` (the type does
      not derive `PartialEq`).
