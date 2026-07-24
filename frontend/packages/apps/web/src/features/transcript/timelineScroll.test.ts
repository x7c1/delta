import { describe, expect, it, vi } from 'vitest';
import {
  SCROLL_DOM_READY_TIMEOUT_MS,
  nearestRenderedNeighborUuid,
  scheduleScrollAfterRender,
} from './timelineScroll';

describe('ThreadTimelineOverlay scheduleScrollAfterRender DOM-ready wait (v11 Improvement 2)', () => {
  // v10's cross-lane jump deferred the scroll a single rAF; when the
  // subthread switch re-render took 2+ frames (which it usually does),
  // querySelector found no target and the scroll silently dropped. The
  // new behaviour polls each rAF until the uuid is in the DOM, capped
  // by SCROLL_DOM_READY_TIMEOUT_MS.

  it('waits across multiple rAFs for the target element, then scrolls when it appears', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    // Capture rAF callbacks so we can drive them one frame at a time.
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* cancellation is exercised elsewhere */
    }) as typeof window.cancelAnimationFrame;
    try {
      const container = document.createElement('div');
      document.body.appendChild(container);
      try {
        const cancel = scheduleScrollAfterRender(container, 'late-uuid');
        // First two ticks: target absent, scroll must NOT fire.
        expect(rafCallbacks).toHaveLength(1);
        let cb = rafCallbacks.shift()!;
        cb(performance.now());
        expect(scrollIntoView).not.toHaveBeenCalled();
        // Polling re-queues itself for the next frame.
        expect(rafCallbacks).toHaveLength(1);
        cb = rafCallbacks.shift()!;
        cb(performance.now());
        expect(scrollIntoView).not.toHaveBeenCalled();
        expect(rafCallbacks).toHaveLength(1);
        // Third tick: target is now in the DOM (mirrors a real cross-lane
        // re-render that took 3 frames). The scroll fires this tick.
        const target = document.createElement('article');
        target.setAttribute('data-message-uuid', 'late-uuid');
        container.appendChild(target);
        cb = rafCallbacks.shift()!;
        cb(performance.now());
        expect(scrollIntoView).toHaveBeenCalledTimes(1);
        expect(scrollIntoView.mock.instances[0]).toBe(target);
        cancel();
      } finally {
        document.body.removeChild(container);
      }
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
    }
  });

  it('gives up after SCROLL_DOM_READY_TIMEOUT_MS when the target never appears, but still settles exactly once', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    const originalPerfNow = window.performance.now;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* not exercised here */
    }) as typeof window.cancelAnimationFrame;
    // Drive performance.now so the loop crosses the timeout deterministically
    // — first tick at t=0, second at t=TIMEOUT+1 ms.
    let nowValue = 1_000;
    window.performance.now = (() => nowValue) as typeof performance.now;
    try {
      const container = document.createElement('div');
      document.body.appendChild(container);
      try {
        // No matching child is ever appended. onScroll must never fire (no
        // scroll happens), but onSettled MUST fire on timeout so a caller's
        // in-flight guard counter is released rather than latching forever.
        const onScroll = vi.fn();
        const onSettled = vi.fn();
        const cancel = scheduleScrollAfterRender(
          container,
          'never-arrives',
          onScroll,
          onSettled,
        );
        expect(rafCallbacks).toHaveLength(1);
        let cb = rafCallbacks.shift()!;
        nowValue = 1_000; // first tick: t=0 elapsed
        cb(nowValue);
        expect(scrollIntoView).not.toHaveBeenCalled();
        expect(onSettled).not.toHaveBeenCalled();
        expect(rafCallbacks).toHaveLength(1);
        // Advance past the timeout and tick again: the loop bails without
        // re-queuing and without scrolling — but settles.
        nowValue = 1_000 + SCROLL_DOM_READY_TIMEOUT_MS + 1;
        cb = rafCallbacks.shift()!;
        cb(nowValue);
        expect(scrollIntoView).not.toHaveBeenCalled();
        expect(onScroll).not.toHaveBeenCalled();
        expect(onSettled).toHaveBeenCalledTimes(1);
        expect(rafCallbacks).toHaveLength(0);
        // A cancel after the timeout must NOT settle a second time.
        cancel();
        expect(onSettled).toHaveBeenCalledTimes(1);
      } finally {
        document.body.removeChild(container);
      }
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
      window.performance.now = originalPerfNow;
    }
  });

  it('re-calls scrollIntoView after one animation frame so post-layout scroll-margin-top is honoured', async () => {
    // Cross-lane jumps mount a freshly-rendered article whose computed
    // scroll-margin-top resolves only after the first layout pass. The
    // initial scrollIntoView therefore scrolls with margin=0 and the
    // article lands behind the floating top overlay. Scheduling a second
    // scrollIntoView in the next animation frame guarantees the browser
    // recomputes the scroll with the resolved margin and the article lands
    // just below the overlay.
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* cancellation is exercised elsewhere */
    }) as typeof window.cancelAnimationFrame;
    try {
      const container = document.createElement('div');
      document.body.appendChild(container);
      try {
        // Target article is already present so the polling loop fires its
        // run() body on the very first tick — mirroring the cross-lane jump
        // case after the new subthread's article has just been mounted.
        const target = document.createElement('article');
        target.setAttribute('data-message-uuid', 'reflow-uuid');
        container.appendChild(target);
        const cancel = scheduleScrollAfterRender(container, 'reflow-uuid');
        // First tick: target is in the DOM, the initial scrollIntoView
        // fires, and the helper schedules a follow-up rAF for the
        // post-layout re-scroll.
        expect(rafCallbacks).toHaveLength(1);
        const initialTick = rafCallbacks.shift()!;
        initialTick(performance.now());
        expect(scrollIntoView).toHaveBeenCalledTimes(1);
        expect(scrollIntoView.mock.instances[0]).toBe(target);
        // The follow-up rAF is queued; until it fires, no second scroll.
        expect(rafCallbacks).toHaveLength(1);
        // Drive the follow-up frame: the second scrollIntoView fires on the
        // same article. After this, layout has resolved scroll-margin-top
        // and the browser scrolls honouring the reserved top region.
        const reflowTick = rafCallbacks.shift()!;
        reflowTick(performance.now());
        expect(scrollIntoView).toHaveBeenCalledTimes(2);
        expect(scrollIntoView.mock.instances[1]).toBe(target);
        cancel();
      } finally {
        document.body.removeChild(container);
      }
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
    }
  });

  it('runs the onTimeout fallback (once, before settle) when the target never renders — and never on a successful landing', async () => {
    // A cross-lane jump to a renders-nothing target (e.g. a tool_result
    // carrier) polls to the DOM-ready timeout without ever scrolling. The
    // timeout leg must run the caller's deterministic fallback (scroll to a
    // rendering neighbor) BEFORE releasing the guard via onSettled — and never
    // on the success leg.
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    const originalPerfNow = window.performance.now;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* not exercised here */
    }) as typeof window.cancelAnimationFrame;
    let nowValue = 1_000;
    window.performance.now = (() => nowValue) as typeof performance.now;
    try {
      const container = document.createElement('div');
      document.body.appendChild(container);
      try {
        const onSettled = vi.fn();
        const onTimeout = vi.fn(() => {
          // The fallback fires while the schedule is still un-settled.
          expect(onSettled).not.toHaveBeenCalled();
        });
        const cancel = scheduleScrollAfterRender(
          container,
          'never-arrives',
          undefined,
          onSettled,
          onTimeout,
        );
        // First tick: target absent, still within budget — no fallback yet.
        let cb = rafCallbacks.shift()!;
        nowValue = 1_000;
        cb(nowValue);
        expect(onTimeout).not.toHaveBeenCalled();
        expect(onSettled).not.toHaveBeenCalled();
        // Cross the timeout: the fallback runs exactly once, then settle.
        nowValue = 1_000 + SCROLL_DOM_READY_TIMEOUT_MS + 1;
        cb = rafCallbacks.shift()!;
        cb(nowValue);
        expect(onTimeout).toHaveBeenCalledTimes(1);
        expect(onSettled).toHaveBeenCalledTimes(1);
        // A late cancel neither re-fires the fallback nor re-settles.
        cancel();
        expect(onTimeout).toHaveBeenCalledTimes(1);
        expect(onSettled).toHaveBeenCalledTimes(1);
      } finally {
        document.body.removeChild(container);
      }
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
      window.performance.now = originalPerfNow;
    }
  });

  it('does NOT run onTimeout when the target renders in time (success leg)', async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView =
      scrollIntoView as Element['scrollIntoView'];
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {
      /* not exercised here */
    }) as typeof window.cancelAnimationFrame;
    try {
      const container = document.createElement('div');
      document.body.appendChild(container);
      try {
        const target = document.createElement('article');
        target.setAttribute('data-message-uuid', 'here');
        container.appendChild(target);
        const onTimeout = vi.fn();
        const onSettled = vi.fn();
        const cancel = scheduleScrollAfterRender(
          container,
          'here',
          undefined,
          onSettled,
          onTimeout,
        );
        const cb = rafCallbacks.shift()!;
        cb(performance.now());
        expect(scrollIntoView).toHaveBeenCalledTimes(1);
        expect(onTimeout).not.toHaveBeenCalled();
        expect(onSettled).toHaveBeenCalledTimes(1);
        cancel();
      } finally {
        document.body.removeChild(container);
      }
    } finally {
      window.requestAnimationFrame = originalRaf;
      window.cancelAnimationFrame = originalCancelRaf;
    }
  });
});

describe('nearestRenderedNeighborUuid (cross-lane timeout fallback target)', () => {
  function makeContainer(renderedUuids: string[]): HTMLElement {
    const container = document.createElement('div');
    for (const uuid of renderedUuids) {
      const article = document.createElement('article');
      article.setAttribute('data-message-uuid', uuid);
      container.appendChild(article);
    }
    return container;
  }

  // A single lane's timeline order; the middle entry renders nothing.
  const sorted = [
    { uuid: 'a', threadId: 1 as const },
    { uuid: 'b', threadId: 1 as const },
    { uuid: 'target', threadId: 1 as const },
    { uuid: 'd', threadId: 1 as const },
    { uuid: 'e', threadId: 1 as const },
  ];

  it('returns the closest rendering neighbor, preferring the tail-ward one on a tie', () => {
    // Both immediate neighbors render: distance ties, tail-ward (`d`) wins so
    // the pane lands just past the non-rendering carrier.
    const container = makeContainer(['a', 'b', 'd', 'e']);
    expect(
      nearestRenderedNeighborUuid(container, sorted, 'target', 1),
    ).toBe('d');
  });

  it('reaches past a non-rendering immediate neighbor to the next rendering one', () => {
    // `d` (immediate tail-ward) is NOT rendered, so the nearest rendering
    // neighbor is `b` (immediate head-ward) at the same distance.
    const container = makeContainer(['a', 'b', 'e']);
    expect(
      nearestRenderedNeighborUuid(container, sorted, 'target', 1),
    ).toBe('b');
  });

  it('ignores neighbors in other lanes', () => {
    const mixed = [
      { uuid: 'x-other', threadId: 2 as const },
      { uuid: 'target', threadId: 1 as const },
      { uuid: 'y-lane', threadId: 1 as const },
    ];
    // The head-ward neighbor is in another lane; only the tail-ward same-lane
    // `y-lane` is eligible.
    const container = makeContainer(['x-other', 'y-lane']);
    expect(
      nearestRenderedNeighborUuid(container, mixed, 'target', 1),
    ).toBe('y-lane');
  });

  it('returns null when no lane message is rendered (caller falls back to lane top)', () => {
    const container = makeContainer([]);
    expect(
      nearestRenderedNeighborUuid(container, sorted, 'target', 1),
    ).toBeNull();
  });

  it('returns null when the target is not in the sorted list', () => {
    const container = makeContainer(['a', 'b']);
    expect(
      nearestRenderedNeighborUuid(container, sorted, 'missing', 1),
    ).toBeNull();
  });
});
