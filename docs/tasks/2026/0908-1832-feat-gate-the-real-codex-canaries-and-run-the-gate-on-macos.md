---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && bash scripts/tests/e2e-real-gate.test.sh && ! grep -q "date -d" scripts/e2e-real-gate.sh && ! grep -qE "head -n -" scripts/e2e-real-gate.sh && grep -q "e2e-real-codex" scripts/e2e-real-gate.sh && grep -q "DELTA_CODEX_BIN" scripts/e2e-real-gate.sh'
assignee: null
branch: task/0908-1832-feat-gate-the-real-codex-canaries-and-run-the-gate-on-macos
created_at: 2026-09-08T18:32:42Z
updated_at: 2026-09-08T19:36:01Z
---

# feat(canary): gate the real-codex canaries and make the gate runnable on macOS

## Overview

`make e2e-real-gate` (`scripts/e2e-real-gate.sh`) is the gated trigger for the
real-agent canaries: when invoked (by hand, or by a periodic driver) it runs a
suite only if the installed CLI's version changed since the last attempt AND
at least 24h have passed. Today it knows **one** provider — it keys on
`claude --version` and runs `make e2e-real-claude`. The Codex counterpart,
`make e2e-real-codex` (`backend/crates/gateway/codex-agent/tests/real_codex_canary.rs`:
one safe turn, the thread-metadata fields, the worktree git grant, and the
vendored app-server **schema drift** check), has no gate at all —
`docs/guides/development/canary.md` says so ("It has no auto-gating wrapper
yet"). The consequence showed on 2026-09-09: the vendored schema pin sat at
codex-cli 0.144.4 while the installed CLI was 0.153.4, nine minor versions of
drift nobody noticed until a by-hand run (fixed in #382).

Second problem: the gate script assumes GNU userland. `flock` is required
(`die` if missing), timestamps use `date -d "@$epoch"`, and log rotation uses
`head -n -"$KEEP_LOGS"`. None of these exist on stock macOS, so on the
development machine the script cannot run even by hand (observed: `date:
illegal option -- d`, `head: illegal line count -- -10`).

### What to build

1. **Make the gate provider-aware.** `scripts/e2e-real-gate.sh` iterates over
   the providers `claude` and `codex` (in that order) and applies the same
   gate to each, independently:

   | provider | binary (override)            | version probe       | suite                    |
   | -------- | ---------------------------- | ------------------- | ------------------------ |
   | claude   | `claude` (`DELTA_CLAUDE_BIN`) | `claude --version`  | `make e2e-real-claude`   |
   | codex    | `codex` (`DELTA_CODEX_BIN`)   | `codex --version`   | `make e2e-real-codex`    |

   - A provider whose binary is missing is **skipped quietly** for that
     provider only (same "not a canary host" semantics as today), and the
     other provider still runs.
   - Per-provider state: `${XDG_STATE_HOME:-$HOME/.local/state}/delta/e2e-real/<provider>/last-attempt`
     and `.../<provider>/logs/`. Keep the existing key=value format
     (`version`, `epoch`, `date`, `result`, `log`). On first run, if the legacy
     `delta/e2e-real/last-attempt` exists and `claude/last-attempt` does not,
     move it into place so an existing claude host does not re-run its suite
     for no reason.
   - One shared lock for the whole tick (today's `delta/e2e-real/lock`),
     still honoured by `scripts/e2e-real-claude.sh` via `DELTA_E2E_REAL_LOCK_HELD=1`.
   - The debounce stays 24h for both providers (the codex turn canary
     consumes quota too).
   - A failing suite for one provider does not stop the other provider's
     gate. The tick exits non-zero if any suite failed, prints one `FAILURE:`
     line per failed provider, and ends with a one-line summary of what each
     provider did (`ran: success` / `ran: failure (exit N)` / `skipped (…)`).
   - Testing-only override: `E2E_REAL_CMD` keeps its meaning (replaces the
     suite command) and applies to whichever provider is running; add
     `E2E_REAL_GATE_PROVIDERS` (space-separated) to restrict the provider
     list, so a test can exercise one provider at a time.
   - Log prefix stays `[e2e-real-gate]`; provider name appears in every line
     that is about a provider.

2. **Make the script run on stock macOS** without changing behaviour on Linux:

   - `flock` when available; otherwise an atomic `mkdir`-based lock directory
     holding the owner pid, released by `trap`, reclaimed when the recorded
     pid is dead. A missing `flock` is no longer fatal.
   - No `date -d`. The attempt timestamp *is* now, so format the current time
     directly (`date -u +%Y-%m-%dT%H:%M:%SZ`, `date +%Y%m%d-%H%M%S`).
   - No `head -n -N`. Keep the newest `KEEP_LOGS` logs with a portable
     construction (e.g. `sort -r | tail -n +$((KEEP_LOGS + 1))`).
   - Desktop notification: `notify-send` when present, else `osascript -e
     'display notification …'` when present, else nothing — all best-effort.

3. **A test for the gate that spends no quota**:
   `scripts/tests/e2e-real-gate.test.sh` (bash, self-contained, exits
   non-zero on the first failed assertion). It builds a temp dir with stub
   `claude` / `codex` binaries that print a version, points `XDG_STATE_HOME`
   at a temp state dir, sets `E2E_REAL_CMD` to a stub that records which
   provider ran (via an env var the gate exports, e.g.
   `E2E_REAL_GATE_PROVIDER`), and asserts at least:

   - tick 1: both providers run; both `<provider>/last-attempt` files exist
     with `result=success`;
   - tick 2 (same versions): both skipped, no suite invocation;
   - tick 3 (codex version bumped, still inside the debounce): codex deferred
     with the "runs on a later tick" reason, claude skipped as unchanged;
   - a tick where the stub suite fails for codex only: exit code non-zero,
     `codex/last-attempt` says `result=failure (exit …)`, and claude's gate
     still evaluated (skipped as unchanged);
   - legacy migration: a pre-seeded `delta/e2e-real/last-attempt` is moved to
     `claude/last-attempt` and honoured (claude skipped as unchanged);
   - `E2E_REAL_GATE_PROVIDERS=codex` runs only codex.

   The check phase runs this test (it is in `check_command`), so it must pass
   on macOS and Linux with only bash, coreutils and the shims the test itself
   creates. `flock` may be absent.

4. **Docs**: update `docs/guides/development/canary.md` (the "Real-codex
   canaries" paragraph no longer says there is no gate; the "Automatic canary
   trigger" section describes both providers, the per-provider state layout,
   the migration, and that stock macOS now works — while the periodic
   *driver* remains opt-in and only systemd/cron examples are shipped),
   `docs/guides/compatibility.md` ("retained tool" paragraph), and the
   `Makefile` help line for `e2e-real-gate`. Update the header comment of
   `scripts/e2e-real-gate.sh` to match.

Keep the script's existing structure and tone (section banners, `log`/`die`,
comments explaining *why*). Prefer a loop over duplicated per-provider blocks.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `scripts/e2e-real-gate.sh` gates both providers: it references
      `e2e-real-codex` and `DELTA_CODEX_BIN` (both greps are in
      `check_command`), keeps per-provider state under
      `delta/e2e-real/<provider>/`, and migrates the legacy claude state file.
- [x] The script contains no `date -d` and no `head -n -` (both negative
      greps are in `check_command`); a missing `flock` is not fatal.
- [x] `scripts/tests/e2e-real-gate.test.sh` exists and passes (it is in
      `check_command`), covering: both run on first tick; both skip on
      unchanged versions; a version change inside the debounce is deferred; a
      failing provider yields a non-zero exit while the other provider is
      still evaluated; the legacy state file is migrated;
      `E2E_REAL_GATE_PROVIDERS` restricts the list.
- [x] `make check` stays green (the Rust and TypeScript trees are untouched
      except for comments, if at all).

### Manual / on-hardware (verified by a human before merge)

- [ ] On the macOS development machine with the real `claude` and `codex`
      installed and a fresh state dir, `make e2e-real-gate` runs both suites
      on the first tick and skips both on the second, with the summary line
      naming each provider's outcome. (Consumes one claude suite run and one
      codex turn.)
- [ ] A deliberately failing codex suite (e.g. temporarily point
      `DELTA_CODEX_BIN` at a stub whose `--version` differs and whose
      app-server does not exist) produces the `FAILURE:` line and a macOS
      notification via `osascript`, and the tick exits non-zero.

## Out of scope

- Installing or shipping a periodic driver for macOS (launchd) or wiring the
  gate into CI. The driver question is tracked separately; this task only
  makes the gate correct and runnable when something invokes it.
- Automatically re-vendoring the schema or opening a PR when the codex drift
  check fails. The gate reports; a human (or a Claude Code session) re-vendors.
- Normalising the vendored schema files (`jq -S`) to shrink re-vendor diffs.
- Changing what `make e2e-real-claude` / `make e2e-real-codex` themselves run,
  or `scripts/e2e-real-claude.sh` beyond keeping the shared-lock handshake
  working.
- The Playwright `e2e-real/` directory name and the `E2E_REAL_*` variable
  names.
