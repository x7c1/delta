---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && make audit"
assignee: null
branch: task/1022-1505-update-rustls-webpki-and-webpki-root-certs-for-new-advisories
created_at: 2026-10-22T15:05:18Z
updated_at: 2026-10-22T15:40:00Z
---

# build(deps): update rustls-webpki, webpki-root-certs and vite for new security advisories

## Overview

The `audit` CI job (`make audit`, which runs `cargo deny check advisories`
in `backend/` and `pnpm audit --prod` in `frontend/`) now fails on every
branch because of two new advisories
against crates in `backend/Cargo.lock`:

- RUSTSEC-2026-0382 — `rustls-webpki` 0.103.15: certificate verification
  may accept distrusted intermediates.
- RUSTSEC-2026-0383 — `webpki-root-certs` 1.0.9: the bundled root
  certificates allow trust in distrusted emSign intermediates.

Update both crates in `backend/Cargo.lock` to versions that the advisories
list as patched, preferably with `cargo update -p rustls-webpki -p
webpki-root-certs` so nothing else moves. If a patched version is not
reachable within the semver ranges the dependents allow, raise the direct
dependency that pins it, keeping the change as small as possible. Do not
add these advisories to the `cargo deny` ignore list: they have fixes, so
the fix is to take them.

If `make audit` reports any other advisory once these two are fixed, fix it
the same way in this change when a patched version exists; when none does,
stop and report it instead of ignoring it.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make audit` passes, so neither RUSTSEC-2026-0382 nor
      RUSTSEC-2026-0383 is reported for `backend/Cargo.lock`.
- [x] The backend still builds and its tests and lints pass under
      `make check`.
