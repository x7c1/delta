# Real-agent canaries

## Overview

Contract-monitoring suites that run against the **real, authenticated agent
CLIs** — never in CI, always on demand (or behind the opt-in trigger below):

- `make e2e-real-claude` — the real-claude canary suite, checking the fake-claude
  lane's recording of claude's implicit contract against reality.
- `make e2e-real-codex` — the real-codex canaries, checking the Codex
  app-server wire contract against the real `codex app-server`.
- `make e2e-real-gate` — a gated wrapper that runs each suite above only when
  that CLI's installed version changed, for a periodic driver.

The scripted lanes these canaries keep honest are documented in
[e2e.md](e2e.md).

## Real-claude canaries (`make e2e-real-claude`)

The fake-claude lane ([e2e.md](e2e.md)) is a *recording* of claude's implicit
contract — the hook events and payload fields, the JSONL transcript shapes,
the interrupt marker, queued prompts, `isMeta` flagging, the
permission-decision envelope. The real-claude canary suite checks that
recording against reality:

```bash
make e2e-real-claude
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
   the full loop closes against the real binary. `scripts/e2e-real-claude.sh` boots
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
3. Re-run `make e2e-real-claude` to confirm the canary is green against the new
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
while the `fake-codex` re-enactment is green — or let the gate below run it
for you when `codex` updates, which is what keeps the vendored schema from
drifting unnoticed.

## Automatic canary trigger (opt-in)

```bash
make e2e-real-gate    # gated: runs each suite only when it is worth a run
```

`scripts/e2e-real-gate.sh` is a gating wrapper meant to be invoked by hand or
by a periodic driver. Each invocation walks both providers:

| provider | binary (override) | suite |
|----------|-------------------|-------|
| `claude` | `claude` (`DELTA_CLAUDE_BIN`) | `make e2e-real-claude` |
| `codex`  | `codex` (`DELTA_CODEX_BIN`)   | `make e2e-real-codex`  |

and runs that provider's suite only when **both** hold:

- the installed `<binary> --version` differs from the version recorded at that
  provider's last attempt, **and**
- at least 24 hours have passed since that attempt.

Otherwise that provider is skipped with a one-line `<provider>: skipped
(reason)`, and the tick ends with a summary naming what each provider did
(`ran: success` / `ran: failure (exit N)` / `skipped (…)`). Both CLIs
auto-update frequently — sometimes several times a day — and both suites cost
real subscription quota (the claude suite a handful of turns, the codex
canaries one safe turn plus the schema-drift check), so the gate caps
automatic spend at one run per provider per day, spends nothing on days
without an update, and never misses an update (a version change inside the
debounce window runs on a later tick).

The providers are independent. A host without one of the CLIs skips only that
provider and still gates the other — it simply is not a canary host for the
missing one, so the same timer can be installed everywhere — and a failing
suite for one provider still leaves the other provider's gate evaluated and
run. The tick exits non-zero if any suite failed.

**State and logs** live per host (every checkout/worktree shares the host's
CLIs and quota, so they share one gate), under
`${XDG_STATE_HOME:-$HOME/.local/state}/delta/e2e-real/`:

- `<provider>/last-attempt` — `key=value` lines: the CLI `version`, attempt
  `epoch`/`date`, the `result` (`success` / `failure (exit N)` /
  `interrupted`), and the `log` path of that run.
- `<provider>/logs/` — full output of that provider's recent runs (the newest
  10 are kept). A suite's own output goes only there, never to the terminal, so
  a manual `make e2e-real-gate` is quiet for as long as the suite takes; the
  tick prints the log path before starting the run, to `tail -f` if you want to
  watch it.
- `lock` — overlap guard for the whole tick, shared with
  `scripts/e2e-real-claude.sh`, so a periodic tick never overlaps an in-flight
  suite run, including a manual `make e2e-real-claude` from any checkout (the
  tick skips and tries again later). It is `flock` where available and an
  atomic `lock.d` directory (holding the owner pid, reclaimed when that pid is
  gone) otherwise. Only the `flock` guard covers manual runs: on a host
  without `flock`, `make e2e-real-claude` takes no lock at all (a manual run
  stays an explicit "run it now"), so a tick that starts while one is in
  flight will collide with it on the suite's fixed ports — worth knowing if
  you hand-roll a periodic driver on macOS.

A host set up before the gate knew about Codex has its claude state in the
root `last-attempt` file; the first tick moves it to `claude/last-attempt`, so
migrating costs no re-run.

The wrapper runs on **stock macOS as well as Linux**: it needs no `flock` and
no GNU-only `date`/`head` flags, and the failure notification falls back from
`notify-send` to `osascript` (and to nothing at all when neither exists).
Only the periodic *driver* is Linux-flavoured — the shipped examples are a
systemd user timer and a cron line; on macOS, invoke the gate however you
prefer (by hand, or from a driver you install yourself).

**The debounce is on the attempt, not on success.** A red canary usually
means real upstream drift; auto-retrying it hourly would burn quota without
new information. A failure is loud instead: the wrapper exits non-zero (the
systemd unit shows as failed), prints one `FAILURE:` line per failed provider
with the saved log path, records `result=failure` in that provider's
`last-attempt`, and fires a best-effort desktop notification. It stays visible
afterwards: while that CLI's version is unchanged, every later tick repeats the
verdict in that provider's skip line and in the tick summary (`skipped (version
unchanged; last attempt: failure (exit 3))`) with the log path, so a red canary
does not read as green once the `FAILURE:` line has scrolled away. When that
happens, read the run log and follow the drift runbook above (for codex, a
red schema-drift check means re-vendoring the app-server schema); the next
automatic run happens once that CLI updates again (or run the suite manually
after the fix — manual runs are not gated).

A manual run does not touch the gate's record, so the repeated verdict stays
until the gate itself runs that provider again;
`rm ~/.local/state/delta/e2e-real/<provider>/last-attempt` clears it and makes
the next tick re-run that suite from scratch.

**Periodic driver (systemd user timer).** A ready-made unit pair lives in
`scripts/systemd/`. It is opt-in: nothing installs it for you, and the
service file's `DELTA_REPO` must point at your checkout. Install:

```bash
cp scripts/systemd/delta-e2e-real-gate.{service,timer} ~/.config/systemd/user/
"$EDITOR" ~/.config/systemd/user/delta-e2e-real-gate.service   # set DELTA_REPO
systemctl --user daemon-reload
systemctl --user enable --now delta-e2e-real-gate.timer
```

The timer ticks hourly (`Persistent=true`, so a machine that was off catches
up on boot); almost every tick is an immediate skip — the gate, not the
timer, decides when quota is spent. Inspect it with:

```bash
systemctl --user list-timers delta-e2e-real-gate.timer   # next/last tick
journalctl --user -u delta-e2e-real-gate.service -n 50   # gate decisions + failures
head ~/.local/state/delta/e2e-real/*/last-attempt        # last attempt per provider (headed by path)
```

Uninstall:

```bash
systemctl --user disable --now delta-e2e-real-gate.timer
rm ~/.config/systemd/user/delta-e2e-real-gate.{service,timer}
systemctl --user daemon-reload
```

**Cron alternative** for non-systemd hosts (the login shell `bash -lc` gives
the run the same PATH as an interactive terminal):

```cron
0 * * * * bash -lc 'd="$HOME/.local/state/delta/e2e-real"; mkdir -p "$d"; make -C "$HOME/repos/delta" e2e-real-gate >>"$d/cron.log" 2>&1'
```

The redirect is *inside* `bash -lc`, after a `mkdir -p`: the gate creates that
state directory itself, but only once it runs, so a cron-level `>>` into it
would fail before the gate ever got a chance on a host that has never run it.

**Testing the gate without spending quota.** The test script exercises the
gate's decision paths with stub CLIs and a stub suite:

```bash
bash scripts/tests/e2e-real-gate.test.sh
```

To drive the gate by hand the same way: point `DELTA_CLAUDE_BIN` /
`DELTA_CODEX_BIN` at stubs that print a fake version, set `XDG_STATE_HOME` to
a temp dir, and set `E2E_REAL_CMD` (testing-only override, run via `bash -c`,
with `E2E_REAL_GATE_PROVIDER` naming the provider it was invoked for) to a
stub command. `E2E_REAL_GATE_PROVIDERS` (space-separated) narrows the tick to
one provider.
