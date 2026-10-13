---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && make check-version-change-test && grep -rq 'check-version-change-test' .github/workflows/"
assignee: null
branch: task/1013-1507-fix-release-version-check-and-macos-sweep-test
created_at: 2026-10-13T06:07:07Z
updated_at: 2026-10-13T06:32:44Z
---

# fix(scripts): read the previous release version from its own tree, and make the sweep test pass on macOS

## Overview

Two scripts that guard the release and the local gate are wrong on inputs they
meet in practice, plus one stale line of `make help` text.

### 1. The release version check fails when a commit adds a workspace dependency

`.github/workflows/release.yml` runs `scripts/check-version-change.sh` on every
push to `main` to decide whether to cut a release. The script reads the
previous version by writing `HEAD~1:backend/Cargo.toml` over the working-tree
copy and running `cargo metadata --no-deps` against it — with the member
crates' manifests still at HEAD. When HEAD adds an entry to
`[workspace.dependencies]` and a member inherits it (`foo.workspace = true`),
the HEAD~1 root manifest lacks the entry, the member manifest no longer
resolves, and `cargo metadata` fails. This happened on 4b2ec64e (the in-app
update merge): the Release workflow went red. Had that commit also been the
version bump, the release would not have been cut.

Read the previous version so that it depends only on `HEAD~1`'s tree, not on a
mix of two trees. Either parse `[workspace.package].version` from
`git show HEAD~1:backend/Cargo.toml` with a real TOML parser (not a grep), or
run `cargo metadata` in a temporary `git worktree` of `HEAD~1`. Keep the
current version read as it is. Update the script's implementation notes to
match.

Add `scripts/tests/check-version-change.test.sh`, in the style of
`scripts/tests/sweep-test-residue.test.sh`: it builds a scratch git repository
under `mktemp -d` with a minimal `backend/` workspace, commits a base, and
asserts the script's `$GITHUB_OUTPUT` for at least these histories:

- version unchanged → `changed=false`
- version bumped → `changed=true` and `version=<new>`
- HEAD adds a `[workspace.dependencies]` entry that a member inherits, version
  unchanged → `changed=false` (today: the script fails)
- the same, together with a version bump → `changed=true`

The script finds the repository through `git rev-parse --show-toplevel`, so
the test can run it with the scratch repository as the working directory.
Wire the test in as `make check-version-change-test`, add it to `CHECK_STEPS`
in the `Makefile`, and run it in CI (`.github/workflows/`) next to the other
script tests.

### 2. `sweep-test-residue-test` always fails on macOS

`scripts/tests/sweep-test-residue.test.sh`, case "a dead run's live
delta-server is killed", plants the orphan as a `#!/bin/sh` script named
`delta-server`. On macOS, `ps -o comm=` reports a script by its interpreter
(`/bin/sh`), so `scripts/sweep-test-residue.sh` correctly declines to kill it
and the case fails, on every run and on a clean `main`. The comment above the
fixture claims the name is reported "on Linux and macOS"; that is wrong. CI
runs on Linux only, so CI never sees it, but `make check` on a Mac is always
red.

The sweep itself is correct: the real `delta-server` is a binary. Fix the
fixture so its process is reported as `delta-server` by `ps -o comm=` on both
Linux and macOS — for example, a copy of a real non-multicall binary (such as
`bash`) named `delta-server` that runs the same FIFO wait. Do not skip the case
on macOS. Keep the constraints the current comment records (the process must
not leave a child that outlives `kill -9`, and must not rely on a symlink to a
multicall coreutils), and rewrite the comment to state what is actually true
on each platform.

### 3. `make help` for `desktop` omits why a running Delta must be restarted

The `## desktop:` line in the `Makefile` says to restart a running Delta after
the bundle is replaced, but not why. With an unchanged version number, the
running process keeps handing the old icon to the Dock and Cmd-Tab (macOS);
`killall Dock` does not fix it and restarting Delta does. Add that reason to
the line, briefly. It is bundled here as the one other stale line found next
to these scripts; it needs no separate PR.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `scripts/check-version-change.sh` reads the previous version from
      `HEAD~1`'s tree alone, and `make check-version-change-test` passes all
      four histories above, including the inherited workspace dependency with
      and without a version bump.
- [x] `check-version-change-test` is part of `make check` (`CHECK_STEPS`) and
      is run by a workflow under `.github/workflows/`.
- [x] `sweep-test-residue-test` passes under `make check` without skipping the
      orphan case on any platform.

### Before merge (verified outside the check command)

- [ ] On this Mac, `make sweep-test-residue-test` passes, including "a dead
      run's live delta-server is killed".
- [ ] `make check-version-change-test` fails when the script is reverted to
      the `main` version (the new test catches the original bug), and passes
      with the fix.
- [ ] The `## desktop:` line states that restarting refreshes the icon the
      Dock and Cmd-Tab show.
