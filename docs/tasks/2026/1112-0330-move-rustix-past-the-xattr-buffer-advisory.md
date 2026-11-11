---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && (cd backend && cargo deny check advisories) && ! grep -q 'RUSTSEC-2026-0486' backend/deny.toml && ! awk '/^name = \"rustix\"/{getline; print}' backend/Cargo.lock | grep -qE '^version = \"1\\.(0\\.|1\\.[0-5]\")'"
assignee: null
branch: task/1112-0330-move-rustix-past-the-xattr-buffer-advisory
created_at: 2026-11-11T18:23:46Z
updated_at: 2026-11-11T18:44:06Z
---

# fix(backend): move rustix to 1.1.6 to leave RUSTSEC-2026-0486 behind

## Overview

CI's `audit` job (`make audit`, which runs `cargo deny check advisories` in
`backend/`) now fails with RUSTSEC-2026-0486: in rustix 1.x before 1.1.6 the
extended-attribute functions in `rustix::fs` mark as initialized bytes the
operating system never wrote when given a zero-length buffer. `backend/Cargo.lock`
resolves rustix 1.1.4, pulled in through `async-io`, `async-process`,
`async-signal`, `polling` and `tempfile`. The advisory was published after
`main`'s last `audit` run, so every pull request's `audit` check fails until
this lands, including ones that do not touch the backend.

What to do:

1. **Update rustix** in `backend/Cargo.lock` to the newest 1.x (1.1.6 or
   later) with `cargo update -p rustix` (if the lock holds more than one
   rustix, update the 1.x one, e.g. `cargo update -p rustix@1.1.4`). The lock
   should change only by what the update pulls in. A 0.38 rustix, if present,
   is not affected and stays.
2. **Do not waive the advisory.** `backend/deny.toml` must not gain an
   `ignore` entry for RUSTSEC-2026-0486; the fix is the update.
3. If the update does not compile or a test fails, fix the cause; do not add
   `allow` attributes.

Out of scope: other dependency updates.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `backend/Cargo.lock` resolves no rustix 1.x older than 1.1.6 (gate in
      `check_command`)
- [x] `cargo deny check advisories` passes in `backend/` without
      RUSTSEC-2026-0486 being named in `backend/deny.toml`
- [x] `make check` passes
