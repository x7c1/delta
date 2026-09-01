---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -rq "rejects_a_hook_without_the_secret" backend/crates/apps/delta-server && grep -rq "rejects_a_transcript_path_outside_the_allowed_root" backend/crates'
assignee: null
branch: task/0901-1456-fix-hook-callback-auth-and-transcript-confinement
created_at: 2026-09-01T14:56:00Z
updated_at: 2026-09-01T15:39:00Z
---

# fix(hooks): authenticate hook callbacks and confine the transcript path they supply

## Overview

The `/hooks/*` endpoints are unauthenticated (the per-run bearer token added
earlier explicitly exempts them — `auth_guard.rs:57` lets `/hooks/*` and
`/health` through), so any local process can forge a hook event by POSTing to
`http://127.0.0.1:<port>/hooks/<name>`. Worse, each hook payload carries a
`transcript_path` that Delta stores and then reads from disk with **no
confinement**: a forged (or first, unvalidated) hook can name
`/Users/you/.ssh/id_rsa` and Delta's tailer will read it and surface parseable
lines to the browser. Two defenses, both triggered by the hook trust boundary:

### Part A — authenticate `/hooks/*` with a per-run secret (carried in the URL)

- **Mint a per-run hook secret** at startup, alongside the existing auth token
  (`main.rs::auth_token_from_env` / `Config` / `AppState`). Store it as an
  `Arc<str>` on `AppState` next to the bearer token.
- **Embed it in the URL** that `render_session_settings`
  (`backend/crates/libs/delta-bootstrap/src/settings.rs`) renders for every
  hook — thread the secret into that function's signature (currently `port`
  only) and `Config::session_settings_json()`. Use a **query parameter**, e.g.
  `http://127.0.0.1:{port}/hooks/{path}?hs={secret}`. The URL form works
  uniformly for both the `http`-type hooks and the `curl_post` command hook —
  and critically, the fake-claude consumer reads the whole URL
  (`fake-claude/src/settings.rs::hook_url`), so **no fake-claude change is
  needed**. (Do NOT use a header: `http`-type hooks have no proven header
  support in this codebase, only the curl hooks could carry `-H`.)
- **Verify it** in a middleware layered over `/hooks/*` (mirror `auth_guard`;
  reuse `auth_guard.rs::constant_time_eq`). Remove `/hooks/` from the bearer
  guard's exemption (`auth_guard.rs:57`) — `/hooks/*` now has its OWN auth (the
  `hs` query secret), and `/health` stays exempt from both. A `/hooks/*`
  request with a missing/wrong `hs` secret is rejected with **401** (or 403 —
  pick one and be consistent; 401 matches the bearer guard). Constant-time
  compare.
- **Per-session is out of scope** (see below) — per-run is the increment here.

### Part B — confine `transcript_path` to an allowed root

The hook `transcript_path` flows: `hooks/mod.rs` (payload) →
`on_user_prompt_submit.rs` / `on_session_start.rs` / `register_on_first_contact.rs`
→ **`register_session_row.rs:35`** (persists it) → `session.transcript_path` →
`sync/conversation_source.rs` → `delta-transcript/src/reader.rs:51`
(`fs::read_to_string`), driven by the background tailer
(`state.rs::spawn_transcript_tail`). The existing
`hook_transcript_guard.rs::is_foreign_transcript` only compares against the
**stored** path (subagent detection) and returns "not foreign" when nothing is
stored yet — so the **first** hook's path is trusted and stored unvalidated.

- **Validate at `register_session_row.rs`** (the single choke point every
  first-contact registration passes through), so a bad path is never persisted
  nor read. Rule: **canonicalize** the incoming `transcript_path`, require it to
  be **under an allowed root** and to **end in `.jsonl`**; reject otherwise
  (add an `Error` variant, e.g. `Error::InvalidTranscriptPath`, mapped to a
  sensible status in `api_error.rs`).
- **Allowed root derivation** (must match where real Claude Code writes AND
  where the fake harness writes):
  - Real Claude Code writes transcripts under `<claude-config-dir>/projects/…`,
    where the config dir is `$CLAUDE_CONFIG_DIR` if set, else `$HOME/.claude`.
    So the default allowed root is `${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects`.
  - Make it **overridable via `DELTA_TRANSCRIPT_ROOT`** so tests and the fake
    harness (which write elsewhere) still validate. Thread the resolved root
    through `Config` → `Interactor` (like `session_workdir_base`), so
    `register_session_row` can see it.
  - Reuse the `HOME` derivation pattern from
    `interactor/workdir/home_dir.rs` / the `$HOME/.claude.json` derivation in
    `git-worktree/src/git.rs`.
- Canonicalize to defeat `..` escapes; a path that does not canonicalize (does
  not exist yet) should be handled deliberately — canonicalize the parent and
  check the prefix, or normalize lexically — pick an approach that does not
  reject a legitimate not-yet-created transcript. Comment the choice.

### Test blast radius (do this up front — mirrors the lessons from the earlier hardening PRs)

Requiring the `hs` secret and confining the transcript root breaks every test
that POSTs to `/hooks/*` or supplies a `/tmp/*.jsonl` transcript path:

- **`backend/crates/apps/delta-server/src/app/tests/hooks.rs`** — every
  `/hooks/*` `.oneshot(...)` needs the `?hs=<secret>` query (read the built
  `AppState`'s hook secret via a helper, mirroring the existing `bearer()`
  helper) AND a `transcript_path` under the allowed root (set
  `DELTA_TRANSCRIPT_ROOT` on the test `Config`, or point the paths at a temp
  root the test configures). ADD the two negative tests here:
  a forged hook with no/invalid `hs` → rejected (test name contains
  `rejects_a_hook_without_the_secret`), and a `transcript_path` outside the
  allowed root → refused (test name contains
  `rejects_a_transcript_path_outside_the_allowed_root`).
- **`backend/crates/apps/delta-server/tests/end_to_end.rs`** — its `post_json`
  helper's `/hooks/*` calls need the `hs` secret, and its transcript paths need
  to be under the configured root.
- **`backend/crates/apps/fake-claude/tests/full_loop.rs`** — the `hs` secret
  rides in the rendered URL, so fake-claude needs no change for auth; BUT it
  sets `FAKE_CLAUDE_TRANSCRIPT_DIR` (full_loop.rs ~L222), so the in-process
  `Config` must set `DELTA_TRANSCRIPT_ROOT` to that same dir (or a parent) so
  the backend allows the fake transcripts.
- **`frontend/packages/apps/web/e2e-fake/support/server.ts`** — it exports
  `FAKE_CLAUDE_TRANSCRIPT_DIR` (~L315) to the fake binary; set
  `DELTA_TRANSCRIPT_ROOT` to the SAME directory in the backend spawn `env`
  block so the real backend allows the transcripts the fake writes. **This is
  the integration seam most likely to fail the e2e-fake gate if missed** — get
  it right up front.
- **`backend/crates/apps/delta-server/tests/real_claude_canary.rs:213`** —
  renders settings for real `claude`; pass the secret; the real transcript is
  under `~/.claude/projects` so the default root covers it.
- **`fake-codex`** full_loop harness — if it POSTs to `/hooks/*` or supplies
  transcript paths, apply the same treatment.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A POST to a `/hooks/*` route with a missing or wrong `hs` secret is
      rejected (401/403) before the handler runs; a POST with the valid secret
      reaches the handler (test name contains `rejects_a_hook_without_the_secret`;
      grepped by `check_command`). `/health` remains reachable with neither the
      bearer token nor the hook secret.
- [x] A hook whose `transcript_path` is outside the allowed root (e.g.
      `/etc/passwd` or `/tmp/evil.jsonl` when the root is `~/.claude/projects`)
      is refused and the path is never persisted nor read from disk (test name
      contains `rejects_a_transcript_path_outside_the_allowed_root`; grepped by
      `check_command`); a path under the allowed root ending in `.jsonl` is
      accepted.
- [x] `make check` passes green — the hook middleware, the settings URL change,
      the transcript confinement, and every hook-driving test/harness
      (delta-server hooks tests, end_to_end, fake-claude full_loop, fake-codex,
      e2e-fake) pass, with the transcript root aligned to where the fake
      harness writes.

### Manual / on-hardware (verified by a human before merge)

- [ ] A live `make dev` session with real `claude` works end to end: hooks fire
      and are accepted (the rendered `hs` secret matches), the transcript under
      `~/.claude/projects` is read and the conversation updates, and a forged
      `curl` POST to `/hooks/user-prompt-submit` without the secret is rejected.
      (Non-blocking for merge under the agreed CI-green autonomous policy;
      recorded for dogfooding.)

## Out of scope

- **Per-session** hook secret (binding the secret to a specific `session_id`).
  Per-run is implemented here; per-session requires moving the settings render
  per-spawn and using a per-session settings path to avoid the shared-file race
  — a larger refactor. Follow-up: a compromised agent that learns the per-run
  secret could forge hooks for a *different* session; per-session would scope a
  leaked secret to its own session. Noted for a later PR.
- Stopping the `additionalContext` conversation-excerpt INFO log
  (`hooks/mod.rs:74`) — that is a separate hardening item.
- Changing the temp settings-file permissions (a separate item) — the `hs`
  secret now lives in that file, which strengthens the case for that follow-up.
