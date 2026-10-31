---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1031-1852-hand-an-activation-token-to-launched-apps
created_at: 2026-10-31T09:52:03Z
updated_at: 2026-10-31T10:13:16Z
---

# fix(desktop): bring the apps Delta opens to the front on Linux

## Overview

On GNOME (Wayland), the apps the desktop app starts never come to the front.
"Open in VS Code" and a web link followed in the window both end in a
"<app> is ready" notification, and the user has to click the notification to
see the window. The compositor's focus-stealing prevention refuses an
activation that carries no activation token, and Delta passes none:

- "Open in VS Code" is spawned by the server's `SystemOpener`
  (`backend/crates/gateway/external-opener/src/lib.rs`), wired in
  `backend/crates/libs/delta-bootstrap/src/build.rs`, with only the
  configuration's `child_env` on top of the inherited environment.
- A web link goes to `xdg-open` through `links::open_externally`
  (`backend/crates/apps/delta-desktop/src/links.rs`).
- Delta removes its own launch token from its environment at startup
  (`activation_token::take_from_env`), so the children inherit none either.

It is not a limitation of the launched apps: with VS Code 1.140 already
running natively on Wayland, `code <dir>` run from GNOME's Alt+F2 dialog (which
hands the command a token) brings the VS Code window to the front.

Delta can mint the token itself. At the moment of the click the Delta window
has focus, and the compositor grants an xdg-activation token to the focused
surface. GTK 3 does this in `GdkAppLaunchContext`'s startup notify ID
(`gdk_app_launch_context_get_startup_notify_id`; on Wayland it requests an
`xdg_activation_v1` token for the focused surface with the latest input serial,
on X11 it makes a startup-notification ID). The installed GTK is 3.24.52, which
carries `xdg_activation_v1`. This is the counterpart of what
`backend/crates/apps/delta-desktop/src/activation_token.rs` already does for a
second launch of Delta: there Delta *receives* a token; here it *hands one on*.

### The change

- In the desktop shell on Linux, mint a fresh activation token for each app it
  starts, on the GTK main thread, from the Delta window's display. Put it in the
  child's environment as both `XDG_ACTIVATION_TOKEN` and `DESKTOP_STARTUP_ID`
  (the same pair a launcher sets; see `activation_token.rs`). One token per
  launch: a token is single-use.
- Web links: `open_externally` sets the token on the `xdg-open` child. Check on
  which thread the webview's new-window and navigation handlers run, and mint
  on the main thread either way.
- "Open in VS Code": the server runs inside the desktop process, but the
  request arrives on the tokio runtime. Give the `SystemOpener` a way to obtain
  extra launch environment per spawn (for example a port or callback the
  desktop shell supplies when it builds the server; `delta-bootstrap` and the
  CLI server keep a no-op default). The desktop's implementation asks the main
  thread for a token and waits for it with a short timeout. Keep the
  `ExternalOpener` port in `delta-usecase` unchanged unless that is clearly the
  better seam; activation is a platform concern of the spawn, not of the use
  case.
- When no token can be had — the server run on its own (`make dev`, the CLI,
  a browser as the client), macOS, a display that is neither Wayland nor X11,
  minting fails or times out — launch exactly as today, without the variables,
  and log at `debug` (a failure to mint at `warn`). A launch is never refused
  or delayed beyond the timeout for want of a token.
- Keep the module docs current: `activation_token.rs` (it now also hands tokens
  on), `links.rs`, `external-opener/src/lib.rs`, and the desktop `main.rs`
  overview where it lists what the shell does on Linux.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A unit test asserts that a command `SystemOpener` builds carries
      `XDG_ACTIVATION_TOKEN` and `DESKTOP_STARTUP_ID` set to the token the
      supplied source returns, and carries neither when the source returns none.
- [x] A unit test asserts the same for the `xdg-open` command `links.rs` builds
      (factor the command construction out of the spawn so it can be inspected,
      as `SystemOpener::command` is).
- [x] The server built without the desktop shell (`delta-bootstrap`'s default)
      spawns external openers without the activation variables; covered by a
      test or by the default source returning none in the test above.

### Before merge (verified outside the check command)

- [ ] (needs a person) On GNOME Wayland, with VS Code already running and
      behind another window, "Open in VS Code" from the desktop app built with
      `make desktop` brings the VS Code window to the front without a
      notification.
- [ ] (needs a person) On GNOME Wayland, following a web link in a message in
      the desktop app brings the browser window to the front without a
      notification.
