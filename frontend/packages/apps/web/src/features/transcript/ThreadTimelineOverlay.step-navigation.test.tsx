/**
 * Wheel and keyboard stepping across large marks.
 *
 * Shared fixtures live in ThreadTimelineOverlay.testkit.tsx.
 */
import {
  act,
  fireEvent,
  screen,
} from '@testing-library/react';
import {
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import type { Message } from '@delta/wire-gen';
import { LANE_LEFT_PAD_PX } from './ThreadTimelineOverlay';
import {
  makeMessage,
  makeThread,
  makeUserText,
  playheadLeftPx,
  renderOverlay,
  resetGlobals,
  stubAxisRect,
  timelineExpandedKey,
  waitForPlayheadAt,
} from './ThreadTimelineOverlay.testkit';

describe('ThreadTimelineOverlay wheel skips small marks', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('walks only the main-conversation (large) subset on wheel, jumping over tool calls', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    // Three large turns interleaved with a small tool call. The wheel-step
    // navigation must skip the tool call so one notch advances by one
    // headline turn — `large-b` should land between `large-a` and
    // `large-c` regardless of the small dot's x.
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'large-a', '2026-01-01T00:00:00Z'),
          makeMessage(1, 1, 'small-t', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:01:00Z',
          }),
          makeUserText(1, 2, 'large-b', '2026-01-01T00:02:00Z'),
          makeUserText(1, 3, 'large-c', '2026-01-01T00:03:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'large-a' },
        { uuid: 'small-t' },
        { uuid: 'large-b' },
        { uuid: 'large-c' },
      ],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    // Initial playhead anchors to the active lane’s latest large turn
    // (large-c, x=1 → 240px).
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    // Wheel up (sub-notch event → one step): the previous LARGE turn is
    // large-b (x=2/3 → 160px), NOT the small tool call between them. The
    // sub-notch keeps the staircase at one step so the assertion targets
    // the immediate large neighbour.
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
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${160 + LANE_LEFT_PAD_PX}px`);
  });

  it('still allows a click to target a small mark precisely', async () => {
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    // A small tool call between two large turns. A click at the tool call's
    // x must jump the playhead to it, even though the wheel would skip it.
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'large-a', '2026-01-01T00:00:00Z'),
          makeMessage(1, 1, 'small-t', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            // Land at x=0.5 (the midpoint) so the click target is
            // unambiguous and deterministic.
            created_at: '2026-01-01T00:01:00Z',
          }),
          makeUserText(1, 2, 'large-b', '2026-01-01T00:02:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'large-a' },
        { uuid: 'small-t' },
        { uuid: 'large-b' },
      ],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    // Click at x=120 (midpoint) → small-t is the nearest mark.
    fireEvent.click(screen.getByTestId('thread-timeline-axis-column'), {
      clientX: 120,
    });
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
  });
});

describe('ThreadTimelineOverlay keyboard navigation', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  /**
   * Three evenly-spaced large turns at x=0, x=0.5, x=1 (px 0, 120, 240 on
   * the 240 px stub axis) — the same fixture shape the wheel-step tests
   * use, so the keyboard assertions observe the identical playhead moves a
   * wheel step produces.
   */
  function threeLargeTurns(): Map<number, Message[]> {
    return new Map([
      [
        1,
        [
          makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
          makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
          makeUserText(1, 2, 'msg-c', '2026-01-01T00:02:00Z'),
        ],
      ],
    ]);
  }

  function renderThreeLargeTurns() {
    stubAxisRect({ left: 0, width: 240 });
    return renderOverlay({
      threads: [makeThread(1)],
      messagesByThread: threeLargeTurns(),
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'msg-a' },
        { uuid: 'msg-b' },
        { uuid: 'msg-c' },
      ],
    });
  }

  /**
   * Dispatch a `keydown` from the given target (default: `window`, where
   * the overlay's listener lives) and return the event so the caller can
   * assert on `defaultPrevented`. `bubbles` is on so events fired from a
   * child element (an input, the conversation body) reach the window
   * listener the way real typing does.
   */
  function pressKey(
    key: string,
    init: KeyboardEventInit = {},
    target: EventTarget = window,
  ): KeyboardEvent {
    const event = new KeyboardEvent('keydown', {
      key,
      bubbles: true,
      cancelable: true,
      ...init,
    });
    act(() => {
      target.dispatchEvent(event);
    });
    return event;
  }

  function playheadPx(): string {
    return playheadLeftPx(
      screen.getAllByTestId('thread-timeline-playhead')[0],
    );
  }

  it('steps one large message per plain ArrowLeft / ArrowRight keydown through the jump path', async () => {
    renderThreeLargeTurns();
    await screen.findAllByTestId('thread-timeline-dot');
    // Initial playhead anchors to the active lane’s latest large turn
    // (msg-c, x=1 → 240px).
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    // ArrowLeft → one large message towards the older end (msg-b), the
    // same playhead move a single wheel-up step produces. The handled key
    // is preventDefault-ed so it cannot leak into page scrolling.
    const left = pressKey('ArrowLeft');
    expect(left.defaultPrevented).toBe(true);
    expect(playheadPx()).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    // ArrowRight → one large message towards the newer end (back to msg-c).
    const right = pressKey('ArrowRight');
    expect(right.defaultPrevented).toBe(true);
    expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
  });

  it('walks the large-message subset, skipping small auxiliary marks', async () => {
    stubAxisRect({ left: 0, width: 240 });
    // A small tool call between two large turns: one keypress from large-c
    // must land on the adjacent LARGE message (large-b, x=2/3 → 160px),
    // never on the small mark between them.
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'large-a', '2026-01-01T00:00:00Z'),
          makeMessage(1, 1, 'small-t', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:01:00Z',
          }),
          makeUserText(1, 2, 'large-b', '2026-01-01T00:02:00Z'),
          makeUserText(1, 3, 'large-c', '2026-01-01T00:03:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads: [makeThread(1)],
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'large-a' },
        { uuid: 'small-t' },
        { uuid: 'large-b' },
        { uuid: 'large-c' },
      ],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    pressKey('ArrowLeft');
    expect(playheadPx()).toBe(`${160 + LANE_LEFT_PAD_PX}px`);
  });

  it('steps once per key-repeat keydown with no cooldown, clamping without wrap at both ends', async () => {
    // Freeze the clock: if the wheel's WHEEL_STEP_COOLDOWN_MS output gate
    // applied to keys, every press after the first would land at the same
    // frozen instant and be swallowed. Keys must not be gated — one
    // keydown (repeat or not) is exactly one step.
    const nowSpy = vi.spyOn(performance, 'now').mockImplementation(() => 5_000);
    try {
      renderThreeLargeTurns();
      await screen.findAllByTestId('thread-timeline-dot');
      await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
      // Two held-key repeats advance two large messages: msg-c → msg-a.
      pressKey('ArrowLeft', { repeat: true });
      pressKey('ArrowLeft', { repeat: true });
      expect(playheadPx()).toBe(`${LANE_LEFT_PAD_PX}px`);
      // At the oldest end a further ArrowLeft clamps (no wrap) but is
      // still preventDefault-ed — the timeline owns the key even when the
      // step is a no-op.
      const clampedLeft = pressKey('ArrowLeft', { repeat: true });
      expect(clampedLeft.defaultPrevented).toBe(true);
      expect(playheadPx()).toBe(`${LANE_LEFT_PAD_PX}px`);
      // Back the other way: two repeats reach the newest end, the third
      // clamps without wrapping and is still preventDefault-ed.
      pressKey('ArrowRight', { repeat: true });
      pressKey('ArrowRight', { repeat: true });
      expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
      const clampedRight = pressKey('ArrowRight', { repeat: true });
      expect(clampedRight.defaultPrevented).toBe(true);
      expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it('ignores keydowns from editable targets (input / textarea / select / contentEditable) without preventDefault', async () => {
    renderThreeLargeTurns();
    await screen.findAllByTestId('thread-timeline-dot');
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    const editables: HTMLElement[] = [
      document.createElement('input'),
      document.createElement('textarea'),
      document.createElement('select'),
    ];
    // jsdom does not compute `isContentEditable` from the attribute, so
    // stub the property the guard reads — in a real browser any element
    // inside a contentEditable region reports true through inheritance.
    const editableDiv = document.createElement('div');
    Object.defineProperty(editableDiv, 'isContentEditable', { value: true });
    editables.push(editableDiv);
    try {
      for (const el of editables) {
        document.body.appendChild(el);
        // Bubbles from the editable target up to the window listener, the
        // way a real keystroke in the composer / terminal does.
        const event = pressKey('ArrowLeft', {}, el);
        expect(event.defaultPrevented).toBe(false);
        expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
      }
    } finally {
      for (const el of editables) {
        el.remove();
      }
    }
  });

  it('ignores modified arrows (Ctrl / Meta / Alt) and already-defaultPrevented keydowns', async () => {
    renderThreeLargeTurns();
    await screen.findAllByTestId('thread-timeline-dot');
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    for (const init of [
      { ctrlKey: true },
      { metaKey: true },
      { altKey: true },
    ]) {
      const event = pressKey('ArrowLeft', init);
      expect(event.defaultPrevented).toBe(false);
      expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    }
    // A keydown some earlier handler already claimed stays claimed: the
    // timeline must not act on it.
    const claimed = new KeyboardEvent('keydown', {
      key: 'ArrowLeft',
      bubbles: true,
      cancelable: true,
    });
    claimed.preventDefault();
    act(() => {
      window.dispatchEvent(claimed);
    });
    expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
  });

  it('attaches no keydown listener while the timeline is collapsed', () => {
    // Override the describe-level beforeEach (which seeds the expanded
    // preference to 'true') back to the collapsed state. No cache reset is
    // needed: `resetGlobals` already cleared the module-level cache, and it
    // only fills on the first hook read, at render — after this write.
    window.localStorage.setItem(timelineExpandedKey(), 'false');
    stubAxisRect({ left: 0, width: 240 });
    renderOverlay({
      threads: [makeThread(1)],
      messagesByThread: threeLargeTurns(),
      activeThreadId: 1,
      conversationArticles: [
        { uuid: 'msg-a' },
        { uuid: 'msg-b' },
        { uuid: 'msg-c' },
      ],
    });
    expect(
      screen.getByTestId('thread-timeline-toggle'),
    ).toHaveAttribute('aria-expanded', 'false');
    // No listener while collapsed: arrows are neither preventDefault-ed
    // nor do they touch any timeline state (no playhead exists to move).
    const event = pressKey('ArrowLeft');
    expect(event.defaultPrevented).toBe(false);
    expect(screen.queryAllByTestId('thread-timeline-playhead')).toHaveLength(0);
  });

  it('detaches the listener when the timeline is collapsed mid-session and reattaches on re-expand', async () => {
    renderThreeLargeTurns();
    await screen.findAllByTestId('thread-timeline-dot');
    // Step from the settled mount anchor (msg-c), not from whatever the
    // playhead happens to read the moment the marks appear.
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    // Sanity: while expanded the timeline owns the arrows.
    expect(pressKey('ArrowLeft').defaultPrevented).toBe(true);
    expect(playheadPx()).toBe(`${120 + LANE_LEFT_PAD_PX}px`);
    // Collapse via the header toggle — the more common path to a collapsed
    // timeline than mounting collapsed. This exercises the effect CLEANUP:
    // the window listener must actually be removed, not merely no-op
    // behind a stale `expanded` closure, or every arrow key on the page
    // would keep being swallowed after the user puts the timeline away.
    fireEvent.click(screen.getByTestId('thread-timeline-toggle'));
    expect(
      screen.getByTestId('thread-timeline-toggle'),
    ).toHaveAttribute('aria-expanded', 'false');
    const collapsed = pressKey('ArrowLeft');
    expect(collapsed.defaultPrevented).toBe(false);
    // Re-expanding runs the effect again and reattaches the listener:
    // ArrowRight is owned again and lands the playhead on the newest
    // message (a step from the pre-collapse position, or a clamp if the
    // playhead already sits there — 240px either way).
    fireEvent.click(screen.getByTestId('thread-timeline-toggle'));
    await screen.findAllByTestId('thread-timeline-dot');
    expect(pressKey('ArrowRight').defaultPrevented).toBe(true);
    expect(playheadPx()).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
  });

  it('binds the keydown listener once for the expanded session, not once per message-list identity', async () => {
    // The step handlers read the sorted lists through refs precisely so the
    // listener does NOT have to be re-bound as messages load and refetch.
    // That only holds if the commit callback they close over is stable too:
    // a listener re-bound in a later commit is, until that commit's effects
    // flush, still the PREVIOUS commit's closure — and React defers effect
    // flushes to a separate task whenever a commit overruns the scheduler's
    // frame budget, which is routine under load. A keypress landing in that
    // window would then be resolved against one message list and committed
    // against another (or against the empty pre-load list, dropping the step
    // silently). Binding once removes the window by construction.
    const bindings: string[] = [];
    const realAdd = window.addEventListener.bind(window);
    const realRemove = window.removeEventListener.bind(window);
    const addSpy = vi
      .spyOn(window, 'addEventListener')
      .mockImplementation(((type: string, ...rest: unknown[]) => {
        if (type === 'keydown') {
          bindings.push('add');
        }
        return (realAdd as (...args: unknown[]) => void)(type, ...rest);
      }) as typeof window.addEventListener);
    const removeSpy = vi
      .spyOn(window, 'removeEventListener')
      .mockImplementation(((type: string, ...rest: unknown[]) => {
        if (type === 'keydown') {
          bindings.push('remove');
        }
        return (realRemove as (...args: unknown[]) => void)(type, ...rest);
      }) as typeof window.removeEventListener);
    try {
      renderThreeLargeTurns();
      await screen.findAllByTestId('thread-timeline-dot');
      // Settle the mount anchor: every render the message load produces has
      // happened by now, so any per-identity re-binding would be recorded.
      await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
      expect(bindings).toEqual(['add']);
    } finally {
      addSpy.mockRestore();
      removeSpy.mockRestore();
    }
  });
});
