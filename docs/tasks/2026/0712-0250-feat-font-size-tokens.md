---
status: completed
pipeline_phase: null
plan: null
base_ref: null
blocked_by: []
subagent_type: general-purpose
retries_remaining: 1
check_command: "make check && ! grep -rnE 'text-(xs|sm|base|lg|xl|\\[)' frontend/packages/apps/web/src frontend/packages/ui/ui-kit/src --include='*.tsx' && ! grep -rnE 'fontSize: *[0-9]' frontend/packages/apps/web/src frontend/packages/ui/ui-kit/src"
assignee: null
branch: task/0712-0250-feat-font-size-tokens
created_at: 2026-07-12T02:50:00Z
updated_at: 2026-07-12T03:44:29Z
---

# feat(web): tokenize font sizes and raise the type scale

## Overview

The web UI renders too small at default browser zoom. The transcript body —
the app's primary long-form reading surface — is 14px, most chrome is 12px,
and a dozen call sites go down to 10.4px, below the ~12px legibility floor.
Concretely, in `frontend/packages/apps/web/src`:

- No base font-size is set on `html`/`body` (`index.css`), so the root is the
  browser default 16px — but nothing uses it: `text-xs` (12px) appears 73
  times, `text-sm` (14px) 31 times, and `text-base` (16px) zero times.
- Arbitrary sizes `text-[0.65rem]` (≈10.4px) and `text-[0.7rem]` (≈11.2px)
  appear 12 times, mainly in `features/transcript/ThreadTimelineOverlay.tsx`
  and `features/transcript/MessageItem.tsx`, plus one `text-[13px]`.
- Message bodies inherit `text-sm` from their `article` wrappers
  (`MessageItem.tsx`, `TranscriptPane.tsx`), so assistant Markdown reads at
  14px; the composer textarea (`Composer.tsx`) is also `text-sm`.
- The terminal hardcodes `fontSize: 13` as a JavaScript literal
  (`features/terminal/TerminalPane.tsx`), outside the design-token system.

The project already has a design-token layer for colors and the terminal font
stack: CSS custom properties in `index.css` `:root`, Tailwind utilities wired
in `tailwind.config.js` (`theme.extend`), and `src/theme.ts` reading tokens at
runtime for consumers Tailwind cannot reach (xterm). Font size is the one
visual dimension not in that system. Fix the root cause by adding semantic
font-size tokens and reclassifying every call site, raising the scale one
step in the process:

1. Define `--delta-text-*` tokens in `index.css` `:root` and expose them as
   Tailwind `fontSize` utilities in `tailwind.config.js` (with paired
   line-heights). Suggested scale — adjust values during implementation if
   visual checks argue otherwise, but keep 12px as the hard minimum:
   - `body`: 1rem (16px) — transcript message bodies, composer input
   - `secondary`: 0.875rem (14px) — tool results, supporting prose
   - `caption`: 0.75rem (12px) — timestamps, badges, labels, timeline
     chrome; this replaces and retires the 0.65rem / 0.7rem call sites
   - `terminal`: 0.875rem (14px) — the xterm canvas
2. Replace every raw size utility (`text-xs`, `text-sm`, `text-[…]`) in
   `frontend/packages/apps/web/src` and the shared UI kit
   `frontend/packages/ui/ui-kit/src` (11 sites, including a 0.65rem badge —
   its components render throughout the app, so leaving them out produces a
   visibly mixed scale) with a semantic utility, choosing by the text's role,
   not by mechanically mapping old size to nearest new size.
3. Route the terminal size through the existing runtime-token pattern: read
   the token in `src/theme.ts` (convert rem to the px number xterm expects)
   and pass it to the `Terminal` constructor in `TerminalPane.tsx`, replacing
   the `fontSize: 13` literal. Follow the precedent of `terminalFontFamily()`
   / `terminalBackground()`.

The Markdown stylesheet (`.markdown-body` in `index.css`) is already
em-based (h1 = 1.4em, inline code = 0.875em), so it follows the body token
automatically — do not restate sizes there.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Semantic font-size tokens (`--delta-text-body`, `--delta-text-secondary`,
      `--delta-text-caption`, `--delta-text-terminal`) are defined in
      `index.css` `:root` and exposed as Tailwind utilities in
      `tailwind.config.js`, and the workspace builds and type-checks
      (`make check`).
- [x] No raw Tailwind size utilities remain in the app or the shared UI kit:
      `grep -rnE 'text-(xs|sm|base|lg|xl|\[)' frontend/packages/apps/web/src
      frontend/packages/ui/ui-kit/src --include='*.tsx'` returns no matches
      (appended to `check_command` as a gate).
- [x] No hardcoded pixel font size remains in component code:
      `grep -rnE 'fontSize: *[0-9]' frontend/packages/apps/web/src
      frontend/packages/ui/ui-kit/src` returns no matches (appended to
      `check_command`); the xterm size is read from the token via `theme.ts`
      instead.
- [x] Existing tests still pass with the new sizes (`make check` runs the
      frontend suite; update any test that asserts on the replaced class
      names).

### Manual / on-hardware (verified by a human before merge)

- [ ] At 100% browser zoom, transcript message bodies and the composer read
      at 16px and the UI is comfortably legible without zooming — visual
      judgement during dogfooding.
- [ ] No text anywhere in the UI renders below 12px (spot-check the thread
      timeline overlay and message metadata, the previous 0.65rem sites).
- [ ] The terminal pane, composer overlay, thread timeline, and badges show
      no layout breakage (clipping, overflow, misaligned rows) at the new
      sizes, in both light and dark themes.

## Out of scope

- A user-facing font-size preference. The token layer this task introduces is
  the enabler; the setting itself is a separate feature.
- Changing the root (`html`) font-size or any rem-based spacing tokens —
  text size must move independently of layout density.
- The Markdown heading scale in `.markdown-body` (already em-based and
  correct relative to the body size).
