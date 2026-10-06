---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! git grep -n 'set_var' -- backend/crates/apps/delta-desktop/src && git grep -q 'child_env' -- backend/crates/libs/delta-bootstrap/src/config.rs && ! git grep -n 'env::var_os(\"PATH\")' -- backend/crates/gateway/binary-detector/src/lib.rs"
assignee: null
branch: task/1006-1620-refactor-desktop-hand-the-login-shells-path-and-locale-to-the-server-as-values
created_at: 2026-10-06T08:22:43Z
updated_at: 2026-10-06T09:27:28Z
---

# refactor(desktop): hand the login shell's PATH and locale to the server as values instead of writing the process environment

## Overview

An app launched from Finder or a desktop file inherits the session manager's
minimal environment, so `delta-desktop` asks the user's login shell for its
`PATH`, `LANG` and `LC_*` at startup
(`backend/crates/apps/delta-desktop/src/login_env/`) and adopts them. Today
"adopts" means `std::env::set_var` on the whole process (`login_env/mod.rs:77,91`).
That has two costs:

- `setenv` after another thread exists is undefined behaviour (the reason
  Rust 2024 made `set_var` unsafe), so the import must finish before any
  thread starts — before the tokio runtime, before the window. The window
  therefore waits up to eight seconds for a slow shell with nothing on
  screen. The follow-up task opens the window first; it cannot while the
  values travel through the process environment.
- Every child process Delta spawns (`tmux`, `claude`, `codex`, `git`, `gh`,
  the opener) silently depends on global state set once in `main`, which no
  test can see and no reader can find from the spawn site.

This task changes the way the values travel, and nothing about which values
or what the children see: **the login shell's `PATH` and locale become data
the shell hands to the server's configuration, and the configuration hands
to every spawn.** The desktop binary stops writing the process environment.

### Change

**Desktop (`login_env`)**

- `import_login_shell_env` becomes a pure read: it returns a `LoginEnv`
  (the `PATH`, `LANG` and `LC_*` pairs the shell printed, with the UTF-8
  `LANG` fallback applied when none of `LC_ALL`, `LC_CTYPE`, `LANG` is set)
  plus the existing `Option<PathNotImported>`. It calls no `set_var` and
  reads the process environment only to fill the fallback and the
  `inherited_path` of the report. Rename it to say what it now does
  (`read_login_env` or similar) and rewrite the module doc, which today
  explains the ordering constraint that no longer exists.
- `main` puts the pairs into the server configuration (below) before
  `start_server`. The desktop crate ends with no `set_var` (gate).

**Server configuration (`delta-bootstrap`)**

- `Config` gains `child_env: Vec<(String, String)>`: the environment
  variables set on every child process Delta spawns, on top of the inherited
  environment. Default empty, so bare `delta-server` and `make dev` behave
  exactly as today (their children inherit the terminal's environment). Only
  the desktop shell fills it, and only with `PATH` and the locale variables.
  Document on the field why the set is deliberately this small: rc-file
  variables such as API keys or proxies are not imported implicitly; a user
  who wants an agent to see a variable sets it explicitly (a launch option,
  in a later feature), never by accident.
- The composition root (`build.rs`) passes `child_env` to every gateway that
  spawns a process, so no spawn site reaches for global state:
  - **binary-detector**: `PathBinaryDetector::new(path: Option<OsString>)`
    resolves bare names against the given `PATH` (falling back to the
    process's when `None`); `ensure_tmux_available` uses the same detector,
    so the startup tmux check sees the login shell's `PATH`. Replace the
    test that mutates the process `PATH` (`lib.rs:152-158`) with one that
    passes it in.
  - **tmux-driver**: `Tmux::new(.., env)` applies `.envs()` to every
    `Command` it runs. The `new-session` that boots the server fixes the
    server's global environment, which is what `claude` in the pane
    inherits — so the pane's `PATH` and locale come from here.
  - **`delta-server/src/pty.rs`**: the attach `CommandBuilder` gets the same
    pairs (it already pins `TERM`); the state carries them next to the tmux
    socket name.
  - **`claude_version.rs`**: `log_claude_version(bin, env)`.
  - **codex-agent**: fill the existing `CodexLaunchConfig.env` from
    `child_env` in `codex_launch_from_env` (rename to take the config).
  - **gh-cli**, **git-worktree**, **external-opener**: constructors take the
    pairs and apply them to their `Command`s. `git` and `gh` are found on
    `PATH` too, and the opener's `xdg-open` runs under the locale.
- Where a gateway has several `Command` construction sites (git-worktree has
  about eight), route them through one helper that applies the environment,
  so a future site cannot forget it.

**Behaviour that must not change**

- The children see the same variables as today: the shell's `PATH` and
  locale when launched from Finder or a desktop file, the terminal's
  environment when launched from a terminal (the login shell's values win
  over inherited ones in both cases, exactly as `set_var` did).
- The `PathNotImported` explanation in the startup dialog, and the timeout.

### Docs

- `docs/guides/development/local-run.md` "PATH and locale" bullet and the
  desktop section of `docs/guides/development/README.md`: the app passes the
  values to its server, which sets them on every command it starts; the
  browser version and `make dev` inherit the terminal's environment.
- `docs/guides/install/README.md` "What the app needs on the host": keep
  the paragraph true (wording may stay).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The desktop crate contains no `set_var`, `Config` has `child_env`, and
      the binary detector no longer reads the process `PATH` (gates in
      `check_command`).
- [x] Unit tests: the binary detector resolves a bare name against a `PATH`
      passed in and not against the process's; the tmux driver's and the PTY
      bridge's commands carry the given pairs (assert on the constructed
      `Command`/`CommandBuilder`, or on a fake binary that prints its
      environment); `read_login_env` returns the shell's pairs and leaves
      the process environment untouched (assert `env::var_os` before and
      after); the locale fallback appears in the returned pairs, not in the
      process.
- [x] `make check` passes and `make desktop-dev-build` builds.

### Before merge (verified outside the check command)

- [ ] On macOS, launch the dev build from Finder (`open` on the built
      `.app`, or Spotlight), start a Claude Code session and run `echo $PATH;
      locale` in its terminal: the `PATH` is the login shell's (Homebrew or
      `~/.local/bin` present) and the locale is UTF-8. Launch it from a
      terminal with a modified `PATH` and confirm the login shell's `PATH`
      still wins, as before.
- [ ] On Ubuntu (GNOME), the same from the application grid.
