---
status: completed
pipeline_phase: null
plan: null
base_ref: task/0618-2344-feat-statusline-ingestion
blocked_by: [0618-2344-feat-statusline-ingestion.md]
subagent_type: general-purpose
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0618-2345-feat-statusline-ui
created_at: 2026-06-18T14:44:56Z
updated_at: 2026-06-18T18:42:00Z
---

# feat(web): show context usage and rate limits from statusLine in the UI

## Overview

The server now broadcasts a `StatusUpdated` session event carrying the Claude
Code status-line snapshot (context-window usage percentage, 5h/7d rate limits,
selected model, cost) keyed by `session_id`. This task renders two pieces of
that snapshot in the web UI. It is stacked on the ingestion task — it branches
off `task/0618-2344-feat-statusline-ingestion` and consumes the `StatusUpdated`
event and its generated TypeScript binding.

Because the status line fires frequently, the live store must treat the snapshot
as a **latest value to replace, not append**: keep the most recent snapshot per
session for the per-session context bar, and a single most-recent rate-limit
snapshot globally (rate limits are account-wide, identical across sessions).

Add a reusable bar primitive and two consumers.

1. **`Meter` primitive** in `frontend/packages/ui/ui-kit/src/` (alongside the
   existing `StatusDot` / `Badge` / `Chip` / `Panel`). It renders a real DOM
   bar — a rounded track `div` with a rounded fill `div` whose width is the
   percentage — **not** a text/unicode bar. It accepts a 0–100 value and an
   accent/track styling hook, exposes `role="meter"` with
   `aria-valuenow`/`aria-valuemin`/`aria-valuemax`, and clamps out-of-range
   values. A numeric label is the caller's concern (passed as a sibling), not
   baked in.

2. **Global rate limits in the navigator footer**
   (`frontend/packages/apps/web/src/features/navigator/NavigatorPane.tsx`). The
   footer today holds a connection dot + "Connected" + a settings gear. Add, in
   the same footer (the natural home for app-global state), two `Meter` rows
   **above** the connection row: `5h <meter> NN% ↻<relative reset>` and
   `7d <meter> NN% ↻<relative reset>`. Rate limits are account-wide, so they are
   driven by the single global snapshot, not the focused session. The reset
   label is derived from `resets_at` (Unix epoch seconds) as a compact relative
   string (e.g. `↻02h13m`, `↻5d04h`). **When a rate-limit window is absent
   (non-Pro/Max account, or before the first API response), hide that row
   entirely** — do not render an empty/zeroed bar. The 5h and 7d bars use
   distinct static accent colors purely to tell them apart (no threshold/colour
   change based on the value).

3. **Per-session context bar on the composer**
   (`frontend/packages/apps/web/src/features/composer/Composer.tsx` and the
   composer card assembled in the transcript pane's bottom overlay). Render the
   focused session's context `used_percentage` as a thin ambient fill along the
   **top edge of the composer card** (the card's top border doubles as the
   track; fill it from the left to `used_percentage`%). This is the spot a user
   most wants it — right where they are about to send. Show the numeric `NN%`
   small at the edge. **When `used_percentage` is unavailable for the focused
   session (no snapshot yet, or null right after `/compact`), hide the bar /
   omit the fill** rather than showing 0%. Forward the server's
   `used_percentage` directly; do not recompute it.

Do not add a "currently selected model" indicator in this task — the selected
model is covered well enough elsewhere and is out of scope here.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A `Meter` component exists in `ui-kit` rendering a track+fill DOM
      structure with `role="meter"` and `aria-valuenow` set to the clamped
      value; a unit test asserts the fill width corresponds to the value and
      that out-of-range values are clamped to 0–100.
- [x] A unit/component test for the navigator footer asserts that, given a
      global snapshot with both 5h and 7d rate limits, two meter rows render
      with their percentages and relative reset labels; and that given a
      snapshot **without** rate limits, neither row renders (no empty bars).
- [x] A unit/component test for the composer context bar asserts that, given a
      focused-session snapshot with `used_percentage`, the bar renders with a
      fill proportional to that value; and that with no/`null`
      `used_percentage` the fill is omitted.
- [x] The live store keeps the `StatusUpdated` snapshot as a replace-latest
      value (per-session for context, single global for rate limits), asserted
      by a store unit test feeding two successive events and checking only the
      latest is retained.
- [x] `make check` passes (frontend build + typecheck + vitest + ESLint +
      dependency-cruiser, and the backend gate inherited from the base branch).

### Manual / on-hardware (verified by a human before merge)

- [ ] Running `make mock` and feeding a `StatusUpdated` event through the fake
      event source, the 5h/7d footer meters and the composer top-edge context
      bar render and update correctly, and the footer rows disappear when a
      snapshot without rate limits arrives. (Visual/runtime confirmation the
      unit suite cannot make.)
- [ ] A Playwright e2e spec is added under `packages/apps/web/e2e/` that drives
      mock mode, feeds a `StatusUpdated` event through the fake event source, and
      asserts the footer meters and composer context bar reflect it. It passes
      under `make e2e` (run by a human / separate job — not part of the
      `make check` pipeline gate).

## Out of scope

- Per-message transcript metadata (model/cwd/branch/time) — separate task.
- A "currently selected model" indicator.
- Threshold-based colour changes on high context usage (may come later).
- Cost (`total_cost_usd`) display — not part of this task's two consumers.
</content>
