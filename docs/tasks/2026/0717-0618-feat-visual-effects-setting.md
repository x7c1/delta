---
status: completed
pipeline_phase: null
plan: null
base_ref: null
retries_remaining: 1
check_command: "make check && make e2e"
assignee: null
branch: task/0717-0618-feat-visual-effects-setting
created_at: 2026-07-17T06:18:59Z
updated_at: 2026-07-17T08:05:00Z
---

# feat(web): add a visual-effects setting with a platform-aware default (flat on Linux WebKit)

## Overview

2026-07-17 Epiphany (WebKitGTK) dogfooding root-caused two "everything feels a
beat late" symptoms to decorative rendering that WebKitGTK pays full price for
while Chromium hides it:

- **Card shadows** (`shadow-md` on every session card and message card) are
  re-rasterized every time their pixels repaint — a timeline jump is an
  instant long scroll, i.e. a full-viewport repaint including every visible
  card's shadow blur.
- **The timeline landing wash** (`.delta-timeline-jump-highlight`'s `::after`
  overlay, `frontend/packages/apps/web/src/index.css:433-461`) animates
  opacity over 450 ms on an `inset: 0` overlay sitting at `z-index: -1`
  behind the message content. WebKitGTK's layer-promotion heuristics decline
  to composite this shape, so the landing repaints a potentially
  screen-filling article for ~27 frames.

An in-browser A/B on the affected machine (injecting
`* { box-shadow: none !important; animation: none !important; }`) made both
session-list hover response and timeline jumps visibly lighter, while
headless measurements show the main thread (style recalc / React / layout) is
NOT the bottleneck — the cost is all raster/paint. macOS WebKit (Safari) does
not exhibit the problem. Shadows also carry real UX value on the light and
sepia themes, so removing them outright is wrong: make the rich rendering a
user setting with a platform-aware default.

Implement a three-way setting `visualEffects: 'auto' | 'on' | 'off'`:

- **Store**: extend `useSettingsStore`
  (`frontend/packages/apps/web/src/store/settingsStore.ts`) with the
  persisted field, defaulting to `'auto'`; on rehydration an unknown value
  falls back to `'auto'` (same pattern as `activeCategory` there and
  `newSessionTab` in `composerStore.ts`).
- **Resolution**: a pure resolver maps the setting plus the environment to an
  effective `'rich' | 'flat'`:
  - `'on'` → `rich`, `'off'` → `flat`, regardless of platform.
  - `'auto'` → `flat` only on Linux WebKit: the UA is WebKit-engined
    (`AppleWebKit` token, no `Chrome/`/`Chromium/`/`Edg/` token — every
    Chromium-family browser also carries `AppleWebKit`) AND the platform is
    Linux. Everything else (macOS Safari/WKWebView, all Chromium-family,
    Firefox) → `rich`. Keep the resolver a pure function of
    `(setting, userAgent, platform)` so it is unit-testable without DOM
    globals.
- **Stamping**: mirror how the theme reaches CSS. The theme provider
  (`frontend/packages/apps/web/src/hooks/themeContext.tsx`, wired in
  `App.tsx`) stamps `<html data-theme="...">`; stamp
  `<html data-effects="rich" | "flat">` the same way, updating live when the
  setting changes (no reload required).
- **CSS**: gate the two decorative costs on the stamp, in
  `frontend/packages/apps/web/src/index.css`:
  - Card shadows: the session card
    (`frontend/packages/apps/web/src/features/navigator/SessionNode.tsx`,
    the card `div`'s `shadow-md`) and the message card
    (`frontend/packages/apps/web/src/features/transcript/TranscriptPane.tsx:91`,
    the shared card class constant) must render with no box-shadow under
    `data-effects="flat"` and unchanged under `"rich"`. Prefer routing these
    two call sites through a CSS custom property (e.g.
    `--delta-card-shadow`, consumed via an arbitrary-value utility such as
    `shadow-[var(--delta-card-shadow)]`, with the variable defined on
    `:root` and overridden under `[data-effects='flat']`) over a blanket
    `.shadow-md { box-shadow: none }` kill: functional overlay shadows
    (dropdown menus, tooltips, dialogs) must NOT be gated.
  - Landing wash: under `[data-effects='flat']`, the
    `.delta-timeline-jump-highlight` `::after` overlay must not render —
    reuse the existing `prefers-reduced-motion` block's approach
    (`index.css:463-470`, `display: none`).
- **Settings UI**: add the control to the existing `appearance` category in
  `SettingsView.tsx` (registry at
  `frontend/packages/apps/web/src/features/settings/SettingsView.tsx`,
  around line 56): a three-option radio group — "Auto (platform default)" /
  "On" / "Off" — with a one-line description of what it controls. Follow the
  existing radio-group idiom in the settings dialog.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `settingsStore` persists `visualEffects` with default `'auto'`, and an
      invalid persisted value falls back to `'auto'` on rehydration (unit
      test).
- [x] The pure resolver returns: `flat` for `('auto', <Epiphany/WebKitGTK
      Linux UA>)`; `rich` for `('auto', <macOS Safari UA>)`, `('auto',
      <Chrome-on-Linux UA>)`, and `('auto', <Firefox-on-Linux UA>)`; and the
      explicit `'on'` / `'off'` settings win over the platform on every UA
      above (unit tests with real UA strings).
- [x] The document root carries `data-effects` reflecting the effective
      value, and changing the setting updates the attribute without a reload
      (test).
- [x] Under `data-effects="flat"`: the session card and message card compute
      `box-shadow: none`, and the timeline landing-wash overlay does not
      render; under `data-effects="rich"` both are unchanged from today
      (component/e2e assertions via computed style).
- [x] Dropdown-menu / tooltip / dialog shadows are NOT affected by
      `data-effects="flat"` (assertion on at least one such surface).
- [x] The Appearance settings category shows the three-way control, reflects
      the stored value, and writes the store on change (component test).

### Manual / on-hardware (verified by a human before merge)

- [ ] On Epiphany (Linux WebKitGTK) with default settings (`auto`): no card
      shadows, no landing flash; session-list hover and timeline jumps feel
      responsive.
- [ ] Switching the setting to `On` in Epiphany restores shadows and the
      landing flash immediately; the choice survives a reload.
- [ ] On Chromium with default settings: visuals unchanged from today
      (shadows and landing flash present).

## Out of scope

- Removing the hover `transition-colors` from the session card — separate
  stacked task `0717-0620-perf-session-hover-transition.md`.
- Per-theme shadow defaults (e.g. dark theme dropping shadows while rich is
  on): the CSS-variable structure makes this a later one-liner, but do not
  implement it here.
- Gating any functional overlay shadow (menus, tooltips, dialogs), the
  spinner animation, or `transition-*` utilities.
- Backend changes of any kind.
