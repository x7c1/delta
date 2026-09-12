---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -rq "_blank" frontend/packages/apps/web/src/features/transcript/'
assignee: null
branch: task/0912-0305-feat-open-assistant-links-in-a-new-tab
created_at: 2026-09-12T03:05:00Z
updated_at: 2026-09-12T03:37:11Z
---

# feat(transcript): open links in assistant prose in a new tab

## Overview

Assistant prose in the conversation pane is rendered by `AssistantMarkdown`
(`frontend/packages/apps/web/src/features/transcript/AssistantMarkdown.tsx`),
which passes the text through `react-markdown` 9 with `remarkGfm` and
`remarkTrimAutolinkPunctuation` and applies no `components` override. Every
anchor it produces is therefore a plain `<a href>`: clicking a link the agent
wrote navigates the Delta tab itself away to GitHub (or wherever the link
points), and the session the user was reading is replaced. Getting back means
using the browser's Back button and waiting for the app to reload and
reconnect.

Delta already treats an outbound link as something that belongs in a separate
tab: the pull-request number on a session card renders as
`<a target="_blank" rel="noopener noreferrer">`
(`frontend/packages/apps/web/src/features/navigator/SessionNode.tsx:447-459`).
Assistant prose should match it, so that no link in the conversation pane can
cost the user the screen they are on.

### Design

1. **A single anchor override in `AssistantMarkdown`.** Pass
   `components={{ a: ... }}` to `ReactMarkdown` alongside the existing
   `remarkPlugins`. The override renders an `<a>` carrying the incoming props
   plus `target="_blank"` and `rel="noopener noreferrer"`. Because both the
   persisted transcript message and the live streaming bubble render through
   this one component, the single override covers both.
2. **Do not spread react-markdown's `node` onto the DOM.** In react-markdown
   9 a `components` entry receives the mdast `node` alongside the HTML props;
   spreading it straight onto `<a>` makes React warn about an unknown DOM
   attribute. Destructure `node` away and spread only the rest.
3. **No exceptions by link shape.** Autolink literals, explicit
   `[label](url)` links, relative links, and `#fragment` anchors all get the
   same treatment: assistant prose is not expected to link into Delta's own
   UI, and a rule with no exceptions is the one a reader can predict. State
   that reasoning in the module doc comment rather than leaving the blanket
   `_blank` unexplained.
4. **Keep `rel` paired with `target`.** `noopener` is what prevents the opened
   page from reaching back through `window.opener`; it is not optional
   decoration. Assert it in the tests so a later edit cannot drop it silently.
5. **Doc comment.** The component's doc comment already explains what GFM and
   the punctuation-trimming plugin contribute. Add the new sentence in the
   same voice: links open in a new tab so the conversation is never navigated
   away from, matching the session card's pull-request link.
6. **Tests** (`AssistantMarkdown.test.tsx`, alongside the existing cases):
   - an autolink literal renders an anchor with `target="_blank"` and
     `rel="noopener noreferrer"`;
   - an explicit `[label](url)` link renders the same pair, with the author's
     `href` and label intact;
   - a relative link (`[doc](/docs/a)`) and a `#fragment` anchor also carry
     the pair, pinning the "no exceptions" rule;
   - rendering emits no React warning about an unknown `node` attribute
     (spy on `console.error` and assert it was not called), which is what
     catches a regression of design point 2.

### Session-state coverage

Not applicable: this changes how already-rendered assistant prose is
displayed and adds no operation a user can trigger against a session.

### Pipeline notes

- Frontend-only change; run `make lint` before finishing the work phase
  (dependency-cruiser must stay green — no new module is introduced).
- The appended gate `grep -rq "_blank" frontend/.../features/transcript/`
  finds nothing on `main` (`_blank` occurs only under `features/navigator/`),
  so it is a real gate rather than one that always passes. It is anchored to
  the directory, not to `AssistantMarkdown.tsx`, so a refine perspective may
  still move the override into its own module.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] An autolinked URL in assistant prose renders an anchor with
      `target="_blank"` and `rel="noopener noreferrer"`, with its `href`
      unchanged (vitest, `AssistantMarkdown.test.tsx`).
- [x] An explicit `[label](url)` link, a relative link, and a `#fragment`
      anchor each render with the same `target` / `rel` pair and the author's
      `href` and label intact (vitest, `AssistantMarkdown.test.tsx`).
- [x] Rendering assistant prose through `AssistantMarkdown` produces no
      React DOM warning on `console.error` — react-markdown's `node` prop is
      not forwarded to the anchor element (vitest,
      `AssistantMarkdown.test.tsx`).
- [x] The CJK-punctuation trimming already covered by
      `AssistantMarkdown.test.tsx` and `remarkTrimAutolinkPunctuation.test.ts`
      still holds — the anchor override composes with the remark plugin
      rather than replacing it (vitest, existing cases).
- [x] `features/transcript/` declares the new-tab attributes (grep gate in
      `check_command`, which finds no match on `main`).

## Out of scope

- Changing how links look (colour, underline, an external-link icon).
- Applying the rule outside assistant prose rendered by `AssistantMarkdown` —
  user text is rendered verbatim, and the session card's pull-request link
  already opens in a new tab.
- Any change to the URL text itself; `remarkTrimAutolinkPunctuation` owns
  that and is edited by its own task.
