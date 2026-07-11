# Development

How to build, test, lint, and run Delta locally. Delta has two parts: a Rust
backend (`backend/`) and a TypeScript frontend (`frontend/`).

The unified entry point is `make`, run from the repo root: it wraps the
per-part commands and the `scripts/dev.sh` loop so you do not have to `cd` into
each part or remember the underlying `cargo`/`pnpm` invocations. Run `make help`
for the full target list. This guide names the relevant `make` target for each
task and keeps the underlying commands only where there is no target (one-time
setup and env-overridden runs).

## Supported platforms

Delta is officially supported on **Linux** and **macOS** for both development
and runtime. The dev scripts (`scripts/dev.sh`, `scripts/stop.sh`,
`scripts/reset.sh`) and the `Makefile` target the common shell baseline shared
by both — see "Portability conventions" below.

### Prerequisites by platform

| Platform | Prerequisites |
|----------|---------------|
| Linux | `tmux`, `lsof`, GNU `make`, `bash` — install via the system package manager (e.g. `apt install tmux lsof make`). |
| macOS | `tmux` via Homebrew (`brew install tmux`). `lsof`, `make` (GNU make 3.81), `awk`, `bash` 3.2, `date`, and `pkill` ship with the system. Installing the Xcode Command Line Tools (`xcode-select --install`) is the standard way to get `make`. |

In addition, both platforms need the Rust toolchain (`cargo`) and pnpm (via
`corepack enable`), and an authenticated Claude Code (`claude`) — see the
"Run the whole thing locally" section below.

### `lsof` is assumed

`scripts/dev.sh`'s `port_in_use` / `kill_port` helpers prefer `lsof` for port
probing and teardown. They fall back to `ss` or `fuser` (Linux-only) when
`lsof` is missing, gated by `command -v`, but a host without `lsof` is not a
supported configuration. macOS ships `lsof` by default, so this is only a
concern on stripped-down Linux images.

### Portability conventions

The dev scripts are written against a shell baseline that runs unmodified on
both platforms. Keep changes within this baseline so macOS support does not
regress:

- Stick to **bash 3.2 / POSIX-compatible** idioms — macOS still ships bash 3.2
  by default, and the scripts are run under that version.
- Avoid bash-4-only features: associative arrays (`declare -A`),
  `mapfile` / `readarray`, case-modifying expansions (`${var,,}` / `${var^^}`),
  and `&>>` redirection.
- Avoid GNU-only flags and tools: `readlink -f`, GNU-style `sed -i`,
  `date -d`, `grep -P`, GNU `stat`, `nproc`, and reads from `/proc`.
- When a feature genuinely needs a Linux-only tool (e.g. `ss`, `fuser`), gate
  it with `command -v` and provide an `lsof`-based path or a no-op fallback so
  the script still does the right thing on macOS.

## Backend (`backend/`)

Quality gate — `make build`, `make test`, and `make lint` each cover both
parts; `make check` runs the whole gate (build, test, lint, plus the frontend
typecheck) for both parts at once, which is what to run before opening a PR.

Run the server (from `backend/`):

```bash
cargo run -p delta-server
```

It listens on `127.0.0.1` only (loopback). Configuration comes from environment
variables, all with local-friendly defaults:

| Variable | Default | Purpose |
|----------|---------|---------|
| `DELTA_PORT` | `7878` | TCP port |
| `DELTA_DB_PATH` | `delta.db` | SQLite overlay file |
| `DELTA_SESSION_WORKDIR` | `.tmp/session` | base directory for per-spawn working directories (`<base>/<token>`) |
| `DELTA_WORKTREE_BASE` | `$HOME/.delta/worktrees` | base directory for per-session git worktrees (`<base>/delta-<session-id>`), deliberately outside any repo tree so the worktree does not inherit a surrounding `CLAUDE.md`/settings |
| `DELTA_TMUX_SOCKET` | `delta` | dedicated tmux socket (`tmux -L <socket>`) for Delta's sessions, isolated from your default tmux server |

The server owns the `claude` session lifecycle: it boots fine with no tmux
session present and spawns nothing on startup or page load. A session is spawned
lazily when first needed — the composer's first Send, a New action, or
`POST /api/sessions`. Each spawn gets its own tmux session, named after a
Delta-minted token (`delta-<n>`), running `claude` in its own working directory
(`<base>/<token>`) with Claude Code hooks pointed back at this server. Naming the
tmux session after a Delta-owned token (never Claude's `session_id`) is what lets
a closed conversation be resumed (`claude --resume <id>`) under a fresh tmux
session without a name collision. Open/closed is in-memory only: after a restart
every persisted conversation is "closed" until it is resumed. Authentication is
assumed — the server relies on a cached Claude Code token (or
`CLAUDE_CODE_OAUTH_TOKEN`) and never runs interactive OAuth.

## Frontend (`frontend/`)

The `frontend/` directory is the pnpm workspace root. pnpm is provided by
corepack from the `packageManager` field — run `corepack enable` once if pnpm is
not on your PATH. Install dependencies once with `pnpm install` from `frontend/`.

The quality gate (build, typecheck, test, and `lint` = ESLint +
dependency-cruiser) is covered by `make check`, or the individual `make build` /
`make test` / `make lint` targets.

### Generated wire bindings (`@delta/wire-gen`)

`frontend/packages/gateway/wire-gen` contains TypeScript generated from the
backend's wire contract (the `delta-wire` crate): the REST request/response
shapes, the `SessionEvent` union, and the `EVENT_KINDS` const. Never edit the
files under `src/generated/` by hand —
change the Rust types and run `make gen` to regenerate, then commit the result.
`make check` (and CI) regenerates and fails on any diff, so stale bindings
cannot land.

### Run the UI against mocks (no backend needed)

MSW mocks the REST API and a fake event source replays the WebSocket stream, so
the full UI runs without the backend — no tmux or `claude` required:

```bash
make mock    # → http://localhost:5173
```

`make mock` builds the workspace libraries first (the dev server resolves them
from built output), then starts the mock-mode dev server with `--force` so the
freshly built libs are re-optimized and served.

### End-to-end UI tests (headless, mock mode)

A headless Playwright suite drives the real browser DOM against mock mode and
asserts functional, structural behavior (message send, branch drill-in, the
pending/running indicator lifecycle, layout restore after reload, terminal
resize). It lives in `@delta/web` (`packages/apps/web/e2e/`), separate from the
vitest unit tests.

Install the browser once:

```bash
pnpm --filter @delta/web exec playwright install --with-deps chromium
```

Build the workspace libraries (`make build`, since the dev server resolves them
from built output), then run the suite with `make e2e` — Playwright starts the
mock-mode dev server itself.

The suite puts the fake event source under manual control (no auto-replay) and
feeds events explicitly, so every run is fast and deterministic.

`make e2e` runs isolated: it pins a dedicated mock-server port (`E2E_PORT=5199`)
so it cannot collide with a dev server on the default 5173, and the suite starts
its own mock-mode build every run (it never reuses an already-running server).
That last part matters — a live `make dev` server (real backend, tmux +
`claude`) is indistinguishable from the suite's own mock build at the port, so
adopting it would drive that **live session, sending real prompts to your real
`claude`**. Hence the suite never reuses by default.

For fast iteration you can reuse an already-running mock server (e.g. `make
mock`) instead of spawning a fresh one per run: set `E2E_REUSE=1` and point the
suite at that server's port — only ever a mock server, never `make dev`:

```bash
E2E_REUSE=1 E2E_PORT=5173 make e2e
```

### End-to-end UI tests (headless, fake mode)

A second Playwright suite (`packages/apps/web/e2e-fake/`) drives the real
backend instead of mocks: a live `delta-server` on a temp database, real tmux
panes, real hooks, the real transcript tail, and the real WebSocket/PTY
channels. The only scripted part is the "claude" the server spawns: the
`fake-claude` binary (`backend/crates/apps/fake-claude`), a stand-in that
accepts `claude`'s CLI flags, fires the same HTTP hooks, and writes the same
transcript JSONL — but follows a deterministic scenario script instead of a
model. This is the suite that proves the full loop
(REST → spawn → tmux → hooks → transcript → tail → WS) end to end.

Run it with:

```bash
make e2e-fake
```

Ownership is split. `scripts/e2e-fake.sh` is a thin wrapper: it only builds
the two binaries (`delta-server`, `fake-claude`) and invokes the Playwright
suite. The **server lifecycle is owned by a worker-scoped Playwright fixture**
(`packages/apps/web/e2e-fake/support/server.ts`), which runs in the worker
process and holds the child-process handle — which is what makes the
server-restart coverage possible (kill the server, relaunch it against the
same database and tmux socket) and means a worker crash reboots the server.
The fixture owns the per-run temp database and tmux socket
(`delta-e2e-fake-<pid>`, killed on teardown), the scripted-claude wrapper, a
shortened launch watchdog (`DELTA_LAUNCH_DEADLINE_MS`), the dedicated backend
port (7899), and the `/health` readiness poll; Playwright starts the Vite dev
server (port 5198) proxied to that backend. Because a hard kill (SIGKILL,
Ctrl-C) can skip teardown, the fixture also **sweeps at startup**: it kills any
leftover `delta-e2e-fake-*` tmux server and removes any `delta-e2e-fake.*` temp
dir from a crashed run, so leaks are bounded to one run. Each server generation
logs to its own file under `test-results/e2e-fake/` (`server.log`,
`server.2.log`, …), all uploaded by CI on failure. Nothing the e2e-fake run
touches collides with `make dev` or the mock suite. It needs tmux, the
Playwright chromium browser (see above), and built workspace libraries
(`make build`).

**Writing a scenario.** Scenarios are JSON files in
`packages/apps/web/e2e-fake/scenarios/`, executed step by step by the fake:
`await_prompt`, `reply`, `tool_use`, `permission_request`, `tool_result`,
`stop`, `await_interrupt`, `write_queued_command`, `delay`, `hang`, plus
`session_start` timing (`immediate` / `skip` / `{ "delay_ms": N }`) and an
optional `loop`. The full vocabulary is documented in the fake's `scenario`
module (`backend/crates/apps/fake-claude/src/scenario.rs`). A spec selects its
scenario through the **first word of the first prompt it sends**: sending
`"first-send hold then answer"` makes the spawned fake load
`scenarios/first-send.json`. Keep specs structural — assert presence, absence,
and ordering of UI elements, never scripted reply text.

The same fake also backs a backend-only integration test
(`backend/crates/apps/fake-claude/tests/full_loop.rs`, part of `cargo test`)
that proves the loop without a browser; it skips where tmux is missing.

### End-to-end canaries (real claude)

The fake-claude lane above is a *recording* of claude's implicit contract —
the hook events and payload fields, the JSONL transcript shapes, the interrupt
marker, queued prompts, `isMeta` flagging, the permission-decision envelope.
The real-claude canary suite checks that recording against reality:

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

### Automatic canary trigger (opt-in)

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

### Run the UI against the real backend

Start the server (see Backend), then:

```bash
pnpm --filter @delta/web dev
```

Vite proxies `/api`, `/ws`, and `/pty` to `127.0.0.1:7878` by default; if the
server runs elsewhere, start the dev server with the same `DELTA_PORT` and the
proxy follows it.

### Notes

- The web dev server resolves workspace libraries from their built output. After
  editing a library package (`@delta/model`, `@delta/ui-kit`, `@delta/api-client`)
  rebuild it, or run a watch in another terminal:
  `pnpm -r --parallel exec tsc -b --watch`. Editing `@delta/web` sources
  hot-reloads directly.
- `esbuild` and `msw` build scripts are allow-listed in `pnpm-workspace.yaml`
  (`allowBuilds`); pnpm does not run dependency build scripts by default.

## Run the whole thing locally

This brings up the full loop end to end: type in the browser, a real `claude`
TUI (running in tmux) receives it via `send-keys`, and its response flows back
through the JSONL transcript and Claude Code's HTTP hooks to the browser.

Opening the browser is the only manual step. `make dev` starts both the
server and the frontend dev server. The server owns the `claude` session
lifecycle but does not spawn anything on startup or on page load: on load the UI
shows the session list (empty on a fresh database), and the first Send from the
composer (or a New action) spawns a session, so there is nothing else to launch.

### Prerequisites

- `tmux` on your PATH (it hosts the `claude` session the server creates).
- An authenticated Claude Code (`claude`). Authentication is assumed — the
  server relies on a cached token (or `CLAUDE_CODE_OAUTH_TOKEN`) and does not run
  interactive OAuth. If you have not logged in yet, run `claude` once on its own.
- The Rust toolchain (`cargo`) and pnpm (via `corepack enable`).

### Launch

```bash
make dev                 # default session workdir: .tmp/session
make dev WORKDIR=~/scratch # or pass your own working directory for claude
```

`make dev` runs `scripts/dev.sh`, which:

1. Starts `delta-server` (`DELTA_PORT=7878`), passing the session working
   directory. The server owns the `claude` session lifecycle: when a session is
   spawned (first Send / New) it creates the tmux session and launches `claude
   --settings <file>` with Delta's rendered session settings (so the hooks point
   at `http://127.0.0.1:7878/hooks/...`); the settings file lives outside the
   working directory, so a real project's own `.claude/settings.json` is never
   touched. Nothing is spawned on startup.
2. Installs and builds the frontend workspace libraries, then starts the web dev
   server against the real backend (port 5173).

Both run as managed background processes, logging to `.tmp/` (a stable
`delta-server.log` / `delta-frontend.log` symlink points at the latest run).
`make dev` does not return until both ports are actually listening, so a
completed command means the UI is openable right away — the frontend's
install+build finishes binding port 5173 before control returns. If either
process dies or is not listening within its budget, `make dev` tears the loop
back down and exits non-zero after printing the tail of the relevant log (the
budgets are overridable via `DELTA_DEV_SERVER_TIMEOUT` / `DELTA_DEV_FRONTEND_TIMEOUT`).

Then open <http://localhost:5173>. On load the UI fetches the session list and
shows it (empty on a fresh database); opening the browser does not spawn
anything. From the composer, the first Send (or a New action) spawns a fresh
`claude` session. Existing sessions show as open or closed: a closed session is
view-only — you can read its history with no process running — and the first
Send to it resumes it (`claude --resume`). After a server restart every prior
session shows as closed until it is resumed via Send.

### First run / answering prompts

If `claude` is not yet authenticated, the first spawn will not become usable and
the UI shows an explicit error. Run `claude` once on its own to finish login, or
attach to a spawned pane to log in and to answer permission prompts as they
appear (each spawn is named `delta-<n>`; the first Send of a run spawns
`delta-1`):

```bash
tmux -L delta attach -t delta-1     # detach again with Ctrl-b then d
```

### Happy-path check

Type a message in the browser. It is dispatched into the tmux pane via
`send-keys`; `claude`'s reply is ingested from the transcript and surfaces in
the browser. When a tool needs permission, answer it in the embedded terminal or
in the TUI (`tmux -L delta attach -t delta-1`).

### Shut down

```bash
make down
```

This stops `delta-server`, the frontend dev server (port 5173), and every
`delta-<n>` tmux session the server spawned. To also delete the SQLite overlay
so the next start recreates an empty schema, run `make reset` instead.

## Release automation

For the developer-facing release flow (cutting a release, promoting to
minor or major, allowed title transitions), see [release.md](release.md).
This section covers the supporting setup: how the release PR is opened
under a user PAT and why that is required.

The `Create Release PR` workflow (`.github/workflows/create-release-pr.yml`)
opens and updates the rolling release PR. It runs on two triggers: every push
to `main`, and `pull_request: types: [edited]` so that promoting a release
PR's title (e.g. `Release v0.1.1` → `Release v0.2.0`) is picked up immediately
rather than waiting for the next main push. The `pull_request` branch is
gated on an **open** PR carrying the `release` label whose **title actually
changed** (`changes.title.from != null`), so body-only edits, no-op title
saves, and bot self-edits do not retrigger the workflow. It pushes a
`release/since-<UTC date+time>` branch (chosen once when the PR is opened and
reused for the lifetime of that PR) and calls `gh pr create` under a
**user-scoped personal access token**, exposed to the workflow as the
repository secret **`RELEASE_PAT`**.

A user-scoped token is required because GitHub's recursion-prevention rule
suppresses `pull_request` workflow runs on PRs authored by `github-actions[bot]`.
With the default `GITHUB_TOKEN` the release PR would be bot-authored, so `CI`
and `Validate Release PR` would sit in `action_required` and never go green.
Pushing and opening the PR under a user PAT makes the PR user-authored, which
lets the existing checks trigger normally.

**Required setup (one-time, per repo).** A maintainer must register
`RELEASE_PAT` in the repository's Actions secrets with these scopes:

- `contents: write` — push the `release/since-<UTC date+time>` branch.
- `pull-requests: write` — create and edit the release PR.

Without the secret, the workflow fails loudly at the `git push` step. There is
no fallback to `GITHUB_TOKEN` by design: a silent fallback would mask exactly
the misconfiguration the PAT is solving.

The `Release` workflow (`.github/workflows/release.yml`) that tags and publishes
after the release PR is merged keeps using the default `GITHUB_TOKEN` — it runs
under a human-triggered merge event, so recursion is not a concern, and no
workflow in this repo listens to tag pushes.
