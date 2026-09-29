---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -rq 'rate-limits-' frontend/packages/apps/web/src/features/navigator/ && grep -rqE 'new-session|NEW_SESSION_FOCUS' frontend/packages/apps/web/src/features/navigator/NavigatorPane.test.tsx && grep -rq 'rate-limits-codex' frontend/packages/apps/web/e2e/status-line.spec.ts"
assignee: null
branch: task/0929-1740-feat-show-every-provider-rate-limit-on-the-new-session-screen
created_at: 2026-09-29T08:40:00Z
updated_at: 2026-09-29T09:50:00Z
---

# feat(web): show every provider's rate limits on the new-session screen

## Overview

The navigator footer shows the account-wide rate-limit meters (Claude's 5h /
7d windows, Codex's 30m / 1d windows) of the **focused session's provider**
(`frontend/packages/apps/web/src/features/navigator/NavigatorPane.tsx:383-431`,
rendered at `:478-513`). On the new-session screen the focus is the pseudo id
`NEW_SESSION_FOCUS` (`frontend/packages/apps/web/src/store/navStore.ts:38`),
which matches no session, so the lookup yields no provider and the footer
renders no meters at all.

That is exactly the moment the meters matter most: a user about to open a
session decides *which* provider to use partly by how much budget each account
has left ("Codex is nearly exhausted this window, start this one on Claude").
The data is already there — `liveStore.rateLimits` is keyed by provider
(`frontend/packages/apps/web/src/store/live/statusSlice.ts:33`) and survives
reloads through `statusPersistence` — it is only the footer's "which provider"
rule that hides it.

### The change

1. **Footer shows one meter group per provider, and the new-session screen
   shows all of them.** Replace the single-provider lookup with a small
   selector/hook (its own file beside `NavigatorPane.tsx`, e.g.
   `footerRateLimitProviders.ts`) that answers *which providers the footer
   lists*:
   - a thread is focused → exactly that session's provider (today's rule; the
     "no cross-provider leak" e2e at
     `frontend/packages/apps/web/e2e/status-line.spec.ts:129` must keep
     passing unchanged);
   - `NEW_SESSION_FOCUS` → every provider that has a **non-empty** window list
     in `liveStore.rateLimits`, in the fixed `PROVIDER_OPTIONS` order
     (`frontend/packages/apps/web/src/providers.ts:51`), never in arrival
     order. A provider that has never reported (no map entry) or reported `[]`
     is not listed, so a user who never used Codex sees no Codex group — no
     new setting, no new branch, just the existing "no empty bars" rule.
   `NavigatorPane` maps over the returned list and carries no mode branch of
   its own. The selected provider tab does **not** filter or highlight the
   footer: the footer is information, the tabs are the control.

2. **Each group has a provider heading on the new-session screen only.**
   Render the provider name above its rows with the existing `ProviderName`
   from `@delta/ui-kit` (`frontend/packages/ui/ui-kit/src/ProviderName.tsx`),
   styled as a quiet caption consistent with the footer. Windows are labeled
   by duration only (`5h`, `1d`), so on the new-session screen — where several
   providers' groups sit side by side and no provider has been chosen yet —
   the heading is what tells the rows apart. In a focused thread the user
   already knows which provider the session runs on, so the single group
   renders its rows with no heading, exactly as the footer looked before. The
   selector from step 1 says whether a group is labelled, so `NavigatorPane`
   still carries no mode branch.

3. **Per-group container test id.** Wrap each group in
   `data-testid="rate-limits-<provider>"` (e.g. `rate-limits-claude`,
   `rate-limits-codex`). Keep the existing `rate-limits` id on the outer
   block and the per-row ids (`rate-limit-5h`, `rate-limit-5h-pct`, …)
   unchanged, so the existing unit and e2e assertions stay valid; new
   new-session assertions scope through the group container, since two
   providers may report a window of the same length.

4. **Restored/stale styling stays per provider.** `restoredRateLimitsObservedAt`
   is already keyed by provider (`statusSlice.ts:60`); pass each group its own
   observed-at so "Codex is a restored guess, Claude is live" renders as the
   existing de-emphasis on the Codex group only.

5. **Comments.** Rewrite the footer comments at `NavigatorPane.tsx:383-388`
   and `:478-484` ("with nothing focused there is no account to speak for")
   to state the new rule, and the doc comment on the new selector should say
   *why* the new-session screen lists every provider (the choice the user is
   about to make).

### Tests

- Unit (`NavigatorPane.test.tsx`, `describe('NavigatorPane rate-limit meters')`
  at `:147`): with `focusedSessionId = NEW_SESSION_FOCUS` and both providers
  holding windows, both groups render with their headings, in Claude → Codex
  order regardless of map insertion order; a provider with no entry or `[]`
  renders no group; a focused thread still renders only its own provider's
  group; the per-group stale de-emphasis applies to one group only.
- e2e (`e2e/status-line.spec.ts`): add a test that emits a Claude snapshot and
  a Codex snapshot, clicks "New session", and asserts both `rate-limits-claude`
  and `rate-limits-codex` are visible with their percentages, then focuses the
  Claude session and asserts `rate-limits-codex` has count 0. Follow the
  existing `emitEvent` / `snapshot` helpers in that file.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] On the new-session screen the footer renders one meter group per
      provider that holds a non-empty window list, in `PROVIDER_OPTIONS`
      order, each under a `ProviderName` heading (unit test).
- [x] A provider with no reported windows (no entry or `[]`) renders no group
      on the new-session screen (unit test).
- [x] A focused thread still renders only its own provider's group, with no
      provider heading (unit test); the existing "no cross-provider leak" e2e
      passes unchanged.
- [x] Each group is wrapped in `data-testid="rate-limits-<provider>"`; the
      existing `rate-limits` and `rate-limit-<label>*` ids are unchanged.
- [x] e2e: after snapshots from both providers, "New session" shows both
      groups; focusing a Claude session hides the Codex group.
- [x] `make check` passes (lint, unit, e2e-fake, gen-check).

### Manual / on-hardware (verified by a human before merge)

- [ ] On a real instance with both a Claude and a Codex session in history,
      "New session" shows both accounts' meters; with only Claude ever used,
      no Codex group appears.

## Out of scope

- The composer's top-edge context-usage bar (per-session, never shown on the
  new-session screen) — unchanged.
- Highlighting or filtering the footer by the selected provider tab.
- Any change to how rate limits are received, stored or persisted.
