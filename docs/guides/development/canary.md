# Real-agent canaries

## Overview

Contract-monitoring suites that run against the **real, authenticated agent
CLIs** — never in CI, always on demand (or behind the opt-in trigger below):

- `make e2e-real` — the real-claude canary suite, checking the fake-claude
  lane's recording of claude's implicit contract against reality.
- `make e2e-real-codex` — the real-codex canaries, checking the Codex
  app-server wire contract against the real `codex app-server`.
- `make e2e-real-auto` — a gated wrapper that runs `e2e-real` only when the
  installed `claude` version changed, for a periodic driver.

The scripted lanes these canaries keep honest are documented in
[e2e.md](e2e.md).

## Real-claude canaries (`make e2e-real`)

The fake-claude lane ([e2e.md](e2e.md)) is a *recording* of claude's implicit
contract — the hook events and payload fields, the JSONL transcript shapes,
the interrupt marker, queued prompts, `isMeta` flagging, the
permission-decision envelope. The real-claude canary suite checks that
recording against reality:

```bash
make e2e-real
```

Its role is **contract monitoring, not feature testing**: it exists to detect
upstream format/behavior drift that would silently break Delta's transcript
parsing and hook handling. It is two layers, cheapest first:

1. **Rust contract canaries**
   (`backend/crates/apps/delta-server/tests/real_claude_canary.rs`): drive the
   real `claude` in tmux directly, with Delta's exact spawn shape (rendered
   `--settings`, `--session-id`, positional prompt), capturing the raw hook
   POSTs and the raw transcript JSONL — no server, no browser. Each test's
   doc comment lists exactly what it pins.
2. **One Playwright smoke spec** (`packages/apps/web/e2e-real/`): browser →
   real `delta-server` → tmux → real claude → transcript → browser, proving
   the full loop closes against the real binary. `scripts/e2e-real.sh` boots
   the backend with the same per-run isolation as the fake lane (temp
   database, per-run tmux socket, dedicated ports 7897/5197).

The suite drives the **real, authenticated `claude` CLI and consumes the local
user's subscription quota** — every canary uses the smallest workable prompt
(a handful of real turns per run), assertions are structural only (never about
response wording, which is non-deterministic), and each canary retries exactly
once. Run it on demand — after a claude version bump, before relying on a new
upstream behavior, or when the real loop misbehaves while the fake lane is
green — or let a periodic driver run it for you (see "Automatic canary
trigger" below). It is deliberately **not wired into CI**: GitHub runners have
no authenticated `claude`, and a contract canary against a live model does not
belong in a merge gate.

**When a canary breaks (drift runbook).** A red canary means the upstream
contract changed, not that the canary is flaky (it already retried once).
The two-layer sync rule:

1. Update **fake-claude's scenario engine** (and its transcript/hook writers)
   to re-enact the new reality, so `make e2e-fake` exercises what claude
   actually does today — the fake lane must never stay green by re-enacting a
   contract that no longer exists.
2. Update **Delta's parsing/handling** (`delta-transcript`,
   `delta-attribution`'s `claude_format`, the hook wire types and handlers) to
   the new reality, keeping compatibility with old recorded transcripts where
   resume needs it.
3. Re-run `make e2e-real` to confirm the canary is green against the new
   contract, and `make check && make e2e && make e2e-fake` to confirm the
   re-enactment still proves the loop.

Drift already pinned by the suite and synced (the queued-prompt format): a
prompt typed while a turn is in flight is no longer written as a
`queued_command` attachment line — current claude records a uuid-less
`{"type":"queue-operation","operation":"enqueue",…}` line at submit time and
replays the prompt as a plain `type:"user"` line (`promptSource: "queued"`,
firing its own `UserPromptSubmit`) when it dequeues. Both layers follow the
new shape: fake-claude re-enacts it with its `enqueue_prompt`/`dequeue_prompt`
steps, and the parser deliberately skips the uuid-less bookkeeping line while
the replayed user line flows the normal attribution path (pinned by the
`queue_operation_dequeue*` corpus cases in `delta-attribution`). The parser's
`queued_command` special case is kept as **legacy-format compatibility**:
transcripts recorded by older claude versions are still resumed and viewed,
so that path must not be cleaned up. Delta's own dispatch is unaffected
either way — it holds browser-composed sends in its own queue and only types
them into an idle pane, so claude-side queueing happens only for prompts
typed directly into the TUI.

Two environment facts the suite handles for you (relevant when running any
real-claude loop by hand):

- A `claude` that inherits nested-session environment markers (`CLAUDECODE`,
  `CLAUDE_CODE_*` — set inside a Claude Code session) does **not** persist its
  transcript JSONL, which silently breaks transcript ingestion. The suite
  strips them from the spawned processes; a `make dev` started from inside a
  Claude Code session would hit the same wall.
- Session workdirs are kept inside this repository (not `/tmp`) so a host that
  has already trusted the repository never sees claude's first-run trust
  prompt mid-suite. The browser smoke additionally anchors its workdir at the
  *main* checkout's root (resolved via `git rev-parse --git-common-dir`): the
  workdir picker hides dot-directories, and a linked git worktree typically
  lives under one, so the picker could never navigate into a worktree path.

## Real-codex canaries (`make e2e-real-codex`)

The Codex counterpart: Rust canaries
(`backend/crates/gateway/codex-agent/tests/real_codex_canary.rs`) that drive
the real `codex app-server` — one safe turn end to end, the thread-metadata
wire fields, the worktree sandbox grant (that the dotted `config` key Delta
injects really reaches a thread's effective writable roots), and schema drift
detection against the vendored app-server schema. Only the turn canary consumes
Codex quota. `DELTA_CODEX_BIN` overrides
the binary. Like the claude suite it is local-only, never wired into CI, and
worth a run after a codex version bump or when the real Codex loop misbehaves
while the `fake-codex` re-enactment is green. It has no auto-gating wrapper
yet.

## Automatic canary trigger (opt-in)

```bash
make e2e-real-auto    # gated: runs e2e-real only when it is worth a run
```

`scripts/e2e-real-auto.sh` is a gating wrapper meant to be invoked by a
periodic driver. Each invocation runs `make e2e-real` only when **both** hold:

- the installed `claude --version` (respecting `DELTA_CLAUDE_BIN`) differs
  from the version recorded at the last attempt, **and**
- at least 24 hours have passed since the last attempt.

Otherwise it exits 0 with a one-line `skipped (reason)`. Claude auto-updates
frequently — sometimes several times a day — and each suite run costs a
handful of real subscription turns, so the gate caps automatic spend at one
run per day, spends nothing on days without an update, and never misses an
update (a version change inside the debounce window runs on a later tick).
On a host without `claude` the wrapper exits 0 quietly: it simply is not a
canary host, so the same timer can be installed everywhere.

**State and logs** live per host (every checkout/worktree shares the host's
claude and quota, so they share one gate), under
`${XDG_STATE_HOME:-$HOME/.local/state}/delta/e2e-real/`:

- `last-attempt` — `key=value` lines: the claude `version`, attempt
  `epoch`/`date`, the `result` (`success` / `failure (exit N)` /
  `interrupted`), and the `log` path of that run.
- `logs/` — full output of recent runs (the newest 10 are kept).
- `lock` — `flock` guard shared with `scripts/e2e-real.sh`, so a periodic
  tick never overlaps an in-flight suite run, including a manual
  `make e2e-real` from any checkout (the tick skips and tries again later).

**The debounce is on the attempt, not on success.** A red canary usually
means real upstream drift; auto-retrying it hourly would burn quota without
new information. A failure is loud instead: the wrapper exits non-zero (the
systemd unit shows as failed), prints a `FAILURE:` line with the saved log
path, records `result=failure` in `last-attempt`, and fires a `notify-send`
desktop notification when available (best-effort). When that happens, read
the run log and follow the drift runbook above; the next automatic run
happens once claude updates again (or run `make e2e-real` manually after the
fix — manual runs are not gated).

**Periodic driver (systemd user timer).** A ready-made unit pair lives in
`scripts/systemd/`. It is opt-in: nothing installs it for you, and the
service file's `DELTA_REPO` must point at your checkout. Install:

```bash
cp scripts/systemd/delta-e2e-real.{service,timer} ~/.config/systemd/user/
"$EDITOR" ~/.config/systemd/user/delta-e2e-real.service   # set DELTA_REPO
systemctl --user daemon-reload
systemctl --user enable --now delta-e2e-real.timer
```

The timer ticks hourly (`Persistent=true`, so a machine that was off catches
up on boot); almost every tick is an immediate skip — the gate, not the
timer, decides when quota is spent. Inspect it with:

```bash
systemctl --user list-timers delta-e2e-real.timer   # next/last tick
journalctl --user -u delta-e2e-real.service -n 50   # gate decisions + failures
cat ~/.local/state/delta/e2e-real/last-attempt      # last attempt summary
```

Uninstall:

```bash
systemctl --user disable --now delta-e2e-real.timer
rm ~/.config/systemd/user/delta-e2e-real.{service,timer}
systemctl --user daemon-reload
```

**Cron alternative** for non-systemd hosts (the login shell `bash -lc` gives
the run the same PATH as an interactive terminal):

```cron
0 * * * * bash -lc 'make -C "$HOME/repos/delta" e2e-real-auto' >> "$HOME/.local/state/delta/e2e-real/cron.log" 2>&1
```

**Testing the gate without spending quota:** point `DELTA_CLAUDE_BIN` at a
stub that prints a fake version, set `XDG_STATE_HOME` to a temp dir, and set
`E2E_REAL_CMD` (testing-only override, run via `bash -c`) to a stub command —
the wrapper then exercises every gate branch without touching the real suite.
