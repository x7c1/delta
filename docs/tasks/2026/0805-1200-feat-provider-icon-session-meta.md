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
updated_at: 2026-08-06T02:05:00Z
---

# feat(web): replace the CL/CX provider badge with hue-based provider markers

## Overview

Dogfooding feedback on the navigator session list: the `ProviderBadge` pill
(`CL`/`CX` monogram in the provider accent hue) sat at the head of every
session card's first line, costing too much of the card's prime horizontal
space, and the colored chip was visually louder than a session-identity
attribute deserves. The requirement it served is unchanged — every session
must remain identifiable as Claude Code or Codex at a glance, for users of
any provider mix — so the marker had to stay per-row; it just needed to be
smaller and quieter.

The first attempt replaced the pill with a small monochrome brand icon on
the card's meta line. On-hardware review tried it at three positions
(leading the line, mid-line, far right) and every one read as a foreign
object: a filled glyph carries more ink than the thin caption text around
it, so wherever it sat in a text line it floated. The final design drops
the inline element entirely and identifies the provider by **accent hue
alone**, applied to things the UI already has:

- **Session card**: the kebab-menu trigger's three dots rest in the
  provider hue (burnt orange for Claude Code, green for Codex) instead of
  the default subtle gray, via a new `Menu` prop `triggerClassName`
  (`frontend/packages/ui/ui-kit/src/Menu.tsx`) merged last so the
  consumer's color utility wins; hover still shifts to the interactive
  text color. The tint map in `SessionNode.tsx` is a
  `satisfies Record<…>` over the provider union, so a new wire provider
  fails to typecheck until it gets a hue. The trigger's accessible name
  gains the provider ("Session actions for … (Claude Code session)") so
  the marker never relies on color alone.
- **Pickers and rows**: a new ui-kit `ProviderName` — the full product
  name written in the provider hue — replaces `ProviderBadge` in Settings'
  default-provider picker, the launch-option form and rows, and the
  new-session provider selector. Deliberately shape-free: an interim
  hue-filled dot was tried first and read as a status indicator next to
  `StatusDot`'s open/closed vocabulary (a green provider dot beside a
  green "open" dot invites misreading), so these surfaces color the words
  instead of adding any glyph.
- **Retired**: `ProviderBadge` (the CL/CX monogram no longer appears
  anywhere a user could learn it), the interim `ProviderIcon` brand-mark
  component with its `@lobehub/icons-static-svg` dependency, and the
  interim `ProviderDot`. The `Provider` union and display names move to a
  shared ui-kit module (`provider.ts`).

Frontend-only; no wire or backend change of any kind.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The session card renders no provider badge or inline provider mark;
      the kebab trigger carries `text-provider-claude` /
      `text-provider-codex` matching the session's provider, and its
      accessible name names the provider (component tests in
      `SessionNode.test.tsx`).
- [x] `ProviderName` writes the full product name ("Claude Code" /
      "Codex") in the provider's hue class and merges a caller `className`
      (ui-kit component test).
- [x] `ProviderSelector`, the Settings default-provider picker, and the
      launch-option rows render the hue-tinted product name via
      `ProviderName` (their component tests).
- [x] Playwright e2e (`e2e/provider-marker.spec.ts`): with the real
      stylesheet, the Claude and Codex cards' kebab triggers resolve to
      different computed colors, both different from the meta line's
      resting text tone, and each trigger's accessible name carries its
      provider.
- [x] `ProviderBadge`, `ProviderIcon`, `ProviderDot`, and the
      `@lobehub/icons-static-svg` dependency are fully removed — no source
      references remain and the lockfile no longer records the package
      (`make check` compiles and lints the tree; a stale import fails it).
- [x] `make check` passes, including dependency-cruiser (ui-kit gains no
      dependency on wire/gateway packages).

### Manual / on-hardware (verified by a human before merge)

- [x] In the running app, each session card's kebab dots rest in its
      provider's hue; the two hues are distinguishable at a glance on
      light, dark, and sepia themes, and hover still gives the usual
      interactive feedback.
- [x] Settings' default-provider picker, the launch-option form and rows,
      and the new-session provider selector show the product name written
      in the provider hue, with no dot-like glyph that could be mistaken
      for the session list's open/closed status dot.

## Out of scope

- A third provider; both the trigger-tint map and `ProviderName`'s hue map
  extend the same way `PROVIDER_METADATA`
  (`frontend/packages/apps/web/src/providers.ts`) does, with compile-time
  exhaustiveness.
- Any brand-mark iconography (evaluated during this task and retired).
- Backend or wire changes.
