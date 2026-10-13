/**
 * Wheel acceleration of the playhead: the velocity staircase, the cooldown,
 * the rolling window, and delta normalization.
 *
 * Shared fixtures live in ThreadTimelineOverlay.testkit.tsx.
 */
import { act, screen } from '@testing-library/react';
import {
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import type { Message } from '@delta/wire-gen';
import { LANE_LEFT_PAD_PX } from './TimelineLaneRow';
import {
  WHEEL_STEP_COOLDOWN_MS,
  WHEEL_VELOCITY_WINDOW_MS,
} from './timelineWheel';
import {
  makeThread,
  makeUserText,
  playheadLeftPx,
  renderOverlay,
  resetGlobals,
  stubAxisRect,
  timelineExpandedKey,
  waitForPlayheadAt,
} from './ThreadTimelineOverlay.testkit';

describe('ThreadTimelineOverlay playhead', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('advances exactly one large-message step on a single sub-notch turn', async () => {
    // Single wheel event with |deltaY| = 50 — below the first staircase
    // threshold (100), so the accumulator lands in the slowest bucket (1
    // step). Five large turns so any off-by-one in the calculator would
    // surface as a wrong landing px.
    const nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const messages = new Map([
        [
          1,
          [
            makeUserText(1, 0, 'm0', '2026-01-01T00:00:00Z'),
            makeUserText(1, 1, 'm1', '2026-01-01T00:01:00Z'),
            makeUserText(1, 2, 'm2', '2026-01-01T00:02:00Z'),
            makeUserText(1, 3, 'm3', '2026-01-01T00:03:00Z'),
            makeUserText(1, 4, 'm4', '2026-01-01T00:04:00Z'),
          ],
        ],
      ]);
      renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      // Initial active = m4 (x=1 → 240px).
      await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
      const body = screen.getByTestId('thread-timeline-axis-column');
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // m3 sits at x=0.75 → 180px — one step back, sub-notch event.
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${180 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('accelerates a fast burst across multiple steps via the staircase', async () => {
    // Five wheel events each at one notch (|deltaY| = 100), spaced just
    // above the output cooldown so every event commits and the staircase
    // owns the burst's pacing. The first event sits in the slowest
    // bucket (1 step — a single notch never accelerates) and later
    // events trip the higher buckets as their cumulative |delta| grows
    // inside the rolling window. The assertion is on the final landing
    // position, which captures the full burst's net advancement.
    let nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const msgs: Message[] = [];
      for (let i = 0; i < 10; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      const messages = new Map([[1, msgs]]);
      renderOverlay({
        threads,
        messagesByThread: messages,
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-axis-column');
      // Five back-to-back wheel-up events, one tick over the output
      // cooldown apart (so each event commits) and well inside the
      // 250 ms rolling window (so the accumulator keeps compounding
      // across events). The cadence references WHEEL_STEP_COOLDOWN_MS by
      // symbol so a future tuning of the cooldown moves this in lock-
      // step instead of leaving a stale magic number behind.
      const burstIntervalMs = WHEEL_STEP_COOLDOWN_MS + 10;
      for (let i = 0; i < 5; i += 1) {
        act(() => {
          body.dispatchEvent(
            new WheelEvent('wheel', {
              deltaY: -100,
              bubbles: true,
              cancelable: true,
            }),
          );
        });
        nowMs += burstIntervalMs;
      }
      // Cumulative steps walked backward across the five events (each
      // event reads the cumulative AFTER its own contribution lands).
      // The first notch always sits in the slowest bucket (1 step) so
      // the user can always land on the immediate neighbour. With the
      // 110 ms cadence, the rolling-window eviction drops the oldest
      // entry by the time the 4th event lands (cutoff = 5330 - 250 =
      // 5080 evicts the t=5000 entry), so cum stays at 300 for events
      // 3..5 instead of climbing to 400 / 500 like it did under the
      // tighter 50 ms cadence:
      //   t=5000: cum=100 → bucket 0   (1) → m9 → m8
      //   t=5110: cum=200 → bucket 200 (2) → m8 → m6
      //   t=5220: cum=300 → bucket 200 (2) → m6 → m4
      //   t=5330: cum=300 → bucket 200 (2) → m4 → m2
      //   t=5440: cum=300 → bucket 200 (2) → m2 → m0
      // Net 9 steps backward from m9 → m0 (x=0) is the final landing.
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('throttles a trackpad-style burst of pixel-mode events to one step per cooldown tick', async () => {
    // macOS trackpads emit a continuous stream of small pixel-mode wheel
    // events for a single gentle gesture (~5–20 px each, ~5–10 ms
    // apart). Each individual event sits below the per-event clamp, so
    // the clamp does not protect against the accumulator-and-staircase
    // committing a step on every event — the playhead races through
    // multiple messages on what the user perceived as one tap. The
    // output-side cooldown gate ({@link WHEEL_STEP_COOLDOWN_MS}) caps
    // commit throughput so a sub-notch event stream lands one step per
    // cooldown tick. This test pins that contract by dispatching 12
    // sub-notch events 10 ms apart and asserting the playhead landed
    // exactly two steps back (one from the first event, one from the
    // event right at the cooldown boundary) rather than racing through
    // every event individually.
    let nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      // 11 messages so two-step-back from the initial (m10) is a clean
      // assertion at m8 — width = 240, m8 sits at x = 8/10 → 192 px.
      const msgs: Message[] = [];
      for (let i = 0; i < 11; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      renderOverlay({
        threads,
        messagesByThread: new Map([[1, msgs]]),
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-axis-column');
      // 12 trackpad-sized events, 10 ms apart starting at t=5000:
      //   t=5000 (event 1): lastCommit=null → commits 1 step (m10 → m9)
      //   t=5010..t=5090 (events 2–10): inside cooldown → suppressed,
      //     accumulator keeps growing through every event
      //   t=5100 (event 11): gap = WHEEL_STEP_COOLDOWN_MS → cleared,
      //     cum ≈ 110 px (still bucket 0 → 1 step), commits (m9 → m8)
      //   t=5110 (event 12): gap = 10 ms → suppressed again
      // Net: 2 commits, final landing at m8.
      for (let i = 0; i < 12; i += 1) {
        act(() => {
          body.dispatchEvent(
            new WheelEvent('wheel', {
              deltaY: -10,
              bubbles: true,
              cancelable: true,
            }),
          );
        });
        nowMs += 10;
      }
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${192 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('lets the first wheel event after a long pause commit immediately (no cooldown gate on the first event)', async () => {
    // The cooldown gate compares the current event time to the prior
    // commit time. After a pause long enough that the gap exceeds
    // WHEEL_STEP_COOLDOWN_MS, the gate naturally does not engage — the
    // ref still holds the last commit time, but the math falls through.
    // This test pins "a leisurely two-notch scrub committed both
    // notches" by firing two sub-notch events separated by a gap that
    // also exceeds the rolling window (so the accumulator resets too,
    // keeping each event at the slowest staircase bucket) and asserting
    // the playhead walked exactly two steps.
    let nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const msgs: Message[] = [];
      for (let i = 0; i < 5; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      renderOverlay({
        threads,
        messagesByThread: new Map([[1, msgs]]),
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-axis-column');
      // First sub-notch event: lastCommit=null → commits (m4 → m3).
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Long pause: well past both the rolling window AND the cooldown
      // (whichever is longer, the gap clears). The accumulator resets
      // and the cooldown's "now - lastCommitAt" is huge — the next
      // event commits without any throttling.
      nowMs += WHEEL_VELOCITY_WINDOW_MS + 150;
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Five messages span x = 0, 60, 120, 180, 240. After two single-
      // step commits from m4 the playhead sits at m2 (x = 120).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('passes a steady mouse-wheel cadence through unthrottled', async () => {
    // Mouse wheels typically emit notches 150+ ms apart in normal use,
    // so the 100 ms cooldown must not slow them down — pins this here
    // so a future tuning that bumps the cooldown above the chosen
    // 200 ms cadence would have to update this test deliberately.
    let nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      // Six messages span x = 0, 48, 96, 144, 192, 240. Start at m5
      // (the latest). Three full-notch events at the documented mouse-
      // wheel cadence walk 5 large steps (1 + 2 + 2 via the staircase)
      // back to m0 (x = 0). The intermediate-cadence math:
      //   t=5000 (event 1): cum=100 → bucket 0   (1) → m5 → m4
      //   t=5200 (event 2): cum=200 → bucket 200 (2) → m4 → m2
      //                     (entry t=5000 survives — cutoff = 4950)
      //   t=5400 (event 3): cum=200 → bucket 200 (2) → m2 → m0
      //                     (entry t=5000 evicted — cutoff = 5150)
      // If the cooldown had swallowed either event 2 or event 3 the
      // final landing would be m2 (x = 96) instead of m0 (x = 0), so
      // m0 distinguishes "all three commits landed" from a throttled
      // path.
      const msgs: Message[] = [];
      for (let i = 0; i < 6; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      renderOverlay({
        threads,
        messagesByThread: new Map([[1, msgs]]),
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-axis-column');
      const wheelIntervalMs = 200;
      for (let i = 0; i < 3; i += 1) {
        act(() => {
          body.dispatchEvent(
            new WheelEvent('wheel', {
              deltaY: -100,
              bubbles: true,
              cancelable: true,
            }),
          );
        });
        nowMs += wheelIntervalMs;
      }
      expect(wheelIntervalMs).toBeGreaterThan(WHEEL_STEP_COOLDOWN_MS);
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('resets the accumulator after the rolling window elapses', async () => {
    // Two sub-notch events separated by a gap longer than the window. Each
    // event alone contributes 50 px (bucket 0 → 1 step). With reset, the
    // total is 2 steps; without reset, the second event's cumulative would
    // be 100 (bucket 100 → 2 steps) and the total would be 3 — pinning the
    // reset behaviour by total advancement.
    let nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const msgs: Message[] = [];
      for (let i = 0; i < 6; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      renderOverlay({
        threads,
        messagesByThread: new Map([[1, msgs]]),
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-axis-column');
      // First event: cum=50 → 1 step back (m5 → m4).
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Wait longer than the window so the accumulator resets.
      nowMs += WHEEL_VELOCITY_WINDOW_MS + 150;
      // Second event after the gap: fresh cum=50 → 1 step back (m4 → m3).
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Six messages at x = 0, 48, 96, 144, 192, 240. m3 sits at x = 144.
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${144 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('caps a single trackpad-sized event at the slowest bucket via the per-event clamp', async () => {
    // A single trackpad-sized event (|deltaY| = 10) lands in the slowest
    // staircase bucket (1 step) regardless of `deltaMode`. The clamp's
    // role is preventing a single noisy event from skipping straight to
    // the top bucket; this test pins that contract by dispatching the
    // burst's first event in isolation and asserting it advances exactly
    // one large step (rather than e.g. ten, which would happen if the
    // calculator multiplied steps by event count without going through
    // the staircase).
    const nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const msgs: Message[] = [];
      for (let i = 0; i < 6; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      renderOverlay({
        threads,
        messagesByThread: new Map([[1, msgs]]),
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-axis-column');
      // First trackpad event of a burst (|deltaY| = 10). Cumulative is 10
      // → bucket 0 → 1 step back. m5 → m4 (x=192 on the 6-msg axis).
      act(() => {
        body.dispatchEvent(
          new WheelEvent('wheel', {
            deltaY: -10,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      // Six messages → 5 gaps → 240 / 5 = 48 px each. m4 sits at x=192.
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${192 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('treats deltaMode=1 (line) as ~40 px per line via normalization', async () => {
    // A line-mode event with |deltaY| = 3 must behave like a pixel-mode
    // event of ~120 px — i.e. clamped at the per-event cap (100 px) and
    // tallied into the rolling-window accumulator at one notch's worth.
    // Two such back-to-back events together push the cumulative above
    // the 2-step bucket (200 px) so the burst walks 1 then 2 steps —
    // 3 large-message steps in total, mirroring how a real line-mode
    // device emits per-tick scrolls.
    let nowMs = 5_000;
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => nowMs);
    try {
      stubAxisRect({ left: 0, width: 240 });
      const threads = [makeThread(1)];
      const msgs: Message[] = [];
      for (let i = 0; i < 6; i += 1) {
        msgs.push(
          makeUserText(
            1,
            i,
            `m${i}`,
            `2026-01-01T00:${String(i).padStart(2, '0')}:00Z`,
          ),
        );
      }
      renderOverlay({
        threads,
        messagesByThread: new Map([[1, msgs]]),
        activeThreadId: 1,
        conversationArticles: [],
      });
      await screen.findAllByTestId('thread-timeline-dot');
      const body = screen.getByTestId('thread-timeline-axis-column');
      // Two line-mode events one tick past the output cooldown apart
      // (so the second event commits — the burst is not throttled).
      // Each event contributes 3 * 40 = 120 px clamped to 100 → cum=100
      // then 200. Walks 1 step then 2 steps: m5 → m4 → m2.
      for (let i = 0; i < 2; i += 1) {
        act(() => {
          body.dispatchEvent(
            new WheelEvent('wheel', {
              deltaY: -3,
              deltaMode: 1,
              bubbles: true,
              cancelable: true,
            }),
          );
        });
        nowMs += WHEEL_STEP_COOLDOWN_MS + 10;
      }
      // Six messages at x = 0, 48, 96, 144, 192, 240. Starting on m5
      // (x=240), three steps back → m2 (x=96).
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${96 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });
});
