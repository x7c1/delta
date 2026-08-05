---
status: completed
pipeline_phase: null
plan: null
base_ref: null
retries_remaining: 1
check_command: "make check && make e2e"
assignee: null
branch: task/0805-1200-feat-provider-icon-session-meta
created_at: 2026-08-05T12:00:41Z
updated_at: 2026-08-05T14:16:00Z
---

# feat(web): replace the session-card provider badge with a monochrome brand icon in the meta line

## Overview

Dogfooding feedback on the navigator session list: the `ProviderBadge` pill
(`CL`/`CX` monogram in the provider accent hue) sits at the head of every
session card's first line
(`frontend/packages/apps/web/src/features/navigator/SessionNode.tsx:304-310`)
and costs too much of the card's prime horizontal space, while the colored
chip is visually louder than a session-identity attribute deserves. The
requirement it serves is unchanged — every session must remain identifiable
as Claude Code or Codex at a glance, for users of any provider mix — so the
marker must stay per-row; it just needs to be smaller and quieter.

Demote the provider marker from line 1 to the card's meta line (line 2:
launch-time repo label on the left, last-activity time on the right,
`SessionNode.tsx:366-395`, styled `text-caption text-fg-subtle`), and render
it as a small monochrome brand icon instead of a colored pill:

- **Icon source**: add the npm package `@lobehub/icons-static-svg` (MIT) as a
  frontend dependency, **pinned to the exact version `1.94.0`** (no `^`
  range): the package was security-audited at that version (SVG-only, zero
  dependencies, zero install scripts, all 903 SVGs scanned for active
  content) and carries no npm provenance attestation, so the pin plus the
  pnpm lockfile integrity hash is what guarantees the audited bytes are what
  installs. Use its monochrome marks: `icons/claude.svg` for
  the `claude` provider (the widely recognized Claude spark; the package also
  ships `claudecode.svg` but the spark is the more recognizable mark at
  ~14px) and `icons/codex.svg` for `codex`. Both are `fill="currentColor"`,
  `width/height="1em"`, `viewBox="0 0 24 24"` — designed to inherit the
  surrounding text color and font size. Do NOT copy the SVG files into the
  repository; import them from the package so licensing provenance stays with
  the dependency. The marks are used nominatively — to state which product a
  session runs on — with no color, no endorsement implied.
- **New ui-kit component `ProviderIcon`**: a sibling of `ProviderBadge`
  (`frontend/packages/ui/ui-kit/src/ProviderBadge.tsx`), living in ui-kit for
  the same reason (shared, domain-agnostic). Reuse the local
  `Provider = 'claude' | 'codex'` union exported there so ui-kit keeps its
  no-wire-dependency rule (dependency-cruiser enforced). The component must
  render the SVG so that `currentColor` inheritance actually works — an
  `<img src>` would not inherit it. Prefer a CSS `mask-image` (SVG as the
  mask URL, `background-color: currentColor` on the element): browsers treat
  a masked SVG as an image and never execute any script it might contain, so
  a hypothetically compromised future icon version stays inert
  (defense-in-depth on top of the version pin). Fall back to a `?raw` inline
  import only if masking proves unworkable (e.g. in the test environment) —
  and strip or suppress the SVG's embedded `<title>` there if it would fight
  the wrapper's tooltip. The wrapper carries
  `title` and `aria-label` with the full product name ("Claude Code" /
  "Codex") exactly as `ProviderBadge` does today, with the glyph itself
  `aria-hidden`.
- **Placement**: at the far right end of the meta line, after the
  last-activity time (placement iterated during on-hardware review:
  before-the-time → leading the line → far right, the leading position
  having visually columned with line 1's status dot); `shrink-0`, sized
  slightly below the caption (`0.85em`, revised down from `1em` in the same
  review) with a tight 4px gap to the time and an 8px minimum from the
  truncating repo label, inheriting `text-fg-subtle`, nudged down `0.125em`
  so the baseline-aligned square sits on the text's optical middle. Line 1 drops the badge entirely: StatusDot, branch name,
  spinner/unread/permission markers only. Move the `data-testid` to the new
  element as `session-provider-icon` (the old `session-provider-badge`
  testid disappears with the badge).
- **`ProviderBadge` stays**: the provider selector
  (`features/composer/ProviderSelector.tsx`), Settings' default-provider
  picker, and the launch-option form keep the existing pill — those surfaces
  have room and benefit from the louder treatment. Only the session card
  changes. Keep `ProviderBadge` exported from ui-kit.

Frontend-only; no wire or backend change of any kind.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The session card's first line no longer renders `ProviderBadge`, and
      the meta line renders `ProviderIcon` for both a Claude and a Codex
      session (component tests in `SessionNode.test.tsx`, replacing the two
      assertions on `session-provider-badge` at lines 365 and 379).
- [x] `ProviderIcon` renders the Claude mark for `claude` and the Codex mark
      for `codex`, each with accessible name "Claude Code" / "Codex" and an
      `aria-hidden` glyph (ui-kit component test, mirroring
      `ProviderBadge.test.tsx`).
- [x] The rendered icon carries no provider accent classes
      (`text-provider-*` / `bg-provider-*`) and no hardcoded fill — it
      inherits `currentColor` (structural assertion on the rendered markup).
- [x] `ProviderSelector` and the Settings default-provider picker still
      render `ProviderBadge` unchanged (their existing tests keep passing
      without modification).
- [x] Playwright e2e: the session list shows the provider icon in the meta
      line for the Codex mock session and a Claude session
      (`session-provider-icon` locator); no spec still references
      `session-provider-badge`.
- [x] `@lobehub/icons-static-svg` is declared as exactly `1.94.0` (no range
      prefix) in the consuming package's `package.json`, and the pnpm
      lockfile records its integrity hash.
- [x] `make check` passes, including dependency-cruiser (ui-kit gains no
      dependency on wire/gateway packages).

### Manual / on-hardware (verified by a human before merge)

- [ ] In the running app, session cards read: line 1 = status dot + branch
      (+ activity markers), line 2 = repo … time + icon; the icon is quiet
      (subtle foreground tone, no colored chip), vertically aligned with the
      neighboring text, and the two marks are distinguishable at a glance.
- [ ] The icon tooltip shows the full product name, and the glyph renders
      correctly on light, dark, and sepia themes.

## Out of scope

- Redesigning `ProviderBadge` or its other call sites (provider selector,
  Settings picker, launch-option form).
- Colored icon variants or any provider accent hue in the session list.
- A third provider; the icon map extends the same way `PROVIDER_METADATA`
  (`frontend/packages/apps/web/src/providers.ts`) does.
- Backend or wire changes.
