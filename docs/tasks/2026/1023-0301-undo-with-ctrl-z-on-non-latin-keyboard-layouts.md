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
branch: task/1023-0301-undo-with-ctrl-z-on-non-latin-keyboard-layouts
created_at: 2026-10-22T18:01:38Z
updated_at: 2026-10-22T18:15:19Z
---

# fix(web): undo with Ctrl-Z on Linux under keyboard layouts without a Latin Z

## Overview

In the Linux desktop shell, Ctrl-Z / Ctrl-Shift-Z in a text field run
WebKit's undo / redo (`frontend/packages/apps/web/src/undoKeys.ts`).
`commandFor` matches the key press by `event.key.toLowerCase() === 'z'`, so
the keys do nothing under a layout whose key produces a non-Latin character
(Russian, Greek, Hebrew, Arabic, …): `event.key` is that character, not `z`.

Fix `commandFor` so the shortcut works under those layouts as well, the way
editors and browsers resolve shortcuts:

- When `event.key` is a single Latin letter, it decides, as today. This keeps
  layouts that place Z elsewhere (Dvorak, AZERTY, QWERTZ) undoing on the key
  labelled Z, not on the physical QWERTY Z position.
- Otherwise fall back to the physical key, `event.code === 'KeyZ'`.

Keep the rest of the guards as they are (IME composition, modifiers,
editable target, terminal excluded). Update the module and function docs to
state how the key is matched and why.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A test in `undoKeys.test.ts` asserts Ctrl-Z runs undo and Ctrl-Shift-Z
      runs redo when `event.key` is a non-Latin character and `event.code`
      is `KeyZ`.
- [x] A test asserts a Latin `event.key` decides over `event.code`: a key
      press with `key: 'z'` and a different `code` runs undo, and one with a
      Latin key other than `z` and `code: 'KeyZ'` does not.
- [x] The existing undo-key and typing-step tests still pass.

### Before merge (verified outside the check command)

- [ ] On the Linux desktop app, with a non-Latin layout active (for example
      Russian), Ctrl-Z in the composer undoes the last edit (needs a person
      to switch the layout).
