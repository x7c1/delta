---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check'
assignee: null
branch: task/0915-0427-feat-mark-the-newest-subthread-with-the-arrow-colour
created_at: 2026-09-15T04:27:00Z
updated_at: 2026-09-15T05:04:12Z
---

# feat(navigator): mark the newest sub-thread with its arrow colour, and let titles use the row's full width

## Overview

The navigator marks the sub-thread where the last message landed by giving
its row `font-semibold`
(`frontend/packages/apps/web/src/features/navigator/ThreadTree.tsx`, the
`isNewest && 'font-semibold'` entry in the row's `cn(...)`). Dogfooding
showed the weight step is too faint to pick out: in a list of short CJK and
ASCII titles at the navigator's size, semibold next to normal is hard to
tell apart at a glance, so the mark fails at the one job it has.

Move the mark from the row's weight to the row's branch arrow. Every
sub-thread row starts with a `⤷ ` glyph drawn in `text-fg-subtle` (the
`<span className="text-fg-subtle">⤷ </span>` inside the title span). On the
marked row, draw that glyph in the ordinary foreground colour `text-fg`
instead — the colour the row's title already has, near-white on the dark
theme where the subtle arrow is grey — and drop `font-semibold` from the row
entirely. The `sr-only` "most recent activity" label stays with the row and
trails the title text: placing it inside the arrow span reorders the button's
accessible name to `⤷ most recent activity <title>` and breaks the
`getByRole('button', { name: /⤷ delta etymology/ })` locator in
`e2e/timeline-playhead-follow.spec.ts`, whose fixture row is the marked one.

A second, unrelated defect in the same row rides along: long titles are
truncated about one CJK character earlier than the row allows. The row is a
`flex … justify-between gap-2 … pr-2` button with two children — the title
span and a trailing `flex shrink-0 … gap-1.5` span that holds the spinner,
the unread badge, and the `sr-only` label. That trailing span is rendered on
every row even when it has nothing to show, and a zero-width flex item still
costs the `gap-2` before it. Measured in the running app at the navigator's
14px size: the truncated title ends at x=254 while the button's right edge
is x=270, i.e. 8px of `gap-2` for an empty span plus the 8px `pr-2`. The
`pr-2` is the intended edge padding; the 8px for the empty span is the gap
the user sees. Stop paying it: render the trailing span only when it has
something visible to hold.

### Design

1. **Rendering (`ThreadTree.tsx`, `ThreadTreeNode`).** Remove the
   `isNewest && 'font-semibold'` entry from the row's `cn(...)`, along with
   the comment block above it that explains the weight choice. Give the arrow
   span a `cn('text-fg-subtle', isNewest && 'text-fg')` class (or an
   equivalent `isNewest ? 'text-fg' : 'text-fg-subtle'`), so `cn` resolves
   the colour conflict in favour of the mark. Write a short comment in the
   arrow's place saying why the mark is the arrow's colour: it is a separate
   element from the title, so it neither competes with the active row's
   `text-accent` on the title nor disappears when the two signals land on the
   same row. Keep the existing `sr-only` "most recent activity" label; its
   rationale (a visual-only mark needs a text twin) is unchanged, only reword
   it from "weight" to "colour", and state why it trails the title (see the
   Overview).
2. **Active row.** The active row keeps `bg-accent/10 font-medium text-accent`
   untouched. The arrow's `text-fg` is explicit, not inherited, so on a row
   that is both active and newest the arrow is `text-fg` while the title is
   `text-accent` — the mark stays visible there, which is the property the
   weight-based mark bought with `font-semibold` over `font-medium`.
3. **Tests (`ThreadTree.test.tsx`, `describe('the most-recently-active mark')`).**
   Update the existing cases rather than adding parallel ones. Replace the
   `font-semibold` assertions with assertions on the arrow element: locate it
   as the first child span of the row's title span (or give it a
   `data-testid="thread-arrow"` if the test needs a stable hook — a testid is
   acceptable here since the arrow is already a dedicated element). Assert
   that exactly one arrow carries `text-fg`, that it is on the newest row,
   that every other arrow carries `text-fg-subtle`, and that no row carries
   `font-semibold` any more. Keep the tests that no row is marked when main is
   newest or when nothing has activity, that the row order is unchanged, that
   the mark coexists with the spinner and the active styling, and that a live
   event moves the mark — each now reading the arrow colour instead of the
   weight. Rename test titles that say "with weight only".
4. **Trailing span (`ThreadTree.tsx`, `ThreadTreeNode`).** Move the
   `sr-only` "most recent activity" label out of the trailing span to the end
   of the title span (right after the display name), so the trailing span's
   only contents are the two visible signals and the accessible name keeps
   its `⤷ <title>` opening. Then render the trailing span only when `running` or the
   badge condition (`unread > 0 && !isActive && !running`) holds; when neither
   does, render nothing there, so the title span is the button's last flex
   item and `gap-2` no longer applies. Keep `justify-between`, `gap-2`,
   `pr-2`, and the trailing span's own classes as they are, so a row that
   does show a signal is laid out exactly as before. Add a one-line comment
   on the conditional saying why the span is conditional (an empty flex item
   still costs the gap, which truncated the title early). Check whether any
   existing test locates the spinner or badge through the trailing span's
   position (e.g. `children[1]`) rather than by testid or text, and update
   such lookups.
5. **Do not touch** `newestThreadId`, the live `threadActivity` slice,
   `applySessionEvent.ts`, the backend, or the session list. Which row is
   marked is unchanged; only how it is drawn changes.

### Session-state coverage

No user operation is added or changed; this redraws an existing derived
mark. The states that matter — marked row also active, marked row also
running, main newest (no mark), no activity (no mark) — are already covered
by the tests named above and stay covered after the rewrite.

### Pipeline notes

- TypeScript only, inside `frontend/packages/apps/web`. `make check` is the
  canonical gate and takes over ten minutes; run it through the driver's
  long-running path.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Exactly one row's arrow carries `text-fg`, on the sub-thread with the
      newest activity, and every other row's arrow carries `text-fg-subtle`
      (vitest, `ThreadTree.test.tsx`).
- [x] No row in the tree carries `font-semibold`, whichever row is marked
      (vitest, `ThreadTree.test.tsx`).
- [x] The marked row still carries the `sr-only` "most recent activity" label
      and no other row does (vitest, `ThreadTree.test.tsx`).
- [x] No arrow carries `text-fg` when the main thread is the newest, and none
      when no thread has any activity (vitest, `ThreadTree.test.tsx`).
- [x] A row that is both active and newest keeps `text-accent` on the row and
      `text-fg` on its arrow; a row that is both running and newest keeps its
      spinner (vitest, `ThreadTree.test.tsx`).
- [x] Row order is unchanged regardless of which row is marked (vitest,
      `ThreadTree.test.tsx`).
- [x] A live session event still moves the mark, now read from the arrow
      colour (vitest, `ThreadTree.test.tsx`).
- [x] A row that is neither running nor carrying a visible unread badge has
      the title span as the button's last child, and a row that is running or
      badged still has the trailing span with its spinner or badge (vitest,
      `ThreadTree.test.tsx`).
- [x] The `sr-only` "most recent activity" label is inside the marked row's
      title span, after the display name, so the row's accessible name still
      opens with `⤷ <title>` (vitest, `ThreadTree.test.tsx`).

### Manual / on-hardware (verified by a human before merge)

- [ ] On the dark theme, the arrow of the sub-thread with the newest activity
      is visibly white against the grey arrows of its siblings, and the row's
      title weight matches its siblings.
- [ ] Selecting the marked sub-thread shows the accent title with a
      foreground-coloured arrow, and sending a prompt there keeps the arrow
      marked.
- [ ] A long sub-thread title with no spinner and no badge now runs 8px
      further right before its ellipsis (up to the row's `pr-2` edge), and a
      row showing a spinner or badge looks exactly as before.

## Out of scope

- Changing which thread counts as newest, the live update path, or the
  backend `last_activity_at` column.
- Any change to the unread badge or spinner themselves, the active row
  styling, the row's `pr-2` edge padding, or the row order.
- The light and sepia themes get the same class change; no per-theme tuning.
