---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -q "restores a rate-limit window whose reset is still ahead regardless of snapshot age" frontend/packages/apps/web/src/store/statusPersistence.test.ts && grep -q "marks restored status stale until live data arrives for that key" frontend/packages/apps/web/src/store/liveStore.test.ts'
assignee: null
branch: task/0828-1914-feat-keep-restored-usage-visible-as-stale
created_at: 2026-08-28T10:14:00Z
updated_at: 2026-08-28T13:52:00Z
---

# feat(status): keep restored usage visible as stale instead of blanking it after an hour

## Overview

The navigator footer's rate-limit meters and the composer's context bar are
fed by live `status_updated` snapshots and restored from `localStorage` on
reload (`store/statusPersistence.ts`). Restore is currently gated by a
1-hour wall-clock TTL (`TTL_MS`): the whole snapshot is discarded when
`savedAt` is older. In practice this means opening Delta the morning after
an evening session shows **no rate-limit rows and no context bar at all**
until the next live snapshot happens to arrive — exactly the moment a
"roughly how much of the 7d window is left" readout would be most useful.
An untouched session's context usage cannot have changed overnight, and a
rate-limit window that has not reset yet still carries a meaningful
percentage (a lower bound — other devices can only have added to it). The
wall-clock TTL is the wrong guard: each rate-limit window already carries
its own expiry (`resets_at`), and for context usage the TTL was really
acting as garbage collection, not freshness.

Replace the blanket TTL with per-datum rules, and make restored (not yet
live-confirmed) values visually **stale** instead of invisible:

### Persistence rules (`store/statusPersistence.ts`)

- **Rate limits**: drop the 1-hour whole-snapshot TTL. A window whose
  `resets_at` has passed is dropped (keep `dropExpiredWindows` — the row
  disappears; it is never rendered as 0%, because after a reset neither the
  new percentage nor the next `resets_at` is known). A window with
  `resets_at: null` has no self-expiry, so keep a 1-hour fallback for those
  windows only, measured against their own observed time.
- **Context usage**: restore regardless of age; prune only as garbage
  collection — drop an entry whose observed time is older than 30 days
  (named constant, e.g. `CONTEXT_USAGE_GC_TTL_MS`). Without this the
  session-id-keyed map would accumulate dead sessions' entries forever.
- **Schema**: both rules above need to know *when each datum was observed*,
  and today the payload has a single `savedAt` refreshed on every save (a
  provider or session that went quiet days ago still shares the latest
  `savedAt`). Persist an observed-at epoch per provider (rate limits) and
  per session (context usage). A save must carry each entry's own observed
  time forward — only a live update for that key refreshes it. The payload
  under the `delta:status-snapshot` key changes shape; an old-shape payload
  is discarded to an empty restore (this is a best-effort cache, a one-time
  blank on upgrade is acceptable) — but it must be *detected*, never
  half-parsed into NaN timestamps.

### Staleness in the store (`store/live/statusSlice.ts`)

Restored rate-limit windows are unverified until the server speaks, so the
store must know which providers' windows came from `localStorage`. Track
restored provenance — e.g. a map of observed-at timestamps per restored
provider, exposed to components. Seed the stale mark only for providers
whose restored windows were observed more than one hour ago (reuse the
same 1-hour bound as the undated-window fallback): a fresher restore
renders as live from the start, because a quick mid-work reload must not
dim a minute-old reading — no live snapshot may arrive to un-dim it until
the next turn runs, so the dim would sit on a fresh value indefinitely.
An overnight restore stays marked. Clear the mark per provider on the
first live `status_updated` whose `rate_limits !== null`. A token-usage-only
frame (Codex sends usage and limits on separate frames) states nothing
about rate limits and must NOT clear the provider's stale mark — same
"apply only what the snapshot stated" rule the reducer already follows for
the values themselves.

Context usage gets **no stale mark**. Rate limits are account-wide, so
they can drift while this browser is closed (another device, the CLI) —
but a context percentage belongs to a session, and a session that emits no
live snapshot is one whose agent process is not running: its context
cannot have moved, so the restored value is exact, not a guess. A stale
mark would signal doubt about the one value that cannot drift — and since
a closed session sends no status event until its next turn, the mark would
sit there indefinitely. Restore it and render it exactly like a live
value.

### Stale rendering

- **Navigator footer** (`features/navigator/NavigatorPane.tsx`,
  `RateLimitRow`): when the focused provider's windows are restored-stale,
  render the rows de-emphasized (e.g. reduced opacity on the row) and make
  the observed time reachable — extend the Meter `title` (or equivalent)
  with "last observed <time>". Note the useful property: only the
  **percentage** is stale; the `↻` countdown and the budget line are
  computed from `resets_at` and the current clock at render time, so they
  stay exact even on restored data. Do not freeze or hide them.
- **Composer context bar** (`features/transcript/TranscriptPane.tsx`,
  `composer-context-bar`): no treatment — a restored value renders exactly
  as a live one (see "Staleness in the store" above for why).
- The footer returns to normal styling the moment the store clears the
  provider's stale mark (no reload needed).

Session-state matrix: not applicable — this task adds no operation against
a session; it changes what a reload restores and how it is styled.

### Tests

- `store/statusPersistence.test.ts`: a rate-limit window with a future
  `resets_at` is restored even when saved far more than an hour ago (name
  the test "restores a rate-limit window whose reset is still ahead
  regardless of snapshot age" — gate appended); an expired window is still
  dropped; a `resets_at: null` window is dropped past its 1-hour fallback
  but kept within it; a context-usage entry is restored past one hour and
  dropped past the 30-day GC bound; per-entry observed times survive a
  save that only refreshed a different key; an old-shape payload restores
  as empty.
- `store/liveStore.test.ts`: a provider restored with an observation
  older than one hour is marked stale while a fresher restore is not; a
  live snapshot stating rate limits clears only its provider; a
  usage-only frame does not clear the provider's stale mark; restored
  context usage carries no stale mark (name one of these "marks restored
  status stale until live data arrives for that key" — gate appended).
- `features/navigator/NavigatorPane.test.tsx`: stale windows render the
  de-emphasized treatment with the observed time reachable; live windows
  render as today.
- Existing composer context-bar coverage stays green unchanged — the bar
  gets no stale treatment.

Run `make check` and fix whatever it reports.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `statusPersistence` no longer applies a wall-clock TTL to rate-limit
      windows: a window with a future `resets_at` is restored no matter
      how old the snapshot (test gate appended), an expired window is
      dropped, and a `resets_at: null` window falls back to a 1-hour bound
      on its own observed time.
- [x] Context-usage entries are restored regardless of the 1-hour bound and
      pruned only past a named ~30-day GC constant; each persisted entry
      carries its own observed time, preserved across saves that update
      other keys; an old-shape `delta:status-snapshot` payload is detected
      and restores as empty.
- [x] The store marks a restored provider's rate-limit windows stale only
      when their observed time is more than one hour old at load (a
      fresher restore renders live from the start — a quick reload must
      not dim), and clears the mark per provider on the first live
      snapshot that states rate limits (test gate appended); a usage-only
      frame does not clear the mark; restored context usage carries no
      stale mark.
- [x] Stale rate-limit rows render de-emphasized with the observed time
      reachable (component tests), the `↻` countdown / budget line on
      stale rows are still computed from `resets_at` and the current time,
      and live data — and the restored context bar — render exactly as
      before.

### Manual / on-hardware (verified by a human before merge)

- [ ] Overnight dogfooding: after an evening of use, reload the next
      morning — the 7d row is visible, de-emphasized, with its observed
      time; the (reset) 5h row is absent rather than 0%; the context bar
      of yesterday's focused session is visible, styled as usual; the
      first live snapshot returns the footer to normal styling without a
      reload.
- [ ] The de-emphasized treatment is distinguishable from the live one in
      dark, light, and sepia themes, without being unreadable.

## Out of scope

- Any stale treatment for the composer context bar: a session with no
  live snapshot has no running agent process, so its restored percentage
  cannot have drifted — de-emphasizing it would cast doubt on an exact
  value, indefinitely (a closed session emits no status event to clear
  the mark). Decided during review of this task.
- Server-side replay of the last snapshot on WebSocket connect (an
  alternative considered and deferred: the localStorage restore path
  already exists and closes this gap frontend-only).
- Rendering anything (0%, a placeholder row) for a window whose
  `resets_at` has passed — its next window's state is unknown.
- Re-running expiry while the page stays open (load-time filtering only,
  as today); live snapshots keep an open page current.
- Any change to which provider's windows the footer shows (focused-session
  gating stays).
