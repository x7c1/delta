---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0930-0330-feat-add-a-tauri-desktop-shell-that-runs-delta-server-in-process
created_at: 2026-09-29T17:20:00Z
updated_at: 2026-09-29T18:00:15Z
---

# feat(app): add a Tauri desktop shell that runs delta-server in-process

## Overview

Delta is still "clone the repo and run `make dev`". The server can now serve
the built frontend itself (`delta-server` with the `embed-web` feature mounts
the SPA from `backend/crates/apps/delta-server/src/app/static_web/`), so the
missing piece for a downloadable app is a thin desktop shell: a Tauri v2
application that starts `delta-server` inside its own process and points a
webview at `http://127.0.0.1:<port>/`. This task adds that shell. It does not
bundle or publish anything yet (the release workflow and the unsigned-app
instructions are later steps), but `make app` must produce a runnable bundle
locally on macOS and Linux.

The shell stays a launcher. Everything the browser does today — REST, the
`/ws` event socket, the `/pty` terminal bridge, the `/comms` stream, the
Claude Code hook callbacks to `/hooks/*` — keeps going over plain HTTP to the
loopback server, exactly as through Vite's proxy. The frontend is not served
over `tauri://`, no Tauri IPC is used from the page, and `delta-server` is
not run as a sidecar process: an in-process server means nothing to
supervise or clean up.

### Change

1. **Reuse the server's startup in a library.** `backend/crates/apps/
   delta-server/src/main.rs` builds `Config` from the environment
   (`config_from_env` and its helpers: auth token and hook secret minting,
   transcript root, worktree base, launch overrides, port) and then binds
   and serves. Move that into the `delta_server` library (a `config`
   module, plus a serve helper that takes an already-bound
   `tokio::net::TcpListener`) so the shell and the CLI binary share one
   implementation; the binary shrinks to calling them. Keep the CLI's
   behaviour byte-for-byte: the same env variables, the same defaults
   (`delta.db` and `.tmp/session` relative to the cwd, `$HOME/.delta/
   worktrees`, `${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects`, port 7878),
   and the same user-facing stderr lines and exit code 1 for a refused
   database overlay or a missing host command.
2. **The shell crate.** Add `backend/crates/apps/delta-app` (Tauri v2,
   `tauri-build` in `build.rs`, `tauri.conf.json` with
   `identifier: "io.github.x7c1.delta"` and `productName: "Delta"`, the
   version taken from the workspace package version rather than repeated
   in the config, Tauri's stock icons under `icons/`, and the generated
   `gen/` directory ignored). It depends on `delta-server` with
   `features = ["embed-web"]`. Exclude the crate from the workspace's
   `default-members` so `cargo build` / `cargo test` / `cargo clippy` in
   `backend/` (and the `build` / `test` / `lint` make targets and the CI
   backend job) are unchanged; the app is built only when named with
   `-p delta-app`. Tauri's `tauri-plugin-dialog` is the one plugin allowed,
   for the startup-error dialog below.
3. **Startup.** On launch the shell:
   - imports the login shell's `PATH` — a GUI app launched from Finder or a
     desktop file does not get the user's shell `PATH`, so `tmux`,
     `claude` and `codex` are not found. Run `$SHELL` (fallback `/bin/sh`)
     as a login shell with a command that prints `PATH`, with a short
     timeout; on success set the process `PATH` to the result, on failure
     log a warning and continue with the inherited one. Keep the parsing
     in a pure function with unit tests;
   - builds `Config` through the shared function, then overrides the two
     cwd-relative defaults with the Tauri app data directory
     (`app_data_dir()`, i.e. `~/Library/Application Support/io.github.
     x7c1.delta/` on macOS and `~/.local/share/io.github.x7c1.delta/` on
     Linux): `delta.db` and `sessions/` under it, created on first run.
     An explicit `DELTA_DB_PATH` / `DELTA_SESSION_WORKDIR` still wins. The
     worktree base and the transcript root keep their `$HOME`-based
     defaults — the transcript root is where Claude Code writes, not
     where Delta writes;
   - binds `127.0.0.1:0` to take a free port (an explicit `DELTA_PORT`
     still wins), writes the bound port back into `Config.port` *before*
     `AppState::build`, because the hook URLs rendered into each
     session's settings carry that port;
   - builds `AppState` on a tokio runtime the shell owns, spawns the
     transcript tail and the async-event drain like the CLI does, serves
     the router on the bound listener, and opens one window titled
     `Delta` at `http://127.0.0.1:<port>/`. If `AppState::build` fails
     with one of the two user-facing errors the CLI special-cases, show
     the same text in a native message dialog and exit 1; any other error
     is logged and also exits non-zero.
4. **Exit.** Closing the window ends the process, and the server with it.
   The tmux server on Delta's dedicated socket is left running so open
   sessions survive an app restart and can be resumed — the app does not
   do what `make down` does.
5. **Make and CI.** `make app-dev` runs the shell from source
   (`cargo run -p delta-app`, after `web-dist`), and `make app` produces
   the bundle (`cargo tauri build` inside the crate; document the one-time
   `cargo install tauri-cli --version '^2' --locked`, the same way the
   Playwright browser install is documented). Add `check-app-build` to
   the `check` graph after `check-frontend-build` and `check-backend-build`
   (`cargo build -p delta-app`; debug is fine) so the shell compiles on
   every gate, and add the same build to the CI job that already builds
   the embedded server, with the Linux system packages Tauri v2 needs
   (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`,
   `librsvg2-dev`, `patchelf`, …) installed in that job.
6. **Docs.** `docs/guides/development/README.md`: the Linux prerequisites
   for building the shell in the prerequisites table, and a section on the
   `app-dev` / `app` targets and the one-time tauri-cli install.
   `docs/guides/development/local-run.md`: where the app keeps its data
   (the app data directory) and that `make dev` is unaffected. Do not
   rewrite `README.md`'s Getting started yet.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `delta-server`'s `main.rs` builds its configuration and serves
      through library functions that `delta-app` also calls; the CLI's env
      variables, defaults and startup error messages are unchanged
      (existing tests still pass, plus unit tests for the shared config
      function's defaults and overrides).
- [x] The login-shell `PATH` import and the app-data-dir override are pure,
      unit-tested functions.
- [x] `cargo build` / `cargo test` / `cargo clippy` in `backend/` do not
      build `delta-app`; `make check` is green and includes
      `check-app-build`; CI builds the shell.

### Manual / on-hardware (verified by a human before merge)

- [ ] macOS: `make app` produces `Delta.app`; launched from Finder (not a
      terminal) it finds `tmux` and `claude`, the session list loads, a new
      session spawns and its terminal attaches, and `delta.db` and
      `sessions/` appear under `~/Library/Application Support/io.github.
      x7c1.delta/`.
- [ ] Linux: `make app` produces a bundle that launches with the same
      result under `~/.local/share/io.github.x7c1.delta/`.
- [ ] Closing the window stops the server (the port is released) and the
      tmux sessions survive; relaunching the app resumes them.

## Out of scope

- Building and attaching bundles in the release workflow, code signing,
  the unsigned-app instructions and the README rewrite: later steps.
- A custom icon, auto-update, a tray icon, multiple windows.
- Any change to `make dev`, the Vite proxy, or the `embed-web` serving
  code.
