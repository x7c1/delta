---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -rq 'appearance-none' frontend/packages/apps/web/src/features/new-session/tabs/ && [ \"$(grep -c '^  color-scheme:' frontend/packages/apps/web/src/index.css)\" -ge 3 ]"
assignee: null
branch: task/1001-0024-fix-render-the-clone-root-select-in-the-page-theme-under-webkit
created_at: 2026-09-30T15:24:12Z
updated_at: 2026-09-30T16:02:00Z
---

# fix(web): render the clone-root select in the page theme under WebKit

## Overview

In the desktop app on Linux (WebKitGTK), the "Clone root" dropdown on the
new-session PR tab is close to unreadable in the dark theme: its text is
painted in a colour that nearly matches the dark background. The same
control looks right in Chrome and Firefox. Verified on a Linux machine
during the v0.5 release check (2026-10-01); the macOS app is WebKit too
(WKWebView), so it is expected to show the same defect.

The control is the only `<select>` in the web app
(`frontend/packages/apps/web/src/features/new-session/tabs/PRTab.tsx`,
the `pr-tab-clone-root-select` element). It sets `text-fg` and
`bg-surface` (plus a border) and relies on the browser dropping the native
menulist rendering once an author background or border is set. Chromium
and Gecko do that; WebKit keeps `appearance: menulist` and paints the
closed control's text with the platform theme's colour, so the token
colours never reach the text.

A second cause is independent of the select: the app switches themes by
toggling `<html data-theme="…">` and the `:root[data-theme='…']` token
blocks in `frontend/packages/apps/web/src/index.css`, but nothing sets the
CSS `color-scheme` property. Without it, WebKit renders every native
control — the select's popup list, the checkboxes and radios in
`features/composer/LaunchOptionsPicker.tsx` and
`features/composer/WorktreeOptions.tsx`, scrollbars — with the light
scheme's control colours, whatever the page's theme.

### Change

1. **Own the select's rendering.** Give the select `appearance-none` so
   the closed control is drawn from the page's tokens in every engine,
   and draw the dropdown indicator yourself (an inline SVG chevron
   positioned at the right edge, in the app's existing inline-SVG style —
   see `features/composer/WorkdirPickerBody.tsx`; reserve right padding so
   the value never runs under it). Keep the `data-testid`, the `value` /
   `onChange` wiring and the option list as they are.
2. **Declare the colour scheme per theme.** In `index.css`, add
   `color-scheme: dark` to `:root[data-theme='dark']` and
   `color-scheme: light` to `:root[data-theme='light']` and
   `:root[data-theme='sepia']` (sepia is a light scheme). If the fallback
   `:root` block mirrors the light theme's tokens, give it
   `color-scheme: light` as well so an unstyled first paint is
   consistent. Keep the theme-authoring comment in `index.css` (the list
   of what a new theme block must define) in step with this addition.

Out of scope: restyling the checkboxes and radios themselves (they take
the scheme from step 2 and are otherwise fine), and any change to the
clone flow's behaviour.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The clone-root select in `features/new-session/tabs/` carries
      `appearance-none` and a custom dropdown indicator
      (`grep -rq 'appearance-none' frontend/packages/apps/web/src/features/new-session/tabs/`
      is appended to `check_command`).
- [x] Each of the three theme blocks in `index.css` declares
      `color-scheme` (`grep -c '^  color-scheme:'` on the file is at least
      3, appended to `check_command`).
- [x] The PR tab's existing tests still exercise the select through
      `pr-tab-clone-root-select` and pass under `make check`.

### Manual / on-hardware (verified by a human before merge)

- [ ] In the Linux desktop app with the dark theme, the Clone root value
      and its dropdown list are readable, with text and background taken
      from the dark tokens.
- [ ] In a browser (Chrome or Firefox), the select still looks as before
      in dark and light, and the custom chevron sits inside the control
      without overlapping the value.
