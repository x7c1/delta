---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && test -f backend/crates/domain/delta-usecase/src/ports/spawning_session.rs && grep -q "pub struct SpawningSession" backend/crates/domain/delta-usecase/src/ports/spawning_session.rs && grep -Pzq "fn insert_spawning_session\(\s*&self,\s*\w+: SpawningSession<'"'"'_>,?\s*\)" backend/crates/domain/delta-usecase/src/ports/session_store.rs && [ "$(cd backend && cargo test -q -p delta-usecase -- --list 2>/dev/null | grep -c '"'"': test$'"'"')" -ge 418 ] && [ "$(cd backend && cargo test -q -p delta-sqlite -- --list 2>/dev/null | grep -c '"'"': test$'"'"')" -ge 90 ]'
assignee: null
branch: task/0902-1202-refactor-group-the-spawning-insert-arguments-into-a-struct
created_at: 2026-09-02T12:02:00Z
updated_at: 2026-09-02T12:36:26Z
---

# refactor(store): group the spawning-insert arguments into a `SpawningSession` struct

## Overview

`SessionStore::insert_spawning_session`
(`backend/crates/domain/delta-usecase/src/ports/session_store.rs:127–137`)
takes eight positional arguments:

```rust
async fn insert_spawning_session(
    &self,
    id: &SessionId,
    cwd: &str,
    branch_at_launch: Option<&str>,
    repo_root: Option<&str>,
    requested_workdir: Option<&str>,
    repository_display_name: Option<&str>,
    provider: AgentProvider,
    pull_request_number: Option<i64>,
) -> Result<(Session, ThreadId)>;
```

Five of them are `Option`s of the same shape, so a typical call reads
`store.insert_spawning_session(&id, "/work", None, None, None, None,
AgentProvider::Claude, None)` — four `None`s, a provider, another `None` —
and which `None` is `repo_root` versus `pull_request_number` cannot be told
without the signature. Every column added to the `session` spawn snapshot
(most recently `pull_request_number`) has to be threaded through this list
and through every caller: the trait, its `Box<dyn SessionStore>` delegation
(`session_store.rs:777`), the sqlite implementation
(`delta-sqlite/src/store/session_store.rs:34` → `store/sessions.rs:146`),
the fake store (`interactor/testing/fake_store.rs:142`), the two production
callers (`lifecycle/spawn_fresh.rs`,
`lifecycle/adapter_session/spawn_adapter_session.rs`) and ~35 test call
sites (`interactor/repository/tests.rs` ×19,
`delta-sqlite/src/store/tests/sessions.rs` ×12, `pull_requests/tests.rs` ×2,
`store/tests/schema.rs` ×1,
`lifecycle/tests/reap_stale_spawns_reaps_an_expired_unbound_spawn.rs` ×1).

Replace the positional list with one named-field struct, mirroring the
sibling port `NewSession` (`ports/new_session.rs`, the fields
`register_session` is called with):

```rust
//! The fields the eager `spawning` row is inserted with.

/// What `SessionStore::insert_spawning_session` records: the spawn-time
/// snapshot a fresh session starts from, before its process has reported
/// anything. See the matching `Session` fields for each one's semantics.
#[derive(Debug, Clone)]
pub struct SpawningSession<'a> {
    pub id: &'a SessionId,
    pub cwd: &'a str,
    pub branch_at_launch: Option<&'a str>,
    pub repo_root: Option<&'a str>,
    pub requested_workdir: Option<&'a str>,
    pub repository_display_name: Option<&'a str>,
    pub provider: AgentProvider,
    pub pull_request_number: Option<i64>,
}
```

- Put it in a new module `ports/spawning_session.rs` (one public type per
  module, named after the type), declared in `ports/mod.rs` as an adjacent
  `mod spawning_session; pub use spawning_session::SpawningSession;` pair
  at its alphabetical position. Do **not** add it to `ports/session_store.rs`,
  which already hosts several public items.
- Change the trait method to
  `async fn insert_spawning_session(&self, spawning: SpawningSession<'_>) -> Result<(Session, ThreadId)>`
  and update the `Box<dyn SessionStore>` delegation, the sqlite adapter
  (`store/session_store.rs` → `store/sessions.rs`; the inner
  `pub(super)` method takes the same struct), and the fake store. Field
  docs on the struct replace the per-argument docs currently on the trait
  method; keep the method doc for what the insert does (the eager
  `spawning` row plus its `main` thread) and drop the parameter list from
  it.
- Update the two production callers to build the struct with named fields.
  They already hold the values under these names, so this is a mechanical
  rewrite.
- Test call sites: the `None`-heavy calls should use a shared default
  builder rather than restating every field. `store/tests/mod.rs` already
  has `new_session()` / `new_session_with(id)` for `NewSession`; add the
  parallel `spawning_session(id, cwd)`-style helper(s) there for the sqlite
  tests, and the equivalent in the usecase crate's test support
  (`interactor/testing/` — wherever `insert_spawning_session` calls in
  `repository/tests.rs` and `pull_requests/tests.rs` can share one), so a
  test that only cares about `cwd` and `repository_display_name` writes
  `SpawningSession { repository_display_name: Some("x7c1/delta"),
  ..spawning_session(&id, "/work") }`. Tests that set a specific field keep
  setting exactly that field by name.
- Behaviour, SQL, stored values, and the returned `(Session, ThreadId)` do
  not change. No wire type changes (no `make gen` needed).

### Pipeline notes

- Run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` before
  finishing the work phase; the frontend is untouched.
- The `check_command` gates: the trait signature is matched with a
  multi-line grep (`fn insert_spawning_session(&self, <name>: SpawningSession<'_>)`
  — rustfmt breaks it across lines), which the compiler then extends to
  every implementation and call site; and the `cargo test -- --list`
  counts of `delta-usecase` (418 today) and `delta-sqlite` (90 today) must
  not drop, so no test is deleted in the rewrite.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `ports/spawning_session.rs` exists and exports
      `pub struct SpawningSession<'a>` with exactly the eight fields above,
      re-exported from `ports/mod.rs` next to its `mod` line (file and grep
      gates in `check_command`).
- [x] `SessionStore::insert_spawning_session` takes
      `SpawningSession<'_>` as its only argument in the trait, the
      `Box<dyn SessionStore>` delegation, the sqlite adapter, and the fake
      store; no call site anywhere in `backend/crates` passes positional
      arguments (trait-signature grep gate in `check_command`; the
      workspace compiles under `cargo build`, which rejects any positional
      caller).
- [x] The sqlite and usecase test suites use a shared default builder for
      the struct so that no test call restates more than the fields it
      cares about, and every test that previously set a specific value
      (branch, repo root, requested workdir, display name, provider, PR
      number) still sets that same value by field name (reading the diff
      confirms each rewritten call; `cargo test` stays green with no test
      deleted — the `cargo test -- --list` counts of `delta-usecase`
      (418) and `delta-sqlite` (90) are floors in `check_command`).
- [x] No SQL statement, stored column, or returned value changes: the
      diff touches no `.sql`-bearing string in `store/sessions.rs` except
      to read fields off the struct, and `delta-server/tests/end_to_end.rs`
      is untouched.

## Out of scope

- Splitting the `SessionStore` trait itself (1165 lines) into read / write
  / clone-root traits, and splitting `store/tests/sessions.rs` or
  `store/tests/schema.rs` into directory modules.
- Changing `NewSession` or `register_session`.
- Any change to the frontend or to wire types.
