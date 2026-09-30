---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0930-2100-fix-open-links-from-the-desktop-app-in-the-default-browser
created_at: 2026-09-30T12:11:41Z
updated_at: 2026-09-30T12:33:24Z
---

# fix(app): open links from the desktop app in the default browser

## Overview

In the desktop app, clicking a link in the conversation pane does
nothing. The links are ordinary anchors with `target="_blank"`
(`frontend/packages/apps/web/src/features/transcript/AssistantMarkdown.tsx`,
and the pull-request link in `features/navigator/SessionNode.tsx`): in a
browser they open a new tab, but the Tauri webview has no tabs, so the
click becomes a new-window request, and the shell
(`backend/crates/apps/delta-app/src/main.rs`, `open_window`) handles none,
so WebKit drops it. A link without `target` would instead navigate the
app's only window away from Delta, with no way back.

### Change

1. **New-window requests.** In the shell's window builder, handle new-window
   requests (Tauri 2's `on_new_window`, or the equivalent the installed
   version offers): deny the new webview window and hand the URL to the
   operating system's default opener instead — `open <url>` on macOS,
   `xdg-open <url>` on Linux — so the link opens in whatever browser the
   user has set as default. Use `tauri-plugin-opener` if it fits without
   granting the page any IPC; otherwise spawn the opener as a detached
   child without a shell (as `backend/crates/gateway/external-opener`
   does), logging a failure rather than ignoring it.
2. **Navigation away from Delta.** Also guard in-window navigation
   (`on_navigation`): allow `http://127.0.0.1:<port>/…` (the app's own
   origin, including reloads and SPA routes), and for any other URL cancel
   the navigation and open it the same way. The window must never leave
   Delta's own page.
3. **Only web links.** Open only `http` and `https` URLs externally. Any
   other scheme (`file:`, `javascript:`, custom app schemes) is refused
   and logged, so text in an assistant message cannot open local files or
   other applications.
4. **Browser unchanged.** The web app keeps `target="_blank"`; nothing
   changes in `make dev` or a browser.
5. **Docs.** `docs/guides/development/local-run.md` "The desktop app": one
   bullet on how links open.

Keep the decision (what is opened, what is allowed in-window, what is
refused) in a pure function with unit tests; the Tauri handlers only call
it and act on the result.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Unit tests for the link decision: the app's own origin navigates in
      the window; other `http`/`https` URLs are opened externally;
      other schemes are refused.
- [x] `make check` is green.

### Manual / on-hardware (verified by a human before merge)

- [ ] macOS app: clicking a link in an assistant message and the
      navigator's pull-request link opens it in the default browser;
      the Delta window stays on Delta.

## Out of scope

- Opening local file paths shown in messages.
- Any change to how links are rendered or detected in the web app.
