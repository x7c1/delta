---
status: completed
pipeline_phase: null
plan: null
base_ref: feat/prompt-templates
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0822-1126-feat-composer-prompt-templates-button
created_at: 2026-08-22T11:26:00Z
updated_at: 2026-08-22T17:28:59Z
---

# feat(web): insert prompt templates from a button on the composer rail

## Overview

Prerequisites on `main` before running this task: the prompt-template
registry and client hooks (`usePromptTemplatesQuery`, mock handlers in
`@delta/api-mocks`) and the composer top rail (`ComposerRail`, with its
reserved left slot and the provider tabs). This task fills the rail's left
slot with a **prompt-template button** that opens a popover listing the
registered templates and inserts the chosen one into the composer draft at
the cursor, in both thread and new-session modes.

A prompt template is a named, provider-independent block of text that acts
like a reusable skill invocation across Claude Code and Codex sessions. Its
text can be long (many lines), so the popover must never reduce a template
to a one-line preview.

```
 ┌──┐ ┌────────┬────────┐
 │ ▤│ │ Claude │ Codex  │           ← ▤ = the template button (icon only), leftmost on the rail
┌┴──┴─┴────────┴────────┴──────────────┐
│ Message to start a new session…      │
```

### The button (rail, leftmost, both modes)

- Icon-only, caption-sized (`h-3.5 w-3.5` glyph inside the rail item box),
  `text-fg-subtle` → `hover:text-fg`, `aria-label="Prompt templates"`,
  `aria-haspopup="menu"`, `aria-expanded`, `data-testid="prompt-templates-button"`.
  It renders in **both** thread and new-session modes and is **always
  visible** (also with zero templates) — it is the only discoverable entry
  point. Because the thread-mode rail was allowed to be empty/zero-height in
  the rail task, this task makes the rail always render at its item height
  in both modes (the button is always there).
- The glyph is a "document with text lines" (Feather `file-text`), added
  as an inline SVG component in the existing style (`viewBox 0 0 24 24`,
  `stroke="currentColor"`, `strokeWidth={2}`, round caps/joins,
  `aria-hidden`) — see `SendIcon` in
  `frontend/packages/apps/web/src/features/composer/Composer.tsx:47-64`:

  ```
  <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
  <polyline points="14 2 14 8 20 8" />
  <line x1="16" y1="13" x2="8" y2="13" />
  <line x1="16" y1="17" x2="8" y2="17" />
  <polyline points="10 9 9 9 8 9" />
  ```

- Rail-item styling is the rail task's contract: left/top/right borders,
  `rounded-t-md`, `bg-surface`, no bottom border, no negative margin, so the
  card's top border and the context-usage fill run uninterrupted beneath.
- Keep the structure such that a later tweak can switch the glyph to
  "reveal on card hover / focus-within" (opacity only, rail height fixed)
  without restructuring — e.g. the visibility classes live on the glyph
  wrapper, not on the rail item box.

### The popover

- Opens **upward** from the button (the composer sits at the bottom of the
  screen), left-aligned to the button, layered above the composer card
  (`bottom-full` anchoring like `composer-context-popover`, or a portal with
  the same placement). Own component under `features/composer/` (e.g.
  `PromptTemplatePopover.tsx`); do not extend `@delta/ui-kit` `Menu` (its
  trigger is icon-only and its items are single-line labels), but mirror its
  Escape / click-outside / focus-restore handling
  (`frontend/packages/ui/ui-kit/src/Menu.tsx:108-150`).
- Two columns: **left** a list of template labels only (no body text in the
  list), `role="menu"` / `role="menuitem"`, first item focused on open, ↑/↓
  move, Enter or click selects, Escape closes; **right** a preview pane
  showing the **full text** of the hovered / focused item (`whitespace-pre-wrap`,
  `max-h-[50vh]`, `overflow-y-auto`, monospace-leaning or body font — match
  the Settings editor). Width large enough for prose (cap around `max-w-2xl`,
  never wider than the card).
- Footer item "Manage templates…" opens the Settings dialog on the
  `prompt-templates` category (`useNavStore` `openSettings` +
  `settingsStore.setActiveCategory('prompt-templates')`) and closes the
  popover.
- Empty registry: the list area reads "No prompt templates yet." with the
  same "Manage templates…" footer.
- Loading / error: a `Spinner` while the first load is in flight; a short
  inline error line if the query failed, with the footer still available.

### Insertion

- Selecting a template inserts its `text` into the composer textarea at the
  current selection: replace `[selectionStart, selectionEnd)` of the draft
  with the text, place the caret right after the inserted text, write the
  new draft to `composerStore` (`setDraft(draftKey, …)`), close the popover,
  and return focus to the textarea with that caret position. **No automatic
  newline or space is added** before or after the text — what was registered
  is what is inserted. Nothing is sent.
- Implement the pure string/caret computation as a small exported helper
  (e.g. `insertAtSelection(draft, selectionStart, selectionEnd, text) →
  { next, caret }`) so it is unit-tested deterministically, independent of
  the DOM; the component applies the result and sets
  `textarea.setSelectionRange(caret, caret)` after the store update
  (`requestAnimationFrame` / effect as needed so the caret is set after React
  re-renders the value).
- The caret position must be read from the textarea **at the moment the
  popover opens** (focus moves into the popover, so it must be captured
  before). If the textarea was never focused, insert at the end of the draft.
- Works identically in thread mode and new-session mode — the button and the
  insertion live in/around `Composer.tsx`, which both modes share.

### Session-state coverage

Inserting edits only the local draft; it must not depend on session state.
Per the operation × session-state rule, cover in tests: **new-session**,
**open + idle**, **open + mid-turn** (a send in flight / `sendInFlight`),
**closed** (the read-only "Send to resume this closed session…" composer),
and **resuming** (sends deferred). In every one of these the draft gains the
text and no request other than the template list fetch is issued.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `insertAtSelection` unit tests: insert at start / middle / end; replace
      a non-empty selection; empty draft; multi-line text with leading and
      trailing newlines inserted verbatim (no added separators); caret lands
      at `selectionStart + text.length`.
- [x] The rail shows the template button leftmost in both thread and
      new-session modes, before the provider tabs in new-session mode, and it
      renders even when the registry is empty (component tests).
- [x] Clicking the button opens the popover above the composer with the
      label list (no body text in the list DOM for a multi-line fixture),
      the first item focused, and the preview pane showing the full text of
      the focused item; ↑/↓ moves focus and updates the preview; Escape and
      click-outside close it and restore focus to the textarea (component
      tests).
- [x] Selecting a template with the caret in the middle of an existing draft
      yields `before + text + after` in the store, caret after the inserted
      text, focus back on the textarea, popover closed, and no send request
      (component test; assert on the api-mocks request log that no `POST
      /api/sends` or new-session send happened).
- [x] Insertion works in each session state — new-session, open + idle,
      open + mid-turn, closed (read-only composer), resuming — with the same
      draft outcome (parametrized component test over the store states).
- [x] "Manage templates…" opens Settings on the `prompt-templates` category
      and closes the popover; the empty-registry popover shows the empty
      message and the same footer (component tests).
- [x] e2e-fake spec: with the two seeded fixtures, open a thread, type a
      draft, open the popover, pick the multi-line template, assert the
      textarea value contains the full multi-line text at the caret and no
      message was sent.
- [x] `make check` passes.

### Manual / on-hardware (verified by a human before merge)

- [ ] In the running app the button sits on the card's top-left edge as a
      small tab-like box, the card border and the context-usage fill run
      cleanly beneath it, and it does not read as a second Send button or
      crowd the text (light, dark, sepia).
- [ ] Inserting a ~40-line template mid-draft reads correctly in the
      auto-grown textarea, the caret is where typing continues naturally,
      and the popover's preview was legible (scrolls, full text) before
      choosing.
- [ ] Judge whether the always-visible glyph is distracting during normal
      reading; if so, note it for the follow-up "reveal on hover/focus"
      tweak (not blocking for merge).

## Out of scope

- A keyboard shortcut to open the popover (deliberately dropped: usage
  frequency is low; mouse is enough). Popover-internal ↑/↓/Enter/Escape
  remain.
- Filter/search inside the popover, placeholders / variable expansion,
  inline `;;`-style triggers, `/` triggers (conflict with agent slash
  commands), reordering or tags.
- Any change to the Settings editor or to the API.
