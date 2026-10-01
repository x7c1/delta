---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! git grep -nE 'tmux -L delta( |`|$)|socket `delta`|TMUX_SOCKET:-delta}|DEFAULT_TMUX_SOCKET: &str = \"delta\"' -- ':!docs/tasks' && git grep -qF 'io.github.x7c1.delta.dev' -- scripts/ && git grep -qF '\"io.github.x7c1.delta\"' -- backend/crates/libs/"
assignee: null
branch: task/1002-0003-fix-give-the-tmux-socket-an-identifier-name-and-separate-dev
created_at: 2026-10-01T15:03:52Z
updated_at: 2026-10-01T15:18:04Z
---

# fix(tmux): give Delta's tmux socket an identifier name and a separate one for dev

## Overview

Delta runs every session on its own tmux server, selected with `tmux -L <socket>`.
The default socket name is the single word `delta`
(`backend/crates/libs/delta-bootstrap/src/config.rs`, `DEFAULT_TMUX_SOCKET`), which
sits in the user's tmux namespace (`/tmp/tmux-<uid>/delta`) where any other tool or
a hand-made socket can collide with it. It also does not match the app's identifier,
`io.github.x7c1.delta`, which already names the app data directory.

The same name is shared by the desktop app and `make dev`
(`scripts/dev.sh`, `DELTA_TMUX_SOCKET="${DELTA_TMUX_SOCKET:-delta}"`). Their
databases differ, so neither UI shows the other's sessions, but there is only one
tmux server: `tmux -L delta ls` lists both, and `make down` / `make reset` run
`kill-server` on it and end the installed app's sessions too
(`docs/guides/development/local-run.md` states this as a caveat today).

Change the names:

- The default socket (used by the desktop app, `make app-dev`, and a bare
  `delta-server`) becomes `io.github.x7c1.delta`.
- `scripts/dev.sh` defaults to `io.github.x7c1.delta.dev`, so `make dev`,
  `make down` and `make reset` only ever touch the dev server's sessions.
- `DELTA_TMUX_SOCKET` keeps overriding both, as now.

Sessions left on the old `delta` socket are deliberately **not** migrated or
discovered: after the rename Delta simply does not see them. Nobody but the
developer has run a build with the old name, and the release notes will tell them to
run `tmux -L delta kill-server` once. Do not add any code that looks at the old
socket.

Update every place that names the socket so they stay true:

- `backend/crates/libs/delta-bootstrap/src/config.rs` (`DEFAULT_TMUX_SOCKET`) and
  the doc comments around it; `backend/crates/apps/delta-server/src/config/` uses
  the constant.
- `scripts/dev.sh` (the default, the comment above it, and the login hint near the
  top that says `tmux -L delta attach -t delta-1`).
- `docs/guides/development/local-run.md`: the `tmux -L delta attach` examples
  become the dev socket, and the desktop-app section's caveat that `make down`
  ends the app's sessions is replaced by the fact that the two now use separate
  sockets.
- `docs/guides/install/README.md`: the socket name in the "sessions run on
  Delta's own tmux server" paragraph and in the removal steps.
- Test fixtures that only use `"delta"` as an arbitrary socket string (for
  example `attach_args("delta", "%3")` in `backend/crates/apps/delta-server/src/pty.rs`)
  do not need to change; the check gate only targets the default and the docs.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `DEFAULT_TMUX_SOCKET` is `"io.github.x7c1.delta"`, and the server config test
      that asserts the default still passes against the constant (`make check`;
      the `git grep -qF '"io.github.x7c1.delta"' -- backend/crates/libs/` gate).
- [x] `scripts/dev.sh` defaults `DELTA_TMUX_SOCKET` to `io.github.x7c1.delta.dev`
      and still honours an explicit `DELTA_TMUX_SOCKET` (the `git grep -qF
      'io.github.x7c1.delta.dev' -- scripts/` gate).
- [x] No document, script or default outside `docs/tasks/` still names the socket
      `delta` (`tmux -L delta …`, "socket `delta`", `${DELTA_TMUX_SOCKET:-delta}`,
      or the old constant) — the negative `git grep` gate in `check_command`.

### Manual / on-hardware (verified by a human before merge)

- [ ] With the desktop app running a session, `make dev` followed by `make down`
      leaves the app's session alive (`tmux -L io.github.x7c1.delta ls` still lists
      it) while the dev session is gone.
