---
status: completed
pipeline_phase: null
plan: null
base_ref: feat/in-app-update
perspectives: [completeness, clarity, error-type-design, rust-module-structure, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && git grep -q --untracked '/api/latest-release/install' -- backend/crates/gateway/delta-wire/src && git grep -q --untracked '/api/latest-release/restart' -- backend/crates/gateway/delta-wire/src && git grep -q --untracked '/api/latest-release/install' -- docs/guides/api && git grep -q --untracked 'org.freedesktop.policykit.exec.path' -- backend/crates/apps/delta-desktop && git grep -q --untracked 'pkexec' -- docs/guides/security.md && git grep -q --untracked 'releases/tags/' -- backend/crates && ! git grep -q --untracked 'current_exe' -- backend/crates/apps/delta-desktop/src"
assignee: null
branch: task/1007-1341-feat-install-a-downloaded-update-on-linux-and-restart
created_at: 2026-10-07T13:41:37Z
updated_at: 2026-10-07T14:43:32Z
---

# feat: install a downloaded update on Linux and restart

## Overview

The desktop app can already download and verify a newer release: a desktop
build made by the release workflow offers **Update**, and
`POST /api/latest-release/download` puts the verified asset into the data
directory's `updates/` (`backend/crates/domain/delta-usecase/src/release_update/`,
`backend/crates/gateway/release-feed/src/github_asset_downloader.rs`), after
which the footer shows "Update ready". This task adds the next steps on Linux:
installing the verified `.deb` as root and restarting into the new version. It
also builds the parts a later macOS change will share: the update's state
after "ready", the restart, and the manual fallback. This PR targets the
`feat/in-app-update` integration branch.

### How root is obtained: a dedicated helper behind polkit

Only a dedicated helper runs as root, started through a polkit action of its
own with `pkexec`. Delta's server and UI never hold root, never run `sudo` in
a terminal, and never run `apt` with arguments of their choosing. Rejected on
purpose:

- `sudo apt install` in a tmux pane of Delta's: sudo remembers the
  authentication per tty, and Delta's tmux server hosts autonomous agents that
  can `send-keys` into any pane on the same socket, so an authenticated pane
  would hand them root.
- `pkexec apt install <file>` straight from the server: the `.deb` sits in a
  user-writable directory and can be swapped between the server's
  verification and apt reading it (its maintainer scripts run as root), and
  the authorization would cover running apt with any arguments.

The helper:

- is a small Rust binary of its own (e.g. `delta-update-helper`), installed by
  the `.deb` root-owned at a fixed absolute path, and is the only program the
  polkit action allows (`org.freedesktop.policykit.exec.path` in a policy file
  under `/usr/share/polkit-1/actions/`, shipped through the Tauri bundle
  config for Linux, `backend/crates/apps/delta-desktop/tauri.linux.conf.json`).
  The action asks for an administrator's password on every use
  (`auth_admin`, never `auth_admin_keep`) and its message names Delta's
  update so the user knows what they allow.
- trusts nothing its caller says beyond *which* version to install and
  *which* file holds it, and checks everything itself, as root:
  1. copies the file into a root-owned temporary directory (mode 0700)
     before reading it, so it cannot be swapped afterwards; refuses a symlink;
  2. fetches the release's own asset digest from GitHub's API
     (`https://api.github.com/repos/x7c1/delta/releases/tags/<tag>`, the
     same `sha256:<hex>` the server checked) and compares the copy's sha256
     with it; a missing digest is a refusal;
  3. checks the package's `Package` field is `delta-desktop` and its version
     is the requested one and newer than the installed `delta-desktop`
     (`dpkg-deb` / `dpkg-query`);
  4. installs the copy with `apt-get install -y <absolute path>` (so
     dependencies resolve) and removes its temporary directory.
- exits with a distinct status for each refusal and failure, and prints a
  one-line reason to stderr, so the server can report what happened.
- keeps its decisions in functions that unit tests cover without root (path
  and symlink checks, digest parsing and comparison, version comparison,
  argument parsing); only the thin shell of running `dpkg`/`apt-get` is left
  to the on-machine check.

### Server: install, then restart

- `POST /api/latest-release/install` (Linux desktop `release` builds only)
  runs `pkexec <helper> install --version <tag> --file <path of the ready
  download>` and reports the update's state on `GET /api/latest-release`:
  `installing`, then `installed` (waiting for a restart) or `failed` with the
  cause. It refuses (409, a stable `update_*` code) unless a verified
  download of the newer release is ready, and never accepts a path or a
  command from the request. How the install runs sits behind a port of its
  own in `delta-usecase`, with the pkexec implementation in a gateway, so the
  use case is tested with a fake.
- `pkexec` outcomes map to user-facing results: the user dismissed the
  dialog (exit 126) → back to "ready", no error shown; not authorized or no
  polkit agent (127) → the manual fallback below; the helper's own refusals
  and apt failures → `failed` with the helper's reason.
- **Manual fallback.** Whenever Delta cannot install (no `pkexec`, no polkit
  agent, the helper missing, the install failed), the UI shows the exact
  command for the user's own terminal (`sudo apt install <absolute path of
  the verified .deb>`) with a copy button and a link to the release page.
  This is the way out even if this version's updater turns out to be broken,
  so it must not depend on the helper.
- `POST /api/latest-release/restart` (only once `installed`) stops the
  server through `AppState::stop` with a new `ServerStopped` variant for the
  restart, next to `Erased` (`backend/crates/apps/delta-server/src/serve.rs`).
  The desktop shell (`backend/crates/apps/delta-desktop/src/main.rs`,
  `start_server`) handles it by starting a detached child that waits for the
  current process to exit and then runs `/usr/bin/delta-desktop`, then exits.
  The single-instance plugin would otherwise make the new process exit at
  once. The path is the installed one, not `std::env::current_exe()`: after
  dpkg replaces the binary, `/proc/self/exe` reads `... (deleted)`. tmux
  sessions keep running across the restart as they already do.
- On startup, files in `updates/` for a version not newer than the running
  one are removed, so an installed update does not linger.

### UI

After "Update ready" the footer offers **Install** (Linux); while installing
it shows progress; once installed it offers **Restart**; on a failure it shows
the cause and the manual command. The footer keeps its whole-phrase wrapping.

### Docs

- `docs/guides/api/`: the two endpoints, the new states and error codes.
- `docs/guides/security.md`: a section on updates — the root helper as the
  only root entry, what it verifies, why there is no sudo in a terminal, and
  `updates/` among the data files.
- `docs/guides/install/`: how in-app updates work on Linux, the polkit
  dialog, and the manual command.

### Out of scope

- macOS: replacing `Delta.app` (a later change on this branch reuses the
  state and the restart built here).
- Persisting "ready" across restarts; a restart before installing downloads
  again.
- Signed releases.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The helper's checks are covered by unit tests: a symlinked file is
      refused; a digest that is missing, malformed or different from the
      file's sha256 is refused; a package that is not `delta-desktop`, whose
      version differs from the requested one, or that is not newer than the
      installed one is refused; arguments other than `install --version
      <tag> --file <absolute path>` are refused; the digest is read from the
      release-by-tag JSON for the right asset.
- [x] The `.deb` bundle config installs the helper at a fixed absolute path
      and ships a polkit policy whose `org.freedesktop.policykit.exec.path`
      names exactly that path, with `auth_admin` (a test reads the policy
      file and the bundle config and checks both).
- [x] `POST /api/latest-release/install` answers per state (app tests with a
      fake installer): CLI launcher, `local` build, non-Linux platform, no
      ready download → refused with their codes; ready → `installing`, then
      `installed` on success; dismissed (126) → back to ready; no agent /
      not authorized (127) → a state carrying the manual command; helper
      refusal or apt failure → `failed` with the reason; a second request
      while installing starts nothing.
- [x] `POST /api/latest-release/restart` is refused unless `installed`, and
      otherwise stops the server with the restart variant (app test).
- [x] The shell's relaunch builds a command that waits for the given pid and
      then runs `/usr/bin/delta-desktop`, and `std::env::current_exe` is not
      used in the desktop crate (the `check_command` greps for it).
- [x] Startup removes `updates/` files for versions not newer than the
      running one and keeps a newer one (test against a temp data dir).
- [x] The footer renders Install, installing, Restart, failed, and the
      manual command with its copy button (component tests).
- [x] The API, security and install docs describe the endpoints, the helper
      and the manual command (the `check_command` greps them).

### Before merge (verified outside the check command)

- [ ] On this Linux machine, a `.deb` of this branch built with
      `DELTA_BUILD_ORIGIN=release` and its workspace version set to `0.4.9`
      is installed with `sudo apt install` (needs a person: the password),
      and started with `DELTA_DATA_DIR` pointing at a scratch directory so the
      real data is untouched. Update, then Install, shows the polkit dialog
      naming Delta's update (needs a person: the password); after it the
      footer offers Restart, and `dpkg-query -W delta-desktop` reports
      `0.5.0`.
- [ ] Restart relaunches the app as the official v0.5.0 (its footer and
      `GET /api/version` say 0.5.0), and a tmux session started before the
      update is still alive (`tmux -L <socket> ls`).
- [ ] Dismissing the polkit dialog returns the footer to "Update ready", and
      with the helper made unreachable (e.g. a build whose helper path does
      not exist) the footer shows the manual command, which installs the
      update when run in a terminal.
