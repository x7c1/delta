---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && git grep -q 'Starting Delta' -- backend/crates/apps/delta-desktop/src && git grep -qE 'spawn_blocking|thread::spawn' -- backend/crates/apps/delta-desktop/src/main.rs"
assignee: null
branch: task/1006-1715-feat-desktop-open-the-window-at-once-and-start-the-server-behind-a-placeholder-page
created_at: 2026-10-06T09:39:05Z
updated_at: 2026-10-06T10:31:27Z
---

# feat(desktop): open the window at once and start the server behind a placeholder page

## Overview

`delta-desktop` shows nothing until it has read the login shell's
environment (up to eight seconds) and started its server (tmux check,
SQLite open and migration, `claude --version`, re-adoption of surviving
sessions). On a slow shell or a large database the user double-clicks the
icon and sees no window for seconds; many click again, which the
single-instance plugin turns into a no-op. Now that the login shell's values
are plain data handed to the configuration (the previous task), nothing
forces this order any more.

### Change

In `backend/crates/apps/delta-desktop/src/main.rs`:

- **Open the window first.** In `setup`, create the window immediately on a
  placeholder page: a static HTML document in the app's own colours (the
  SPA's light-theme background) with one short line, "Starting Delta…", and
  nothing else — no spinner that would still be spinning on a failure. Serve
  it with `register_uri_scheme_protocol` (one scheme, one path) rather than
  a `data:` URL, so `links::classify` can name it as the shell's own page.
- **Start in the background.** Run the login shell read and `start_server`
  on a thread (`std::thread::spawn` or the runtime's `spawn_blocking`) and,
  when the port is known, `window.navigate(http://127.0.0.1:<port>/)` on the
  main thread (`AppHandle::run_on_main_thread`). The eight-second timeout of
  the login shell read stays: it still bounds how long a session launched
  right after startup waits for the `PATH`, but no longer blocks a window.
- **The port is no longer known when the window is built**, so what today
  captures it must read it from shared state the background start fills in:
  the `on_navigation` and `on_new_window` closures (a navigation before the
  port is known is a navigation to the placeholder, which stays in the
  window; everything else goes to the browser as today), and
  `links::classify`, which gains the placeholder's origin as `OwnOrigin`.
- **Log the two moments** at `info`: when the placeholder window is shown
  and when it navigates to the server (with the port), so a launch can be
  checked from the log alone — the gap between the two lines is the time the
  user would otherwise have stared at nothing.
- **Failures** keep their dialog: `report_startup_failure` parents it on the
  window (`tauri-plugin-dialog`'s `.parent(&window)`), so the dialog appears
  over the placeholder instead of beside nothing, and exits 1 when
  dismissed, as today.
- `focus_running_window`'s "no window yet" branch becomes unreachable in
  practice (the window exists microseconds after setup); keep the log line
  but reword the comment.
- Rewrite the module doc's "Startup, in order" list and the `login_env`
  module doc for the new order.
- The window-size task (`inner_size`, min size, state restore) touches the
  same `open_window`; this task keeps `inner_size(1280.0, 800.0)` as it is.

### Docs

`docs/guides/development/local-run.md` "The desktop app": the window opens
at once and shows the page when the server is up; the `PATH` and locale
bullet loses its "if the shell does not answer within a few seconds, the
window waits" implication. `docs/guides/install/README.md`: no change unless
a sentence there describes the wait.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The placeholder text exists and the start runs off the main thread
      (gates in `check_command`).
- [x] Unit tests: `links::classify` returns `OwnOrigin` for the placeholder
      URL and still `Web`/`NotWeb` for the rest; the port-holding shared
      state answers "unknown" before the start and the port after.
- [x] With `SHELL` set to a script that sleeps 7 seconds before `exec`ing
      `/bin/sh`, the app's log shows the placeholder window line several
      seconds before the navigation line (an automated check the orchestrator
      runs with a scratch identifier; the visual confirmation is the item
      below).
- [x] `make check` passes and `make desktop-dev-build` builds.

### Before merge (verified outside the check command)

- [x] On macOS: set `SHELL` to a script that sleeps 7 seconds and then
      `exec`s the real shell with its arguments, launch the dev build; the
      window appears within a second showing the placeholder and switches to
      the app when the server is up. With `tmux` hidden from `PATH`, the
      "could not start" dialog appears over the placeholder window and the
      app exits when it is dismissed. Launching the app a second time while
      the first is still on the placeholder brings that window forward. A
      session left open across the restart is re-adopted.
- [ ] On Ubuntu (GNOME): the same, from the application grid.

