---
status: completed
pipeline_phase: null
plan: null
base_ref: task/0717-0618-feat-visual-effects-setting
retries_remaining: 1
check_command: "make check && make e2e"
assignee: null
branch: task/0717-0620-perf-session-hover-transition
created_at: 2026-07-17T06:20:00Z
updated_at: 2026-07-17T08:52:00Z
---

# perf(web): make the session-card hover highlight instant (drop its color transition)

## Overview

The session card in the navigator highlights on hover by strengthening its
border color (`hover:border-border-strong`), but the card also carries
`transition-colors`
(`frontend/packages/apps/web/src/features/navigator/SessionNode.tsx`, the
card `div` under the virtualized row), so both the hover-in and hover-out
border changes animate over Tailwind's default 150 ms. Sweeping the cursor
down the list therefore shows each row reaching full highlight ~150 ms after
the cursor crossed it and fading out in a staggered trail — on WebKitGTK,
whose display path already adds a frame or two of latency, this reads as the
highlight visibly lagging the cursor (2026-07-17 Epiphany dogfooding; an
in-browser A/B injecting `* { transition: none !important; }` confirmed the
perceived lag mostly disappears).

Hover feedback is a "respond now" affordance: a transition on it only adds
latency, on every engine. Remove `transition-colors` from the session card so
the hover (and focused-state) border/background changes apply instantly. This
is deliberately unconditional — not part of the visual-effects setting this
task stacks on — because instant hover response is correct everywhere, while
shadows/washes are a taste/cost trade-off.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The session card element no longer has the `transition-colors` class
      (component test on the rendered card's class list).
- [x] Existing navigator/session-list tests and e2e still pass (hover
      highlight and focused highlight are otherwise unchanged).

### Manual / on-hardware (verified by a human before merge)

- [ ] On Epiphany (Linux WebKitGTK), sweeping the cursor over the session
      list: each row highlights the moment the cursor enters it, with no
      trailing fade-out queue.

## Out of scope

- Transitions anywhere else (e.g. the composer send button's hover styling,
  settings dialog) — this task touches only the session card's hover path.
- The visual-effects setting itself (base task).
