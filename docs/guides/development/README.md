# Development

## Overview

How to build, test, lint, and run Delta locally. Delta has two parts: a Rust
backend (`backend/`) and a TypeScript frontend (`frontend/`).

The unified entry point is `make`, run from the repo root: it wraps the
per-part commands and the `scripts/dev.sh` loop so you do not have to `cd` into
each part or remember the underlying `cargo`/`pnpm` invocations. Run `make help`
for the full target list. These guides name the relevant `make` target for each
task and keep the underlying commands only where there is no target (one-time
setup and env-overridden runs).

This file covers the platform baseline and the day-to-day commands for each
part. The larger workflows live in their own files:

- **[e2e.md](e2e.md)** — the headless Playwright suites: mock mode (`make e2e`)
  and fake mode against the real backend (`make e2e-fake`).
- **[canary.md](canary.md)** — the real-agent canary suites (`make e2e-real-claude`,
  `make e2e-real-codex`), the drift runbook, and the automatic canary trigger.
- **[local-run.md](local-run.md)** — running the whole thing locally with
  `make dev`.
- **[release.md](../release.md)** — the release flow and its supporting
  automation.
- **[install/](../install/README.md)** — installing the released desktop app as an
  end user (not needed for development).

## Supported platforms

Delta is officially supported on **Linux** and **macOS** for both development
and runtime. The dev scripts (`scripts/dev.sh`, `scripts/stop.sh`,
`scripts/reset.sh`) and the `Makefile` target the common shell baseline shared
by both — see "Portability conventions" below.

### Prerequisites by platform

| Platform | Prerequisites |
|----------|---------------|
| Linux | `tmux`, `lsof`, `jq`, GNU `make`, `bash` — install via the system package manager (e.g. `apt install tmux lsof jq make`). |
| macOS | `tmux` and `jq` via Homebrew (`brew install tmux jq`). `lsof`, `make` (GNU make 3.81), `awk`, `bash` 3.2, `date`, and `pkill` ship with the system. Installing the Xcode Command Line Tools (`xcode-select --install`) is the standard way to get `make`. |
| Linux, desktop shell only | The system libraries Tauri v2 links against, needed only to build `delta-desktop` (`make desktop-dev`, `make desktop`, and `make check`'s `check-desktop-build`): on Debian/Ubuntu `apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev patchelf build-essential file`. macOS needs nothing beyond the Xcode Command Line Tools. |

In addition, both platforms need the Rust toolchain (`cargo`) and pnpm (via
`corepack enable`), plus the agent CLIs you plan to drive: an authenticated
Claude Code (`claude`) and/or Codex (`codex`) — see
[local-run.md](local-run.md).

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
parts and stay fast, for the inner loop. `make check` is the pre-PR gate: it
runs the whole thing for both parts at once (build, test, lint, the frontend
typecheck, the generated-bindings freshness check, the vendored Codex schema's
stored form, the canary gate's, the re-vendor script's and the freshness
check's own stubbed tests) **plus both Playwright suites**, so passing it means
CI will pass. It needs tmux, because `make e2e-fake` drives the real backend
through one, and `jq`, which is what the vendored schema's stored form is
defined in terms of.

`make check` is a dependency graph, not a script: each step is a target that
depends only on what it needs (the backend tests, clippy and the freshness
check on the backend build; the frontend typecheck, tests, lint and mock e2e
on the frontend build; the fake-mode e2e on both builds; the script checks on
nothing). Run it with `-j` to let independent steps overlap — the wall clock
then approaches CI's longest job instead of the sum of all steps:

```bash
make -j4 check      # parallel; add -O to keep each step's output together
make check          # serial, in the same dependency order
```

The graph is spelled out above the `check` target in the `Makefile`. The two
Playwright suites run side by side safely: each has its own ports and its own
output directory under `packages/apps/web/test-results/`.

Run the server (from `backend/`):

```bash
cargo run -p delta-server
```

It listens on `127.0.0.1` only (loopback). Configuration comes from environment
variables, all with local-friendly defaults:

| Variable | Default | Purpose |
|----------|---------|---------|
| `DELTA_PORT` | `7878` | TCP port |
| `DELTA_DB_PATH` | `delta.db` | SQLite overlay file; its directory also holds the hook state file `delta-hook-state.json` (the hook secret, kept across restarts — see [local run](local-run.md#the-desktop-app)) |
| `DELTA_HOOK_SECRET` | recorded in the hook state file | the secret every hook URL carries; overrides the recorded one without replacing it |
| `DELTA_SESSION_WORKDIR` | `.tmp/session` | base directory for per-spawn working directories (`<base>/<token>`) |
| `DELTA_WORKTREE_BASE` | `$HOME/.delta/worktrees` | base directory for per-session git worktrees (`<base>/delta-<session-id>`), deliberately outside any repo tree so the worktree does not inherit a surrounding `CLAUDE.md`/settings |
| `DELTA_TMUX_SOCKET` | `io.github.x7c1.delta` | dedicated tmux socket (`tmux -L <socket>`) for Delta's sessions, isolated from your default tmux server (`make dev` sets `io.github.x7c1.delta.dev`) |

The server owns the `claude` session lifecycle: it boots fine with no tmux
session present and spawns nothing on startup or page load. A session is spawned
lazily when first needed — the composer's first Send, a New action, or
`POST /api/sessions`. Each spawn gets its own tmux session, named after a
Delta-minted token (`delta-<n>`), running `claude` in its own working directory
(`<base>/<token>`) with Claude Code hooks pointed back at this server. Naming the
tmux session after a Delta-owned token (never Claude's `session_id`) is what lets
a closed conversation be resumed (`claude --resume <id>`) under a fresh tmux
session without a name collision. Open/closed is in-memory, except that a
session's row remembers the tmux pane it is bound to: after a restart the server
re-adopts each remembered pane still running before it serves anything, and every
other persisted conversation is "closed" until it is resumed. Authentication is
assumed — the server relies on a cached Claude Code token (or
`CLAUDE_CODE_OAUTH_TOKEN`) and never runs interactive OAuth.

### Desktop shell (`delta-desktop`)

`backend/crates/apps/delta-desktop` is a Tauri v2 shell that runs `delta-server`
inside its own process on a free loopback port and opens one window on it,
serving the built SPA (`embed-web`). It is a launcher only: the page talks to
the server over plain HTTP exactly as a browser does, with no Tauri IPC. It is
single-instance (`tauri-plugin-single-instance`, scoped by the app's
identifier): a second launch focuses the running window and exits, so it cannot
start a rival server on the same database and tmux socket, nor take over the
hook port the running copy holds.

The crate is left out of the workspace's `default-members`, so `cargo build`,
`cargo test` and `cargo clippy` in `backend/` (and `make build` / `make test` /
`make lint`) never build it or need the Linux libraries above. Name it to build
it (`cargo build -p delta-desktop`); `make check` does so in `check-desktop-build`,
and CI does in the job that builds the embedded server.

```bash
make desktop-dev        # build the dev environment's shell, then run it (refuses while make dev runs)
make desktop-dev-build  # build the SPA, then the binary make desktop-dev runs, without starting it
make desktop            # build the installed app, then install it
make desktop-build      # build the SPA, then bundle the installed app with cargo tauri build, without installing it
```

Each pair follows one rule: the `-build` target only builds, and the plain one
also takes the result to where you use it. `make desktop-dev` runs the debug
binary from `backend/target/` — nothing is installed. `make desktop` installs
the bundle: on Linux it runs `sudo apt install --reinstall` on the new `.deb`
(reinstall, because a local build keeps the release's version number, which
`apt` would otherwise treat as already installed); on macOS it replaces
`/Applications/Delta.app`. A running Delta keeps the old build until you quit it
and start it again.

The two pairs build two different apps. `make desktop-build` bundles the installed
app, with `tauri.conf.json`'s identifier `io.github.x7c1.delta`. `make
desktop-dev` is the dev environment — the one `make dev` runs — seen through the
desktop shell instead of the browser: `make desktop-dev-build` builds a debug
`delta-desktop` whose identifier is `io.github.x7c1.delta.dev`, so its app data
directory (webview storage included) and its single-instance scope are its own,
and it neither focuses nor disturbs a running installed app. The override is
the `TAURI_CONFIG` environment variable (`{"identifier": …}`), which
tauri-build and `generate_context!` merge over the config files at compile
time; a plain `cargo build` honours it, so the dev build needs neither the Tauri
CLI nor a second config file. `scripts/dev.sh --desktop-build` sets it, and
`scripts/dev.sh --desktop` then runs the binary on `make dev`'s database, tmux
socket, port and session working directory (`WORKDIR` works as for `make dev`).
How the two environments relate is in
[local-run.md](local-run.md#two-environments-the-installed-app-and-the-dev-environment).

`check-desktop-build` builds the shell with `TAURI_CONFIG` unset, i.e. the
installed app's configuration, into the same `backend/target/debug/delta-desktop`.
tauri-build reruns when `TAURI_CONFIG` changes, so after `make check` the next
`make desktop-dev-build` recompiles the `delta-desktop` crate alone (a few
seconds), and the other way round; neither reuses a binary built with the other
identifier.

`make desktop-build` (and so `make desktop`) needs the Tauri CLI, installed once:

```bash
cargo install tauri-cli --version '^2' --locked
```

On Linux, `make desktop-build` links with whatever `cc` is first on `PATH`. If that is
another toolchain's compiler — for example a Nix `gcc` wrapper — the built
`delta-desktop` gets that toolchain's dynamic loader as its interpreter, which
does not read the system library cache, so the installed `.deb` fails to start with
`error while loading shared libraries: libpango-1.0.so.0` (while `ldd` still
looks fine). Point the build at the system compiler in that case:

```bash
CC=/usr/bin/gcc CXX=/usr/bin/g++ \
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=/usr/bin/gcc \
  make desktop-build
readelf -l backend/target/release/delta-desktop | grep interpreter   # /lib64/ld-linux-x86-64.so.2
```

To keep this for every `make desktop-build` (and so `make desktop`), put it in
the gitignored `local.mk` (see `local.mk.example`), scoped to the
`desktop-build` target so other builds keep their cache:

```make
desktop-build: export CC := /usr/bin/gcc
desktop-build: export CXX := /usr/bin/g++
desktop-build: export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER := /usr/bin/gcc
```

The bundles land under `backend/target/release/bundle/` (`macos/Delta.app` and
a `.dmg` on macOS; a `.deb` on Linux). On macOS the `.dmg`
step lays out the image's Finder window through AppleScript, so it needs your
terminal to be allowed to control Finder (System Settings → Privacy & Security →
Automation; macOS asks the first time). Without that permission the step fails
or hangs after `Delta.app` has already been written, which is enough to run the
app. Where the app keeps its data, and how it finds `tmux` and `claude` when
launched from Finder or a desktop file, is in
[local-run.md](local-run.md#the-desktop-app). Installing and opening a released
bundle is in [the install guide](../install/README.md).

The Debian package is named `delta-desktop`, like the command it installs, not
`delta`: Ubuntu's archive already has an unrelated `delta` package, and apt
would treat ours as a version of it. Tauri derives the package name (and the
`.deb` file name) from `productName` and has no deb-specific override, so
`tauri.linux.conf.json`, which Tauri merges over `tauri.conf.json` on Linux
only, sets `productName` to `delta-desktop`. The name users see stays `Delta`:
the desktop entry template (`linux/delta-desktop.desktop`, a copy of Tauri's
default) names the application menu entry, the window title is set in
`main.rs`, and macOS still builds `Delta.app`.

### Reading the SQLite schema

The schema has no single file to open: it is built by replaying the migration
ladder in `backend/crates/gateway/delta-sqlite/src/migrations/`, one module per
schema subject. To read the whole thing at once, dump it from a database the
ladder built:

```bash
sqlite3 delta.db .schema
```

That is true by construction — it is the schema delta is actually running
against, including every migration step this particular file has been through.
`sqlite3 delta.db 'PRAGMA user_version'` shows which generation it is stamped
at. For when a change needs a migration step and when it may ask for a reset,
see the [compatibility policy](../compatibility.md).

Pulling a change that adds a step migrates the file in place on the next server
start — there is no command to run. A *destructive* step (the first is v6)
additionally leaves `delta.db.bak-v<source version>` beside it, gitignored and
removed only by `make reset`. That snapshot is also the way back: the ladder
runs forward only, so an older checkout refuses to open a database a newer one
migrated, and the error it prints offers `make reset` — which deletes the
thread overlay and the send queue with it. To keep those, stop the server,
remove `delta.db` with its `-wal`/`-shm` sidecars, and put the `.bak-v<n>` copy
back in its place.

### Vendored `codex app-server` schema

`backend/crates/gateway/codex-agent/vendor/app-server-schema/` holds the JSON
Schema of the Codex app-server protocol, generated from a pinned Codex CLI and
stored key-sorted. Re-vendor it with `make vendor-codex-schema` (needs that
Codex CLI installed; `DELTA_CODEX_BIN` overrides the binary) rather than by
running the generator by hand: its output order is unstable, and the target
normalises the files so the diff shows the protocol changes instead of
reorderings. `make vendor-codex-schema-check` fails when a vendored file has
left that form — `make check` and CI's backend job both run it, and it needs
only `jq`. `make vendor-codex-schema-test` exercises the re-vendor path against
a stub generator, so it needs no Codex CLI at all.

Why the files are stored that way, which generator outputs are vendored, and
what to bump when the pinned version moves live in that directory's
[README](../../../backend/crates/gateway/codex-agent/vendor/app-server-schema/README.md).

## Frontend (`frontend/`)

The `frontend/` directory is the pnpm workspace root. pnpm is provided by
corepack from the `packageManager` field — run `corepack enable` once if pnpm is
not on your PATH. Install dependencies once with `pnpm install` from `frontend/`.

The quality gate (build, typecheck, test, and `lint` = ESLint +
dependency-cruiser) is covered by `make check` — which also runs both Playwright
suites — or by the individual `make build` / `make test` / `make lint` targets
when a faster loop is wanted.

### Generated wire bindings (`@delta/wire-gen`)

`frontend/packages/gateway/wire-gen` contains TypeScript generated from the
backend's wire contract (the `delta-wire` crate): the REST request/response
shapes, the `SessionEvent` union, and the `EVENT_KINDS` const. Never edit the
files under `src/generated/` by hand —
change the Rust types and run `make gen` to regenerate, then commit the result.
`make gen-check` (part of `make check`, and run by CI) generates the bindings
into a temporary directory and fails when they differ from the files on disk,
so stale bindings cannot land. It writes nothing and ignores git, so it passes
as soon as `make gen` has run — before the result is committed; CI's clean
checkout is what makes sure it is committed.

### Run the UI against mocks (no backend needed)

MSW mocks the REST API and a fake event source replays the WebSocket stream, so
the full UI runs without the backend — no tmux or `claude` required:

```bash
make mock    # → http://localhost:5173
```

`make mock` builds the workspace libraries first (the dev server resolves them
from built output), then starts the mock-mode dev server with `--force` so the
freshly built libs are re-optimized and served.

### Run the UI against the real backend

Start the server (see Backend), then:

```bash
pnpm --filter @delta/web dev
```

Vite proxies `/api`, `/ws`, `/pty`, and `/comms` to `127.0.0.1:7878` by default;
if the server runs elsewhere, start the dev server with the same `DELTA_PORT`
and the proxy follows it.

### Notes

- The web dev server resolves workspace libraries from their built output. After
  editing a library package (`@delta/model`, `@delta/ui-kit`, `@delta/api-client`)
  rebuild it, or run a watch in another terminal:
  `pnpm -r --parallel exec tsc -b --watch`. Editing `@delta/web` sources
  hot-reloads directly.
- `esbuild` and `msw` build scripts are allow-listed in `pnpm-workspace.yaml`
  (`allowBuilds`); pnpm does not run dependency build scripts by default.
