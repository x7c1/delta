---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && git grep -q 'RunEvent::Exit' -- backend/crates/apps/delta-desktop/src && git grep -q 'Library/WebKit' -- backend/crates/apps/delta-desktop/src && ! git grep -qi 'rm -rf ~/Library/WebKit' -- docs/guides/install/README.md"
assignee: null
branch: task/1006-1540-feat-desktop-finish-an-erase-by-removing-the-shells-own-files-and-quitting-with-the-report
created_at: 2026-10-06T07:38:19Z
updated_at: 2026-10-06T08:13:40Z
---

# feat(desktop): finish an erase by removing the shell's own files and quitting with the report

## Overview

`POST /api/storage/erase` makes the server remove what Delta created, delete
its data directory and stop; the desktop shell then exits 0 with nothing
shown. Two things are left: the shell's own files, and the report.

- **The shell's files.** The webview's storage lives where the platform
  puts it, under the app's bundle identifier: on Linux inside the app data
  directory (`localstorage/`, `storage/`, `CacheStorage/`, `WebKitCache/`),
  which is why the server's erase leaves that directory behind on Linux —
  it removes only its own files and keeps a directory that still holds
  something — and on macOS outside it, in `~/Library/WebKit/<identifier>/`
  and `~/Library/Caches/<identifier>/`. WebKit writes these back while the
  webview is alive, so they can only be deleted after the window is gone. A later task adds a window-state file
  under the app's config directory; it must be covered without another
  change here.
- **The report.** The server's `ServerStopped::Erased(report)` says what was
  kept and why. In the browser the SPA shows it; in the desktop app the
  window closes with the server, so the shell must show it itself.

### Change

In `backend/crates/apps/delta-desktop/src/main.rs`:

- Replace `.run(context)` with `tauri::Builder::build(context)?.run(|app, event| ..)`
  so the shell receives `RunEvent::Exit`.
- When the serve task returns `Erased(report)`: close the window, then show
  the report in a native message dialog (`tauri-plugin-dialog`, the way
  `report_startup_failure` does): "Delta removed its files and will quit."
  followed by the kept items, each as its path or branch name and reason, or
  "Nothing was kept." Exit 0 when the dialog is dismissed. Record on the
  managed state that this exit is an erase.
- In `RunEvent::Exit`, when the exit is an erase, delete the per-app
  directories Tauri's path resolver names for this identifier
  (`app_data_dir`, `app_local_data_dir`, `app_config_dir`, `app_cache_dir`,
  `app_log_dir`), and on macOS `~/Library/WebKit/<identifier>`. Most do not
  exist; a missing one is not an error, any other failure is logged at
  `warn` and does not change the exit code. Put the list in a pure function
  (`erase_paths(identifier, resolver dirs) -> Vec<PathBuf>`) with a unit
  test per platform (`cargo test -p delta-desktop` runs under
  `check-desktop-build`), so the set is readable in one place.
- Update the module doc's "Closing the window ends the process" paragraph
  with the erase path, and the install guide: "Removing everything" drops
  the manual webview steps for both platforms (the macOS `rm -rf` of the
  WebKit and Caches directories, the Ubuntu `rm -rf` of the data directory)
  and says the in-app action removes the shell's files too, so on the
  desktop app nothing is left under the identifier; the pre-v0.5 names stay
  manual. The browser version's paragraph keeps saying the directory stays
  when it holds something else.

Do not add Tauri IPC: the server's return value is the whole channel between
the page and the shell, as the first task designed it.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The shell handles `RunEvent::Exit`, names the macOS WebKit directory,
      and the install guide no longer tells macOS users to remove it by hand
      (gates in `check_command`).
- [x] A unit test fixes the directory set per platform, including that the
      macOS list has the WebKit and Caches directories and the Linux list
      has the config directory under which the window state will live.
- [x] `make check` passes, and `make desktop-dev-build` builds.

### Before merge (verified outside the check command)

- [x] On macOS with the dev build (`make desktop-dev`): open a session so the
      webview has storage, erase from Settings; the dialog lists what was
      kept, and after dismissing it the app has quit and
      `~/Library/WebKit/io.github.x7c1.delta.dev`,
      `~/Library/Caches/io.github.x7c1.delta.dev` and
      `~/Library/Application Support/io.github.x7c1.delta.dev` no longer
      exist, while the installed app's directories (without `.dev`) are
      untouched. Relaunching starts from a fresh state.
- [ ] On Ubuntu (GNOME): the same, with `~/.local/share/<id>`,
      `~/.config/<id>` and `~/.cache/<id>`.
