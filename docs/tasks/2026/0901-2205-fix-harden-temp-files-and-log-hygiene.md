---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -rq "writes_settings_with_owner_only_permissions" backend/crates && grep -rq "refuses_to_write_settings_through_a_symlink" backend/crates && grep -rq "writes_the_conf_with_owner_only_permissions" backend/crates && grep -rq "refuses_to_write_the_conf_through_a_symlink" backend/crates && grep -q "dangerouslySetInnerHTML" frontend/eslint.config.js && ! grep -q "<none>" backend/crates/apps/delta-server/src/hooks/mod.rs && grep -qi "settings.json" docs/guides/security.md'
assignee: null
branch: task/0901-2205-fix-harden-temp-files-and-log-hygiene
created_at: 2026-09-01T22:05:20Z
updated_at: 2026-09-02T02:35:00Z
---

# fix: harden temp-file creation, stop logging conversation excerpts, and guard raw-HTML sinks

## Overview

Three defensive-hardening items, bundled because each is small and none
changes user-visible behavior.

### 1. Temp-file hardening (settings.json and the tmux conf)

Delta writes two files under `std::env::temp_dir()` with no permission or
symlink handling. On macOS `$TMPDIR` is per-user 0700, so the practical
exposure is Linux `/tmp` (world-writable, sticky) — a shared-host / other-UID
threat model; say so in code comments so the trade-off is legible.

- **settings.json** — path from `Config::session_settings_path()`
  (`backend/crates/libs/delta-bootstrap/src/lib.rs:188-194`):
  `temp_dir()/delta-<port>/settings.json`, port-predictable. Written by
  `FsWorkspace::write` (`backend/crates/gateway/workspace-fs/src/workspace.rs:22-29`)
  via bare `create_dir_all` (0755) + `tokio::fs::write` (0644, **follows
  symlinks**). The content embeds the per-run hook secret in every hook URL
  (`backend/crates/libs/delta-bootstrap/src/settings.rs:31`) and its
  `statusLine` / `SessionStart` entries are commands Claude Code executes —
  so the file is both a secret-read surface and a command-injection surface
  if another local user can pre-plant a symlink or swap the directory.
- **tmux conf** — `Tmux::new` (`backend/crates/gateway/tmux-driver/src/tmux.rs:85-91`)
  derives `temp_dir()/delta-tmux-<socket>.conf` (production socket is the
  constant `delta` → fully predictable path) and `create_session` writes it
  with bare `tokio::fs::write` (`tmux.rs:154-159`). No secrets inside, but
  tmux loads it with `-f` and executes its directives — a planted symlink or
  pre-created file is a directive-injection surface.

The fix (keep the current paths and the deliberate overwrite semantics —
`workspace.rs:176-181` asserts settings are rewritten so hook URLs stay
current; the tmux write is idempotent by design, `tmux.rs:145-153`):

- Create the settings parent dir with mode **0700**
  (`DirBuilderExt::mode` — note umask does not apply to an explicit mode on
  `O_CREAT`, but `create_dir_all` without it inherits umask), and refuse a
  parent that exists as a **symlink** (`symlink_metadata`), since hardening
  only the file leaves the directory as the swap target.
- Open both files with
  `OpenOptions::new().write(true).create(true).truncate(true)` plus
  `.mode(0o600)` and `.custom_flags(libc::O_NOFOLLOW)`
  (`std::os::unix::fs::OpenOptionsExt`). `libc` is already in the workspace
  dependency table (`backend/Cargo.toml:37-38`) — add
  `libc = { workspace = true }` to `workspace-fs` and `tmux-driver`; no new
  lockfile entry. The codebase is Unix-only (no `#[cfg(windows)]` anywhere,
  tmux+PTY design, CI is ubuntu-latest), so no Windows fallback; match the
  existing `#[cfg(unix)]` style of `binary-detector` if guarding.
- New tests (CI is Linux, so mode assertions are meaningful; use `tempfile`,
  already a workspace dep — `tmux-driver` has no `[dev-dependencies]` block
  yet and needs one): a written settings file has mode 0600
  (`writes_settings_with_owner_only_permissions`), a symlinked target is
  refused (`refuses_to_write_settings_through_a_symlink`), and the same pair
  for the tmux conf (`writes_the_conf_with_owner_only_permissions`,
  `refuses_to_write_the_conf_through_a_symlink`).
- Existing tests to keep green: `overwrites_existing_settings`
  (`workspace.rs:162-181` — hard-breaks under `create_new`, so keep
  truncate), `writes_settings_creating_parent_dirs` (`workspace.rs:146-160`
  — keep recursive dir creation), `conf_path_is_derived_per_socket`
  (`tmux.rs:559-571` — keep the filename). The fake-claude full loop
  (`backend/crates/apps/fake-claude/tests/full_loop.rs:80-100`) exercises the
  real write + real tmux in CI and proves the hardened files stay readable.
- Fold-in: `backend/crates/apps/fake-codex/tests/full_loop/support.rs:183`
  uses a hard-coded shared `"/tmp/delta-codex-full-loop-settings.json"`;
  under 0600 a leftover file owned by another UID on a shared box would fail
  the suite — switch it to a per-pid temp path.

### 2. Stop logging conversation excerpts

`backend/crates/apps/delta-server/src/hooks/mod.rs:72-78` logs the
`additionalContext` string at INFO. That string is a framed **verbatim
conversation excerpt** (`frame_locator_context.rs:20-29` /
`frame_thread_switch_context.rs:23-45` embed the selected quote), so
conversation text lands in the server log. Replace the content field with a
non-content signal — e.g. `injected = additional_context.is_some()` and/or
`additional_context_len` — keeping the log line itself (it is a useful
control-plane milestone). No test asserts on this log (verified); nothing
else in `delta-server/src` logs prompt/transcript/quote text, so this one
site completes the fix.

### 3. ESLint guard against raw-HTML sinks

Frontend markdown is rendered by `react-markdown` without `rehype-raw`
(`AssistantMarkdown.tsx:1-17`), so raw HTML from the model is inert today;
this guard prevents regression. Do **not** add `eslint-plugin-react` — the
core `no-restricted-syntax` rule with esquery selectors works on the
TS/JSX AST as parsed by `@typescript-eslint/parser` in the existing flat
config (`frontend/eslint.config.js`, currently 18 lines, no `rules:` block
yet; one root config governs all six packages via `pnpm -r lint`). Add a
single rules block (the rule is not additive across config objects — keep
all selectors in one array):

- `JSXAttribute[name.name="dangerouslySetInnerHTML"]` — error, repo-wide.
- `Property[key.name="dangerouslySetInnerHTML"]` — catches the
  spread-object form.
- `MemberExpression[property.name="innerHTML"]` — error outside test files;
  the one existing hit is a test fixture helper
  (`frontend/packages/apps/web/src/features/transcript/branchHighlight.test.ts:6`),
  so either scope this selector with a `files`-based relax block for
  `**/*.test.ts?(x)` or leave a scoped eslint-disable at that line.

Put the rationale in a short comment in the config file. All six packages
must stay lint-green (`pnpm -r lint`; the web package also runs
dependency-cruiser after eslint).

### Docs

Add a short "Temp-file hardening" note to `docs/guides/security.md`
(sections already cover the bearer token, hook secret, trust seeding, and
dangerous launch options): what the two files are, why they are 0600/0700
with symlink refusal, and that the realistic exposure is multi-user Linux
`/tmp`. Mention the log-hygiene rule (server logs carry no conversation
text) in the same doc where it fits naturally.

### Test blast radius

- `workspace-fs` unit tests (`workspace.rs:142-292`) — two existing tests
  listed above must stay green; new permission/symlink tests land here.
- `tmux-driver` — gains its first `[dev-dependencies]` (tempfile); existing
  `conf_path_is_derived_per_socket` / `fixed_config_pins_the_deliberate_settings`
  stay green.
- fake-claude / fake-codex full-loop suites drive the real writes in CI;
  fake-codex needs the shared-path fix above.
- e2e-real cleanup paths reference the tmux conf path
  (`backend/crates/apps/delta-server/tests/real_claude_canary.rs:287-292`,
  `scripts/e2e-real.sh:130`) — unchanged if the path stays; do not rename.
- Frontend: no source changes, only the lint rule — `pnpm -r lint` across
  all six packages is the gate; e2e specs are linted too (verified: no
  `innerHTML` hits there).

Session-state coverage: not applicable — no user-triggerable operation
changes; the changes are file permissions, a log field, and a lint rule.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The settings file is written 0600 inside a 0700 parent dir and a
      symlinked settings path is refused before any bytes are written —
      test names contain `writes_settings_with_owner_only_permissions` and
      `refuses_to_write_settings_through_a_symlink` (grepped by
      `check_command`); the existing overwrite and parent-dir-creation tests
      still pass.
- [x] The tmux conf is written 0600 and a symlinked conf path is refused —
      test names contain `writes_the_conf_with_owner_only_permissions` and
      `refuses_to_write_the_conf_through_a_symlink` (grepped); the conf path
      and filename are unchanged.
- [x] `backend/crates/apps/delta-server/src/hooks/mod.rs` no longer logs the
      `additionalContext` content (the `"<none>"` placeholder is gone —
      negative grep in `check_command`); the log line still reports whether
      context was injected.
- [x] `frontend/eslint.config.js` rejects `dangerouslySetInnerHTML` (and
      `innerHTML` outside tests) via `no-restricted-syntax` (grepped), and
      `pnpm -r lint` passes for all six packages.
- [x] `docs/guides/security.md` documents the temp-file hardening (grep for
      `settings.json` in that doc via `check_command`).
- [x] The fake-codex full-loop support no longer hard-codes a shared
      `/tmp/delta-codex-full-loop-settings.json` path (per-pid temp path
      instead), and `make check` (including e2e-fake with a real tmux) is
      green.

### Manual / on-hardware (verified by a human before merge)

- [ ] Live: `make dev` still boots, a Claude session spawns with hooks
      working (settings.json readable by `claude`), and tmux panes open
      normally with the hardened conf. (Non-blocking for merge under the
      agreed CI-green autonomous policy; recorded for dogfooding.)

## Out of scope

- Relocating the temp files to `$HOME/.delta` — the hardened in-place write
  achieves the same protection without touching cleanup scripts and path
  contracts.
- Root-confining `GET /api/workdir/list`: the per-run bearer token already
  gates the endpoint (arbitrary local processes can no longer call it), and
  a root constraint needs a design decision about which roots the directory
  picker may browse — deliberately deferred, not forgotten.
- Adding `eslint-plugin-react` or any new frontend dependency.
- Redacting or restructuring any other log site (verified none carries
  conversation text).
- Windows support for the permission flags (the codebase is Unix-only).
