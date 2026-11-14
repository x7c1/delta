---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -qE '^members *=.*\\*/' backend/Cargo.toml && [ \"$(cd backend && cargo metadata --no-deps --format-version 1 | grep -o '\"workspace_members\":\\[[^]]*\\]' | grep -o 'path+file' | wc -l)\" -eq 24 ] && [ \"$(cd backend && cargo metadata --no-deps --format-version 1 | grep -o '\"workspace_default_members\":\\[[^]]*\\]' | grep -o 'path+file' | wc -l)\" -eq 23 ] && grep -q Dependabot backend/Cargo.toml"
assignee: null
branch: task/1114-1510-let-dependabot-see-every-cargo-workspace-member
created_at: 2026-11-14T06:10:57Z
updated_at: 2026-11-14T06:20:18Z
---

# build(backend): list workspace members one directory level deep so Dependabot finds every crate

## Overview

Every Dependabot version-update job for `cargo` in `/backend` has failed
since it was first scheduled (the first failure is on 2026-10-14; the latest
is run 43460474843 on 2026-11-13), with:

```
`delta-update-helper` is listed in default-members but is not a member
for workspace at `dependabot_tmp_dir/backend/Cargo.toml`.
```

No cargo update pull request has ever been opened, so the backend's Rust
dependencies are not being kept up to date, and security updates for
backend crates cannot be raised either (the glib security-update jobs on
2026-10-24 failed with the same error).

The cause is how Dependabot expands workspace globs. Its cargo file fetcher
(`expand_workspaces` in `cargo/lib/dependabot/cargo/file_fetcher.rb` of
dependabot-core) lists only the directory in front of the first `*` — for
`members = ["crates/*/*"]` that is `crates/` — and matches the listed
entries against the whole pattern. `crates/apps`, `crates/domain`, … do not
match `crates/*/*`, so the glob yields no members at all. The crates
Dependabot does see reach it only through the `path = "crates/..."` entries
in `[workspace.dependencies]`. `delta-update-helper`, `fake-claude`,
`fake-codex` and `delta-desktop` are not referenced there, so they are
missing, and `default-members` naming `crates/apps/delta-update-helper`
makes cargo reject the reduced workspace. The job log's "Dependency
Snapshot" confirms this: it lists every crate except those four.

What to do:

1. In `backend/Cargo.toml`, replace `members = ["crates/*/*"]` with one glob
   per layer directory, each with `*` only as its last path segment:
   `"crates/apps/*"`, `"crates/domain/*"`, `"crates/gateway/*"`,
   `"crates/libs/*"`. Cargo resolves exactly the same member set (24
   members, 23 default members; the check command pins these counts).
2. Add a comment above `members` saying why it is written this way: a glob
   with a `*` before the last segment is not expanded by Dependabot, which
   then silently drops crates and fails the update job. Keep it short and
   say a new layer directory needs its own entry.
3. If `docs/guides/` or `CLAUDE.md` describes the workspace layout in a way
   that a reader adding a new layer directory would follow, make it mention
   that the directory must be added to `members`. Do not duplicate the
   explanation; point to `backend/Cargo.toml`.

Keep `default-members` as it is; it is read only by cargo, which expands its
globs correctly.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `members` in `backend/Cargo.toml` has no `*` followed by `/` (the check
  command greps for `^members *=.*\*/`)
- [x] `cargo metadata` reports 24 workspace members and 23 default members,
  the same sets as before the change
- [x] A comment in `backend/Cargo.toml` explains, next to `members`, that
  Dependabot does not expand a `*` before the last path segment and that a
  new layer directory needs its own entry (the check command greps
  `backend/Cargo.toml` for `Dependabot`)
