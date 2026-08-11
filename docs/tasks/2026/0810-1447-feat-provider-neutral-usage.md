---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: 'cd backend && cargo fmt --all -- --check && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && BEFORE=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && make gen && AFTER=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && [ "$BEFORE" = "$AFTER" ] && cd frontend && pnpm -r build && pnpm -r typecheck && pnpm -r test && pnpm -r lint && cd .. && make e2e'
assignee: null
branch: task/0810-1447-feat-provider-neutral-usage
created_at: 2026-08-10T14:47:22Z
updated_at: 2026-08-11T03:50:00Z
---

# feat(usage): provider-neutral token usage and rate limits

## Overview

The token-usage / rate-limits UI has exactly one producer today: Claude's
status-line command POSTs structured JSON to `/hooks/status-line`
(`backend/crates/libs/delta-bootstrap/src/settings.rs:75`), whose handler
(`backend/crates/apps/delta-server/src/hooks/mod.rs:317`) broadcasts
`SessionEvent::StatusUpdated { session_id, snapshot }` — broadcast-only,
never persisted. The frontend consumes two things from it
(`frontend/packages/apps/web/src/store/live/statusSlice.ts`): a per-session
`context_used_percentage` (composer context bar,
`frontend/packages/apps/web/src/features/transcript/TranscriptPane.tsx`
~line 1054) and a single **global** `rateLimits` slot (navigator footer,
`frontend/packages/apps/web/src/features/navigator/NavigatorPane.tsx`
~line 411, hardcoded 5h/7d rows).

Codex sessions are adapter-backed and have no hook, so nothing ever produces
usage for them — even though `codex app-server` emits equivalent data
(vendored 0.144.4 schema, `backend/crates/gateway/codex-agent/vendor/`):

- `thread/tokenUsage/updated` (per-thread): `tokenUsage.total` / `.last`
  (`totalTokens`, `inputTokens`, `cachedInputTokens`, `outputTokens`,
  `reasoningOutputTokens` — all required) plus optional `modelContextWindow`.
- `account/rateLimits/updated` (account-scoped, **no `threadId`**): sparse
  `RateLimitSnapshot` — `primary` / `secondary` windows (`usedPercent` int,
  optional `resetsAt` epoch-seconds, optional `windowDurationMins`), plus
  credits/plan metadata. The schema explicitly says clients must **merge**
  sparse updates: a null field "does not clear a previously observed value".

Two structural gaps swallow these frames today:

1. The notification match in
   `backend/crates/gateway/codex-agent/src/translate.rs` (~line 146) has no
   arm for either method; `thread/tokenUsage/updated` carries a `threadId`,
   reaches the match, and dies at the `_ => Vec::new()` catch-all.
2. `account/rateLimits/updated` has no `threadId`, so routing
   (`backend/crates/gateway/codex-agent/src/lib.rs` ~line 615,
   `route_thread_event`) diverts it to the connection-level `unrouted`
   channel — which production never drains (`take_unrouted` is used only in
   tests). It accumulates unbounded. Note the topology: **one shared
   app-server connection hosts many sessions** (`adapter.rs` module docs), so
   this data is genuinely account × provider scoped, not per-session.

Hoist the usage supply chain from hook-dependent to provider-neutral, so any
adapter-backed provider can populate the same UI.

Implementation direction (details are yours, invariants are not):

- **Additive neutral event.** Follow the `ContentBlock::Thinking` precedent
  (delta#303): add `AgentEvent` variant(s) carrying a provider-neutral usage
  model; the agent-event pump maps them onto the existing
  `SessionEvent::StatusUpdated` broadcast. Observability only: no sqlite
  writes, `SCHEMA_VERSION` unchanged, no coupling into attribution or the
  persistence pipeline — status stays fire-and-forget with client-side
  persistence, exactly as today.
- **Percentages are computed by the provider's own edge.** Codex reports
  absolute counts plus `modelContextWindow`, never a percentage; the Claude
  path's documented rule "forward `used_percentage` verbatim, never
  recompute" (`hooks/mod.rs` ~line 334) generalizes to "the neutral layer
  never recomputes; each provider's adapter/hook is the authority for its own
  numbers". The codex adapter computes
  `last.totalTokens / modelContextWindow` (`last` = the latest model call,
  which is what actually occupies the context window — `total` is the
  thread-lifetime cumulative sum and exceeds the window after a few turns;
  omit the percentage when `modelContextWindow` is absent — never
  NaN/garbage) and the docstrings are updated to state the generalized rule.
- **Rate-limit windows become data-shaped, not name-shaped.** Claude has
  `five_hour`/`seven_day`; Codex has anonymous `primary`/`secondary` with an
  explicit `windowDurationMins`. Mapping `primary → five_hour` would be a
  lie. Generalize the snapshot's rate-limit shape so window identity
  (duration) travels with the data, map Claude's fixed windows onto it, and
  render the navigator rows from the received windows instead of hardcoded
  5h/7d rows — while keeping Claude's rendered output visually unchanged.
- **Rate limits are account × provider scoped.** Store them keyed by
  provider (last-writer-wins within a provider). Invariant: the UI must never
  present provider A's account limits in a context that implies they belong
  to provider B — today's single global slot would show Claude's limits while
  a Codex session is focused. Sparse Codex updates merge into the previously
  observed snapshot per the schema note; null fields do not clear values.
- **Drain the unrouted channel.** Account-scoped notifications must reach the
  new supply path via a drain owned at the connection level; frames that
  still cannot be translated are dropped with a log line (or comms-log
  entry), never silently accumulated. Fix the unbounded channel as part of
  this.
- **fake-codex fidelity.** The fake stamps `threadId` onto every scripted
  notification (`backend/crates/apps/fake-codex/src/server.rs` ~line 311), so
  a naively scripted `account/rateLimits/updated` would exercise the routed
  path while the real server takes the unrouted path — the test would pass
  against a bug. Give the fake a way to emit account-scoped notifications
  without a `threadId`, and add scenario coverage for both notifications.
- **Data-driven UI, never `provider === 'codex'`.** The display is gated on
  data presence (a session with no usage data renders exactly today's empty
  state); no provider branches in the frontend.

Operation × state coverage (usage display vs focused-session state):

- Focused session is Claude → navigator footer and context bar behave exactly
  as today (byte-identical wire fixtures except where the generalized
  rate-limit shape requires updating; identical rendered output).
- Focused session is Codex, turn completes → context bar reflects the latest
  `thread/tokenUsage/updated`; navigator shows Codex windows once an
  `account/rateLimits/updated` has been observed.
- Codex `tokenUsage` without `modelContextWindow` → no percentage: bar
  omitted for that session, no NaN.
- Sparse `rateLimits` update (all fields nullable) → merges into the previous
  snapshot; nulls do not clobber.
- Claude and Codex sessions live simultaneously → switching focus swaps the
  rate-limit display to the focused provider without leakage; localStorage
  persistence round-trips the new shape (existing TTL semantics preserved).
- Session with no usage data at all (e.g. resumed Codex session before its
  first turn) → today's empty state; no crash, no stale other-provider data.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The codex adapter translates `thread/tokenUsage/updated` into a neutral
      usage event carrying token counts and a percentage computed from
      `modelContextWindow` (omitted when absent) — asserted by an
      adapter-level test against `fake-codex`; the frame no longer falls into
      the `_ =>` catch-all.
- [x] An `account/rateLimits/updated` notification emitted **without**
      `threadId` (fake-codex extended to do so) reaches the browser-visible
      rate-limit state via the connection-level drain — asserted by a
      server-level test; the unrouted channel no longer accumulates
      unbounded.
- [x] Sparse rate-limit merge semantics are unit-tested: a second update with
      null fields does not clear previously observed values.
- [x] Usage stays out of the conversation pipeline: no new persisted
      `SessionEvent`, `SCHEMA_VERSION` unchanged; golden and e2e-fake suites
      pass with Claude behavior unchanged (fixtures touched only where the
      generalized rate-limit wire shape requires it).
- [x] Wire exhaustiveness guards (`covered()` samples in
      `delta-wire/src/session_event.rs`) and the e2e snapshot helper
      (`frontend/packages/apps/web/e2e/status-line.spec.ts`) cover the new
      shape; generated TS types are in sync (hash-compare gate in
      `check_command`).
- [x] Frontend store keys rate limits per provider and renders navigator rows
      from received window durations; reducers are unit-tested for the
      operation × state rows listed in the Overview.
- [x] Frontend e2e with the fake Codex provider: a completed turn shows the
      composer context bar, and a rate-limits frame produces navigator rows
      labeled from window durations; existing Claude status-line e2e passes
      with unchanged rendered output.
- [x] No `provider === 'codex'` (or equivalent provider-literal) branch is
      introduced in the frontend for usage display.

### Manual / on-hardware (verified by a human before merge)

- [x] Against a real `codex app-server` session: after a turn completes, the
      composer context bar percentage matches the `thread/tokenUsage/updated`
      frame visible in the comms pane (`last.totalTokens /
      modelContextWindow`),
      and rate-limit rows appear and match an observed
      `account/rateLimits/updated` frame.
- [x] A Claude session on the same build shows the navigator footer and
      context bar exactly as before; switching focus between a Claude and a
      Codex session swaps the rate-limit display with no cross-provider
      leakage.
- [x] Run `make e2e-real-codex` (ignored canaries) once and confirm green —
      these do not run in CI.
