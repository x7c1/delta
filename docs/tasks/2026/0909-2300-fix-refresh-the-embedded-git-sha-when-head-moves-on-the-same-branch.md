---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -q "logs/HEAD" backend/crates/apps/delta-server/build.rs && grep -q "rerun-if-changed" backend/crates/apps/delta-server/build.rs'
assignee: null
branch: task/0909-2300-fix-refresh-the-embedded-git-sha-when-head-moves-on-the-same-branch
created_at: 2026-09-09T14:00:06Z
updated_at: 2026-09-09T14:22:42Z
---

# fix(version): refresh the embedded git sha when HEAD moves on the same branch

## Overview

The footer of the navigator pane shows the server's version string, and on a
debug build that string carries the short git sha of the checkout the binary
was built from (`Delta v0.4.0+dev.<sha>`, see
`backend/crates/apps/delta-server/src/version.rs`). The sha is produced by
`backend/crates/apps/delta-server/build.rs`, which runs `git rev-parse --short
HEAD` at build time and exports it as the `DELTA_GIT_SHA` env var.

The sha goes stale. `build.rs` registers exactly one `cargo:rerun-if-changed`
input: the repository's `HEAD` file (resolved with `git rev-parse
--path-format=absolute --git-path HEAD`). That file holds `ref: refs/heads/main`
and is rewritten only when the checkout switches branches. A commit or a
fast-forward `git pull` on the same branch updates `refs/heads/main` (and the
reflog at `logs/HEAD`) but leaves `HEAD` byte-identical, so cargo never re-runs
the build script: it recompiles `delta-server` with the changed sources and
reuses the build script's cached output, embedding whatever sha was recorded
the last time the script actually ran.

Observed on the development machine (clone always on `main`):

- `HEAD` was last modified 2026-09-03 14:50 (a branch switch back to `main`).
- The build script last ran 2026-09-03 21:56 and recorded `cc9d3d53`
  (`target/debug/build/delta-server-*/output`).
- `refs/heads/main` and `logs/HEAD` were updated 2026-09-09 22:45 (pull to
  `7326b2e0`), and `target/debug/delta-server` was rebuilt at 22:47 from those
  sources — yet the footer still reads `v0.4.0+dev.cc9d3d53`.

The module comment in `build.rs` says "cargo still won't rebuild for every
commit in a working tree with no source changes, and that is fine". The real
behavior is worse than that sentence admits: the sha does not refresh even when
the sources changed and the binary was rebuilt.

### What to build

Make the build script re-run whenever `HEAD` resolves to a different commit,
not only when the `HEAD` file itself changes. The intended mechanism: in
addition to `HEAD`, register the HEAD reflog file, resolved the same way
(`git rev-parse --path-format=absolute --git-path logs/HEAD`). Every
operation that moves `HEAD` — commit, pull, checkout, reset, merge — appends
to that file, and `--git-path` returns the per-worktree file for a linked
worktree (verified: `<repo>/.git/worktrees/<name>/logs/HEAD`).

Constraints to keep, all already stated in the existing module comment:

- Only register paths that exist. Registering a non-existent path makes cargo
  treat the crate as permanently stale and rebuild it on every invocation. The
  reflog can be absent (e.g. `core.logAllRefUpdates=false`, or a fresh
  worktree that has not moved yet), so check for existence before printing the
  `rerun-if-changed` line — the same defensive shape the `HEAD` registration
  already needs.
- Keep the `unknown` fallback for a checkout without `git` / without `.git`.
- Keep the "sha is a debugging hint, not a fingerprint" stance: it is fine that
  a dirty working tree with no commit does not change the sha.

Rewrite the module comment so it describes the new trigger set and states
plainly that the previous `HEAD`-only registration missed same-branch moves.
Do not change `version.rs`, the wire type, or the frontend: the display format
is unchanged, only the freshness of the sha.

Prefer `logs/HEAD` over registering the ref file `HEAD` points to
(`refs/heads/<branch>`): after `git pack-refs` the loose ref file can disappear,
which would trip the "non-existent path → permanently stale" trap unless
`packed-refs` were registered as well. The reflog is a single file that exists
in every ordinary clone and worktree.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `backend/crates/apps/delta-server/build.rs` registers the HEAD reflog
      (`logs/HEAD`, resolved through `git rev-parse --path-format=absolute
      --git-path`) as a `cargo:rerun-if-changed` input in addition to `HEAD`,
      and only when the resolved path exists (`grep -q "logs/HEAD"` on the
      file is appended to `check_command`).
- [x] The build script still exports `DELTA_GIT_SHA` with the `unknown`
      fallback, so `make check` (which builds and runs the `version.rs` tests
      in the debug profile) stays green.

### Manual / on-hardware (verified by a human before merge)

- [x] On the development machine: with the server built from commit A, move
      `main` to a newer commit B by fast-forward `git pull` (no branch switch),
      run `cargo build`, restart the server, and the footer reads
      `+dev.<short sha of B>` — not the sha of A.
- [x] (Carried over from #380, unrelated to this fix — verified on the same
      restarted server) In a Codex session, send a plain message, then a
      branch send mid-turn: the branch-send prompt appears on the new thread,
      the preceding plain prompt is still present on the parent thread, and
      neither prompt is duplicated or dropped.

## Out of scope

- Changing the version format or the release-profile behavior (`v<version>`
  with no metadata).
- Making the sha reflect a dirty working tree (a `-dirty` suffix or similar).
- Registering `refs/heads/<branch>` / `packed-refs` as additional inputs — the
  reflog covers the same events with fewer files.
