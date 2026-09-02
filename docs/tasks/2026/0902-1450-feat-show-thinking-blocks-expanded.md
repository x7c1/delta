---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -q "thinking-block" frontend/packages/apps/web/src/features/transcript/MessageItem.tsx && ! grep -q "exercise the collapsible" frontend/packages/testing/api-mocks/src/fixtures.ts && ! grep -q "collapsed by default with a" frontend/packages/apps/web/src/features/transcript/MessageItem.tsx'
assignee: null
branch: task/0902-1450-feat-show-thinking-blocks-expanded
created_at: 2026-09-02T14:50:22Z
updated_at: 2026-09-02T15:24:53Z
---

# feat(transcript): always show thinking blocks instead of collapsing them

## Overview

In the transcript pane (the centre conversation view) a `thinking` content
block renders as a click-to-expand card: `MessageItem.tsx` (`case
'thinking'`, around line 221) wraps the block's text in the ui-kit
`Collapsible` with the one-line summary `thinking` from `blockSummary.ts`,
and `Collapsible` (`frontend/packages/ui/ui-kit/src/Collapsible.tsx`)
starts collapsed (`defaultOpen` defaults to `false`), so the reasoning is
hidden until the user clicks the summary row. Dogfooding showed that the
extra click is pure friction: when a thinking block has text at all — Claude
Code writes its narration-style thinking with a body and leaves only the
signature for the rest; Codex delivers its reasoning as its own message —
the user wants to read it in the flow of the conversation, every time.

Change the thinking block to be **always visible, with no collapse
affordance**. Keep everything else about its look as it is today: the same
bordered, elevated frame, the same `thinking` caption line on top, the same
`<pre class="whitespace-pre-wrap text-fg-muted">` body underneath, the same
indent and spacing inside the message. Only the interaction goes away: the
caption line is no longer a `<button>`, there is no `▸`/`▾` glyph and no
`aria-expanded`, and the body is rendered unconditionally.

### Design

1. **A static twin of `Collapsible` in ui-kit.** `Collapsible` owns its
   `useState` and always renders the toggle button, so it cannot express
   "never collapsible" cleanly (a `defaultOpen` that cannot be flipped would
   still render a button that lies). Add a small non-interactive component
   next to it — a bordered card with a caption line and an always-visible
   body; `Card` is a natural name, but any short domain-agnostic name that
   reads as the static counterpart of `Collapsible` is fine — and export it
   from `frontend/packages/ui/ui-kit/src/index.ts`. Its props mirror
   `Collapsible`'s minus `defaultOpen`: a `summary`/`label` node for the
   caption line, `className`, `children`. The caption line is a plain
   `<div>` with the same text styling as the Collapsible summary row
   (`text-caption text-fg-muted`, `px-2 py-1`), without the hover
   background, glyph, or `aria-expanded`; the body has the same
   `border-t border-border-default px-2 py-1.5 text-caption` wrapper. Share
   the frame / caption / body class strings between the two components (a
   small shared module in ui-kit is enough) so the two cards cannot drift
   apart visually. Give the new component its own vitest next to
   `Collapsible.test.tsx`: the body is in the document without any click,
   and the card contains no `button`.
2. **Use it for thinking in `MessageItem.tsx`.** Replace the `Collapsible`
   in `case 'thinking'` with the new component, keeping
   `blockSummary(block)` as the caption and the existing `<pre>` body. Put
   `data-testid="thinking-block"` on the card (pass it through a prop or
   render it on the wrapper — the grep gate in `check_command` only
   requires the string in `MessageItem.tsx`). The empty-thinking early
   return (`if (!block.thinking.trim()) return null;`) stays exactly as it
   is: an empty block still renders nothing.
3. **Nothing else changes its behaviour.** Tool calls, orphan tool results,
   `other` blocks, the `meta` message card, and `PermissionNotice`'s
   collapsibles keep using `Collapsible` and keep starting collapsed. The
   bubble rule in `toolPairs.ts` (`blockRendering`: a thinking block with
   text is an `annotation` that sits inside the prose bubble without
   earning one) is untouched — it is about layout, not about the card
   being open — so a reply carrying text and thinking keeps its bubble and
   a standalone Codex reasoning message stays bare, exactly as today.
4. **Update the prose that describes the old behaviour.** The header
   comment of `MessageItem.tsx` says "`thinking` and tool blocks are
   collapsed by default with a one-line summary" — reword it so only tool
   blocks are described as collapsed and thinking is described as an
   always-visible card. The fixture overview in
   `frontend/packages/testing/api-mocks/src/fixtures.ts` (line 24, "a tool
   call and a thinking block to exercise the collapsible blocks") is
   reworded the same way. Both rewrites are grep-gated in `check_command`.
   The `Collapsible` doc comment mentions "thinking" as an example use;
   drop that example.
5. **Tests.** `MessageItem.test.tsx` `'renders a thinking block that has
   text'` (line 238) currently asserts only the summary text — extend it to
   assert the body text is visible without any click and that the thinking
   card contains no `button`. The neighbouring empty-thinking tests must
   keep passing unchanged. Existing `Collapsible.test.tsx` and the tool /
   meta collapsible assertions in `MessageItem.test.tsx` are unaffected.

### Session-state coverage

Not applicable: this change is a passive rendering rule for persisted
transcript content and adds no operation against a session. It applies
identically whatever state the session is in.

### Pipeline notes

- Frontend-only change; run `make lint` before finishing the work phase.
- Negative-test of the appended gates at authoring time: on `main` the
  `thinking-block` grep fails and both `!` greps fail, so all three are real
  gates that the work has to satisfy.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] ui-kit exports a non-interactive card component (the static
      counterpart of `Collapsible`) with a caption line and an
      always-rendered body; its unit test shows the body is in the document
      without any click and that the card contains no `button`.
- [x] `MessageItem` renders a thinking block with text as that card,
      carrying `data-testid="thinking-block"` (grep gate in
      `check_command`): the block's text is visible without a click and the
      card has no `button` / `aria-expanded` (vitest, extending the existing
      `'renders a thinking block that has text'` test).
- [x] An empty or whitespace-only thinking block still renders nothing, and
      a message whose only block is empty thinking still renders nothing
      (the existing `MessageItem.test.tsx` cases pass unchanged).
- [x] Tool-call, orphan tool-result, `other`, and `meta` cards still start
      collapsed and still expand on click (existing `MessageItem.test.tsx`
      and `Collapsible.test.tsx` assertions pass unchanged).
- [x] The `MessageItem.tsx` header comment no longer describes thinking as
      "collapsed by default" and the `fixtures.ts` overview no longer says
      the thinking block exercises the collapsible (both grep gates in
      `check_command`).

### Manual / on-hardware (verified by a human before merge)

- [ ] On a real Delta run, a Claude Code session whose transcript carries a
      thinking block with text shows the reasoning open in the conversation
      pane without any click, under the `thinking` caption, with the same
      frame and indent as before; a Codex session's reasoning message shows
      the same way.
- [ ] In the same sessions, tool cards still start collapsed, and a reply
      that carries both text and thinking still sits inside its prose
      bubble as before.

## Out of scope

- A setting to choose between collapsed and expanded thinking; the block is
  simply always shown.
- Changing the visual design of the thinking card (colours, indent,
  typography, the `thinking` caption) beyond removing the toggle.
- The collapse behaviour of tool, `other`, `meta`, or permission cards.
- Showing thinking while a reply is still streaming; as today, only the
  persisted block is rendered.
