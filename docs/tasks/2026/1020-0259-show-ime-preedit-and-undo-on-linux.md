---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 0
check_command: 'make check && grep -rq "set_enable_preedit(true)" backend/crates/apps/delta-desktop/src/'
assignee: null
branch: task/1020-0259-show-ime-preedit-and-undo-on-linux
created_at: 2026-10-19T17:59:41Z
updated_at: 2026-10-19T18:40:19Z
---

# fix(desktop): show the IME's uncommitted text and undo with Ctrl-Z on Linux

## Overview

Two text-input defects in the Linux desktop app (Tauri over WebKitGTK). The
macOS app has neither. Both were reproduced on Ubuntu 26.04 (Wayland,
fcitx5 + mozc, WebKitGTK 2.52.6).

**1. The IME's uncommitted (preedit) text is not shown.** While typing
Japanese, the candidate window appears and the committed text is inserted,
but the text being composed is never drawn in the textarea. A plain
textarea in WebKitGTK's own MiniBrowser on the same machine draws it, so
the page is not at fault. The cause is wry: `wry-0.57.0/src/webkitgtk/mod.rs`
(around line 421) calls `input_context.set_enable_preedit(false)`
unconditionally on every webview. It did so to work around fcitx placing
its candidate window wrongly on older webkit2gtk
(tauri-apps/tauri#5986), which reportedly no longer happens on
webkit2gtk-4.1 >= 2.50 (tauri-apps/wry#1724 proposes making it
configurable).

Fix: on Linux only, once the main window's webview exists, turn preedit
back on through the webview's input method context (for example via
`WebviewWindow::with_webview`, then the `webkit2gtk` `WebView`'s
`input_method_context()` and `set_enable_preedit(true)`). Add the
`webkit2gtk` dependency to `delta-desktop` under the Linux target at the
version Tauri already resolves (`backend/Cargo.lock` has `webkit2gtk`
2.0.2), so no second copy is built. Put it next to the other Linux-only
window setup in `backend/crates/apps/delta-desktop/src/main.rs` (see where
`quit_shortcut::install` runs), in its own module if that reads better.
Log a warning if the context cannot be reached rather than failing the
launch. Say in a comment why the call is needed (wry's unconditional
disable) so it can be removed once wry offers the option.

**2. Ctrl-Z does nothing in text fields.** WebKitGTK binds no key to undo
(also true in MiniBrowser), so the Linux app has no undo at all, while
macOS gets Cmd-Z from the default Edit menu. Fix it in the page: inside
the Linux shell only (`<html data-shell="tauri-linux">`, see
`frontend/packages/apps/web/src/shell.ts`), handle a keydown of Ctrl+Z as
`document.execCommand('undo')` and Ctrl+Shift+Z as
`document.execCommand('redo')`, calling `preventDefault()` when handled.
Install it once for the whole document, not per field, so every text
field (composer, settings forms) gets it. Handle the key only when:

- the event target is an editable text field (`textarea`, a text-like
  `input`, or a `contenteditable` element);
- the key is not part of an IME composition (`event.isComposing` is false);
- no Alt or Meta modifier is held.

It must never act while the embedded terminal (xterm.js) has focus:
Ctrl-Z there is SIGTSTP for the tmux pane. xterm.js's hidden helper
textarea is a `textarea`, so the editable-target test alone is not
enough; exclude it explicitly (it lives under the terminal's `.xterm`
container). In the browser and the macOS shell the handler is not
installed.

Undo is WebKit's own, page-wide history: undoing past one field's
edits moves into another field's. That matches the macOS app (same
engine) and is accepted here; a per-field history of the page's own is
out of scope.

WebKit makes a whole run of uninterrupted typing, Backspace included,
one undo step, so Ctrl-Z after deleting a few characters cannot bring
just those back. Split that run where an editor would: on keydown,
before WebKit applies the key, set the field's selection to where it
already is (which closes WebKit's open typing step) when the typing
switches between inserting and deleting, at a space or a line break,
after a one-second pause, in a different field, or when an input method
commits its text (`compositionend`: the keys of a composition arrive as
composing and are not counted, so without it a Backspace right after a
commit joins the committed text's step). Do this only for
fields with a text selection (`textarea`, text-like `input`), never
during IME composition or in the terminal. (Doing it on `beforeinput`
was tried and splits one key too late: WebKit has already added that
edit to the open step.)

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `delta-desktop` re-enables preedit on the Linux webview with
      `set_enable_preedit(true)` (grep gate in `check_command`), and the
      `webkit2gtk` dependency is Linux-only and resolves to the version
      already in `Cargo.lock` (no second `webkit2gtk` entry; `make check`
      builds it).
- [x] Unit tests for the undo key handler cover: Ctrl+Z on a textarea
      calls undo and prevents default; Ctrl+Shift+Z calls redo; a key
      during IME composition, a non-editable target, the terminal's
      helper textarea, and Alt/Meta combinations are left alone; outside
      the Linux shell nothing is installed.
- [x] Unit tests for the typing-step splitter cover: one run of the same
      kind stays one step; a switch between inserting and deleting, a
      space or line break, a pause, and an IME commit each start a step; IME
      composition, the terminal's helper textarea, and a `number` input
      are left alone.

### Before merge (verified outside the check command)

- [ ] In the Linux desktop app built from this branch, typing Japanese in
      the composer with fcitx5 + mozc shows the uncommitted text inline,
      and the candidate window sits at the caret — needs a person: an
      IME session typed by hand.
- [ ] In the same build, Ctrl-Z undoes typing in the composer step by
      step (typing `abc` then three Backspaces, Ctrl-Z brings `abc`
      back, and the same with Japanese `あいう`) and Ctrl-Shift-Z redoes it, and Ctrl-Z in the terminal still reaches the
      pane (suspends a foreground job) — needs a person: keyboard input in
      the running desktop app.
