---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -rq 'truncate text-fg' frontend/packages/apps/web/src/features/composer/"
assignee: null
branch: task/0916-2033-feat-show-a-pending-send-full-text-in-the-strip
created_at: 2026-09-16T20:33:00Z
updated_at: 2026-09-16T22:49:46Z
---

# feat(composer): show a pending send's full text in the strip

## Overview

The pending strip above the composer
(`frontend/packages/apps/web/src/features/composer/PendingQueue.tsx`) lists
the sends the server still holds — `queued` (waiting for the turn to end),
`dispatched` (typed into the pane, awaiting its echo), held, and the failed
ones with their reason. Every row renders the send's text through the same
span:

```tsx
<span className="min-w-0 flex-1 truncate text-fg">{text}</span>
```

`truncate` clips the text to one line with an ellipsis (`sendRow`, line 153
today, and the same span in `outcomeRow`, line 135). A queued prompt is
usually several lines long, so the row shows its first few words and
nothing else. There is no way to read the rest: the strip is the only place
a queued send appears until it dispatches, and the text cannot be selected
past the clip.

That matters because a queued send has no edit. #126 added Cancel, and the
roadmap once listed "edit a queued send" as the next step. The decision
(2026-09-17) is that no edit UI is needed: if the full text is visible, the
user can copy it, cancel the send, fix the text in the composer and send
again. Showing the text is the whole feature.

### Change

- Render the text in full in both `sendRow` and `outcomeRow`: replace
  `truncate` with `whitespace-pre-wrap break-words`, so line breaks the user
  typed survive and long unbroken tokens wrap instead of overflowing.
- Cap the height so a very long prompt cannot bury the transcript: `max-h-40
  overflow-y-auto` on the text span. 10rem is the composer's own growth cap
  (`COMPOSER_MAX_HEIGHT` in `autoGrow.ts`), so the copy of a prompt that
  appears after Send never stands taller than the box it was typed in; the
  whole text stays reachable by scrolling and selecting inside the span.
  (Decided during refine on 2026-09-17; the first draft said `max-h-64`.)
- The `local` row — a send already echoed into the transcript whose turn is
  still running — keeps a one-line clamp (`line-clamp-1`, not `truncate`):
  its full text is already on screen in the transcript directly above, so
  showing it again only shrinks the reply area. Every other row (`queued`,
  `dispatched`, held, failed) shows the full text. (Decided during refine on
  2026-09-17; the first draft said every row.)
- Put a hairline divider between rows (`divide-y` with the app's default
  border token on the `<ul>`), so two multi-line prompts stacked in the
  strip read as two entries rather than one block. (Decided during refine on
  2026-09-17.)
- The row is a flex container with `items-center`; with multi-line text the
  status label / Cancel button should sit at the top of the row rather than
  float mid-text. Switch the row to `items-start` (and align the status
  element accordingly) unless the result visibly regresses the one-line
  case — check both in the vitest DOM and by reasoning about the layout.
- Tests: in `PendingQueue.test.tsx`, add a case that renders a queued send
  whose text spans three lines and asserts the rendered span's
  `textContent` is the full text with its newlines intact, and that the
  span no longer carries `truncate` (jsdom has no layout, so the class is
  the only handle on the clipping mechanism). Keep the existing cases, which
  query rows by `pending-item` and labels by text, unchanged.

### Session-state coverage

Not applicable: no operation against a session is added or changed; the
strip renders the same rows in every state it already renders them.

### Pipeline notes

- Frontend only; run `make lint` before finishing the work phase.
- The appended grep anchors on `truncate text-fg`, the exact class pair
  both spans use today; other `truncate` uses in the composer directory
  (path pickers, template list) are unrelated and keep theirs. It was
  negative-tested at authoring time: the grep matches on `main`.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A queued send's multi-line text is rendered in full, newlines intact,
      by both the send row and the outcome row (vitest,
      `PendingQueue.test.tsx`; `! grep -rq 'truncate text-fg'
      frontend/packages/apps/web/src/features/composer/` appended to
      `check_command` pins that the clipping class is gone).
- [x] The text span caps its height and scrolls (`max-h-40 overflow-y-auto`
      on the span; vitest class assertion in the same case), and a `local`
      row's span carries `line-clamp-1` instead (vitest).
- [x] The existing strip cases — labels, Cancel, held Send/Cancel, refused
      cancel — pass unchanged.

### Manual / on-hardware (verified by a human before merge)

- [ ] In the running app, a queued prompt of ~20 lines shows in the strip
      with a scrollbar no taller than the composer's own cap, its text can be
      selected and copied, and the Cancel button sits at the top of the row.

## Out of scope

- An edit control or in-place editing for queued sends.
- Changing which rows the strip shows, or the strip's labels.
- Markdown rendering of the pending text (it is the raw prompt).
