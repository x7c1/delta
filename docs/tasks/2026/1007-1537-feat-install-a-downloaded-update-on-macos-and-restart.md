---
status: completed
pipeline_phase: null
plan: null
base_ref: feat/in-app-update
perspectives: [completeness, clarity, error-type-design, rust-module-structure, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && git grep -q --untracked 'hdiutil' -- backend/crates/gateway && git grep -q --untracked 'AppTranslocation' -- backend/crates && git grep -q --untracked 'CFBundleShortVersionString' -- backend/crates && git grep -q --untracked 'hdiutil' -- docs/guides/security.md && git grep -q --untracked -i 'translocat' -- docs/guides/install/macos.md && ! git grep -q --untracked 'current_exe' -- backend/crates/apps/delta-desktop/src"
assignee: null
branch: task/1007-1537-feat-install-a-downloaded-update-on-macos-and-restart
created_at: 2026-10-07T15:37:15Z
updated_at: 2026-10-07T16:29:37Z
---

# feat: install a downloaded update on macOS and restart

## Overview

On Linux the desktop app can already download, verify, install and restart
into a newer release: the footer goes Update → Update ready → Install →
Installing… → Restart, `POST /api/latest-release/install` runs an
`UpdateInstaller` (a port in `backend/crates/domain/delta-usecase/src/ports/update_installer.rs`,
implemented for Linux by `PkexecInstaller` in
`backend/crates/gateway/update-installer/`), and `POST /api/latest-release/restart`
stops the server with `ServerStopped::Restart` so the desktop shell relaunches
the app through `backend/crates/apps/delta-desktop/src/relaunch.rs`. The
states (`installing`, `installed`, `failed`, `unavailable` with a manual
command, `rejected`), the API and the footer are platform-neutral. On macOS
the download (`Delta_<version>_aarch64.dmg` into the data directory's
`updates/`) works, but Install is not offered: `installs` is false and
`relaunch::installed_app()` returns `None`. This task makes Install and
Restart work on macOS (Apple silicon), reusing all of that. This PR targets
the `feat/in-app-update` integration branch.

### No root on macOS

The app bundle is replaced as the user running Delta; nothing runs as root
and there is no helper or `pkexec`. Where the current bundle's directory is
not writable by that user (a standard account and `/Applications` owned by
the admin group, say), Delta does not ask for a password: it reports
`unavailable` and the manual way below.

### Installing (a new `UpdateInstaller` for macOS)

A new gateway implementation (in `update-installer`, next to
`PkexecInstaller`) does, in order:

1. **Find the running bundle** from the server's executable path resolved
   once, before anything is replaced: `<dir>/Delta.app/Contents/MacOS/<exe>` →
   `<dir>/Delta.app`. Do not hard-code `/Applications`. A process not running
   from inside an `.app` bundle (a dev build, the CLI) has nothing to
   replace → `unavailable`.
2. **Refuse App Translocation.** A `Delta.app` launched straight from the
   `.dmg` or the Downloads folder without being moved runs from a random
   read-only copy under `/private/var/folders/…/AppTranslocation/`; replacing
   that does nothing. Detect the path and report `unavailable` with a manual
   text telling the user to move Delta to `/Applications` first.
3. **Check the bundle's directory is writable** by this user; otherwise
   `unavailable`.
4. **Re-verify the file** just before using it: its sha256 against the digest
   the download was verified with (the user owns `updates/`, so check again
   rather than trust the earlier result); a symlink or non-regular file is
   refused. A mismatch is `rejected` (the file is deleted and the release is
   downloaded again, as on Linux).
5. **Mount the image read-only and privately**: `hdiutil attach -nobrowse
   -readonly -noautoopen -mountpoint <fresh temp dir> <dmg>`, always
   detached again (`hdiutil detach`) on every path, including failures.
6. **Check what is inside** before copying: exactly one `Delta.app` at the
   image's root, its `Contents/Info.plist` with `CFBundleIdentifier`
   `io.github.x7c1.delta` and `CFBundleShortVersionString` equal to the
   requested version (read the plist with the `plist` crate, not by
   shelling out). Anything else is `rejected`.
7. **Copy next to the current bundle**, under a temporary name in the same
   directory (so the final step is a rename on one volume), preserving
   symlinks, permissions and extended attributes (`ditto` is the macOS tool
   for this; a hand-rolled copy must keep the bundle's code signature
   intact).
8. **Swap by rename**: the current bundle to a backup name in the same
   directory, then the new copy to `Delta.app`. If the second rename fails,
   rename the backup back. The running binary is never overwritten in place
   (that breaks its signature and the kernel kills it).
9. Report `installed`. The backup is removed by the next launch (below).

Each failure maps to the existing states: environment problems
(not in a bundle, translocated, not writable, `hdiutil` missing) →
`unavailable`; a file or image that fails its checks → `rejected`; anything
else (`hdiutil` failing, a copy or rename failing) → `failed` with the cause.
Map them through the existing `InstallError` variants; add a variant only if
none fits, and keep the usecase's handling (delete the file on `Rejected`,
keep it on `Installed`) as it is.

### Manual way on macOS

`unavailable` and `failed` carry a manual instruction like Linux's command.
On macOS it is not a shell command to paste but: open the downloaded `.dmg`
(with its absolute path, and a "Show in Finder" or open action if the UI can
offer it without new native plumbing; otherwise the path with a copy button)
and drag Delta to Applications, then restart Delta. Extend the wire's manual
field or add a sibling so the footer can show the right form per platform;
keep the Linux command unchanged.

### Restart on macOS

`relaunch.rs` gains the macOS path: the bundle path resolved at install time
is what is started again, with `open <path to Delta.app>` once this process
has exited (the single-instance plugin would otherwise make the new one exit
at once). `installed_app()` (or its replacement) must not use
`std::env::current_exe()` at restart time; keep the resolved path from the
install. Generalise the detached waiter so it can run `open <bundle>` on
macOS and the binary on Linux without changing the Linux behaviour.

### Clean-up at the next launch

At startup on macOS, remove a backup bundle left next to the running one by
a previous update (the backup name is fixed and Delta's own), and the
existing `updates/` clean-up keeps working.

### Docs

- `docs/guides/install/macos.md`: in-app updates on macOS, where they work
  (the app moved to `/Applications` and writable by you), and the manual way.
- `docs/guides/security.md`: the macOS part of the Updates section (no root,
  the re-verification, the mounted image's checks, why the bundle is renamed
  rather than overwritten).
- `docs/guides/api/settings.md`: `installs` on macOS and the manual
  instruction's macOS form.

### Out of scope

- Intel Macs, notarization and code signing of releases.
- Making the swap work for a non-admin user (no privilege escalation on
  macOS).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Bundle discovery is covered by unit tests: an executable inside
      `<dir>/Delta.app/Contents/MacOS/` yields `<dir>/Delta.app`; a path
      under `/private/var/folders/…/AppTranslocation/` is translocated; a path
      outside any `.app` yields nothing.
- [x] The image checks are covered by unit tests on a fake mounted tree:
      exactly one `Delta.app` with the right `CFBundleIdentifier` and
      `CFBundleShortVersionString` passes; a missing app, two apps, another
      identifier, another version, or an unreadable `Info.plist` is a
      rejection.
- [x] The swap is covered by tests on temp directories: after a successful
      swap `Delta.app` holds the new bundle and the backup holds the old;
      when the second rename fails the original `Delta.app` is restored; a
      symlinked or non-regular `.dmg` and a digest mismatch are rejections.
- [x] `hdiutil` is run through a seam the tests replace: attach is always
      followed by detach, on success and on every failure after attaching.
- [x] The installer's outcomes map to `unavailable` (not in a bundle,
      translocated, not writable, `hdiutil` missing), `rejected`, `failed`
      and `installed`, and the server reports `installs: true` on macOS
      desktop release builds (usecase/app tests with fakes).
- [x] The relaunch waits for the pid and then runs `open <bundle>` on macOS
      and the installed binary on Linux (unit tests on the built command),
      and `std::env::current_exe` is not used in the desktop crate's sources
      (the `check_command` greps for it).
- [x] Startup removes Delta's backup bundle next to the running one (test
      against a temp directory).
- [x] The footer shows the macOS manual instruction (the `.dmg` path and the
      drag-to-Applications text) for `unavailable` and `failed`, and the Linux
      command unchanged (component tests).
- [x] The macOS install guide, the security guide and the API guide describe
      the macOS install (the `check_command` greps them).

### Before merge (verified outside the check command)

- [ ] On a Mac (Apple silicon) — needs a person at that machine — a `.dmg`
      of this branch built with `DELTA_BUILD_ORIGIN=release` and its
      workspace version set to `0.4.9`, installed into `/Applications` and
      started with `DELTA_DATA_DIR` pointing at a scratch directory, offers
      Update; Update then Install replaces `/Applications/Delta.app` with the
      official v0.5.0 without a password prompt, and the footer offers
      Restart.
- [ ] Restart relaunches the app as v0.5.0 (its footer says so), a tmux
      session started before the update is still alive, the backup bundle is
      gone after the relaunch, and Gatekeeper does not block the new bundle
      (no quarantine prompt on the self-downloaded image).
- [ ] Started from the mounted `.dmg` without moving it (translocated), the
      footer shows the "move Delta to Applications" instruction instead of
      installing.
