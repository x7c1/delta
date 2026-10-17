---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 0
check_command: "make check"
assignee: null
branch: task/1018-0436-keep-the-macos-single-instance-socket-per-user-retry-1
created_at: 2026-10-17T19:36:40Z
updated_at: 2026-10-17T21:20:00Z
---

# fix(desktop): keep the macOS single-instance socket per user and read the login shell's env NUL-separated

## Overview

Two low-severity weaknesses in the desktop shell
(`backend/crates/apps/delta-desktop/`), fixed together because both harden
how the shell starts up.

### 1. The macOS single-instance socket lives in the shared `/tmp`

On macOS, `tauri-plugin-single-instance` (2.5.2,
`src/platform_impl/macos.rs` in the crate source under `~/.cargo/registry`)
puts its socket at a fixed path,
`/tmp/io_github_x7c1_delta_si.sock` (`/tmp/<identifier with . and - as _>_si.sock`),
and the path cannot be configured. `/tmp` is shared by every user of the Mac,
so another local user can take that path first:

- listening on it makes every launch of Delta send its cwd and argv there and
  exit 0 silently: Delta can never start (denial of service);
- placing a regular file there makes the connect fail with an error other
  than `NotFound` / `ConnectionRefused`, so the plugin skips single-instance
  and launches normally: two copies can then run against the same database
  and tmux socket;
- the plugin's accept loop reads each connection to EOF inline, so one
  client that connects and never closes stops every later second launch from
  being forwarded.

The callback (`focus_running_window` in `src/main.rs`) ignores argv and cwd,
so there is no code execution or token theft; the fix is about availability.

Fix: on macOS, replace the plugin with Delta's own single-instance lock whose
socket lives in a directory only the user can write, and keep the plugin on
Linux, where it uses the session D-Bus and is not affected (move the
dependency to a Linux-only target table if nothing else needs it on macOS).
Requirements:

- The socket's directory is the per-user temporary directory macOS assigns
  (`confstr(_CS_DARWIN_USER_TEMP_DIR)`, the directory `$TMPDIR` normally
  names; mode 0700, owned by the user), not `$TMPDIR` read from the
  environment alone. Name the socket after the app identifier, as the plugin
  did, so the dev build (`io.github.x7c1.delta.dev`, `make desktop-dev`) and
  the installed app stay independent. Keep the full path within the
  `sun_path` limit (104 bytes on macOS) and say what happens when it would
  not fit.
- Keep the plugin's behaviour otherwise: a second launch notifies the running
  copy and exits 0 before any other plugin or `setup` runs (see the ordering
  comments in `main.rs` around the `.plugin(tauri_plugin_single_instance::init`
  call and in `relaunch.rs`); a stale socket left by a crashed copy
  (`ConnectionRefused`) is removed and taken over; the socket is removed on
  `RunEvent::Exit`.
- One client that connects and never sends or closes must not stop later
  launches from being forwarded (read each connection off the accept loop,
  with a bound on how long it may take).
- If the lock cannot be set up (the directory cannot be resolved, bind
  fails), log why at `warn` or above and launch normally, as the plugin does.
  Never fall back to `/tmp`.
- Write the socket logic so it can be unit-tested on any Unix against a
  temporary directory (bind, forward a second launch, take over a stale
  socket, survive a client that never closes), with only the directory choice
  and the wiring specific to macOS. CI runs on Linux, so tests that only
  compile on macOS would not be gated there.

Out of scope: a running copy of an *older* build still listening on the old
`/tmp` path is not detected by the new build (both would run if a user starts
both at once). Updates relaunch only after the old process exits
(`relaunch.rs`), so this needs no handling; say so in a comment where the
socket path is chosen.

Update the developer docs that describe single-instance
(`docs/guides/development/README.md` around "single-instance
(`tauri-plugin-single-instance`", and `docs/guides/development/local-run.md`
around "single-instance lock") so they name the per-platform mechanism.

### 2. The login shell's `env` output is split on newlines

`src/login_env/read_login_shell_env.rs` runs the user's login shell with
`printf '%s\n' <marker>; env; printf '%s' <marker>`, and
`src/login_env/parse_printed_env.rs` splits the output on newlines. A value
of any variable that contains `\nPATH=/somewhere` is then read as a `PATH`
line, and the last one wins: the `PATH` given to tmux, claude, codex and the
opener can be replaced by whoever controls that variable's value.

Fix: print the environment NUL-separated (`env -0`, which both macOS's
`/usr/bin/env` and GNU coreutils accept; the command string is run by `sh`,
`bash`, `zsh` and `fish` alike, and `env` there is the external binary) and
split entries on `\0`, so a newline inside a value stays inside that value.
Keep the marker scheme and the existing guarantees (output around the
markers ignored, `_=<marker>` not ending the environment, empty values
skipped, only `PATH` / `LANG` / `LC_*` imported). Update the module docs in
`src/login_env/mod.rs` and the comment on `login_shell_args`, and the parser
tests, which currently describe newline-separated input.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A unit test binds the new single-instance socket in a temporary
      directory and shows that a second launch's notification reaches the
      running copy's callback and that the second launch is told to exit.
- [x] A unit test shows a stale socket file (nothing listening) is removed and
      taken over by the new copy.
- [x] A unit test shows a client that connects and never sends or closes does
      not prevent a later launch's notification from reaching the callback.
- [x] A unit test shows the socket path is built in the given directory from
      the identifier and that a path over the `sun_path` limit is handled as
      the code documents.
- [x] A parser test shows that a value containing `\nPATH=/evil` in the
      NUL-separated output does not change the imported `PATH`, and the
      existing marker / empty-value / imported-name cases still hold for the
      NUL-separated format.

### Before merge (verified outside the check command)

- [x] On the macOS development machine, with `make desktop-dev`: the first
      launch creates its socket under the per-user temporary directory and
      nothing named `*delta*_si.sock` under `/tmp`; a second launch of the
      same binary exits 0 while the first keeps running; after quitting, the
      socket is gone.
- [ ] On macOS, a second launch brings the running window to the front —
      needs a person: macOS does not let a process started from a background
      terminal take focus (the plugin-based build behaves the same), and the
      agent here has no accessibility access to observe the window.
- [x] On the same machine, with the shell's rc defining a variable whose value
      contains a newline followed by `PATH=/nonexistent`, the dev app still
      starts its server and finds `tmux` (the imported `PATH` is the real one).
- [ ] On Linux, a second launch still raises the running window (the plugin
      path is unchanged) — needs a person: this machine is macOS, and no agent
      here can reach the Linux desktop session.

## Last failure

The canonical check (`make check`) failed in `check-backend-doc`
(`RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items`):

```
error: `parse_printed_env` is both a function and a module
  --> crates/apps/delta-desktop/src/login_env/read_login_shell_env.rs:24:17
   |
help: to link to the function, add parentheses
24 | /// variable ([`parse_printed_env()`]).
help: to link to the module, prefix with `mod@`
24 | /// variable ([`mod@parse_printed_env`]).
error: could not document `delta-desktop`
```

Every other step that had run before it passed. Run the rustdoc command above
for the crates you touch before reporting success.
