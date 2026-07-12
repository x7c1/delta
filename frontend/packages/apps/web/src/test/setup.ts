import '@testing-library/jest-dom/vitest';

// jsdom does not implement ResizeObserver, which @tanstack/react-virtual uses
// (via `measureElement`) to read each rendered session row's real height.
// Provide an inert stub so the navigator mounts without throwing. It never
// reports a resize, so rows keep their estimated size in jsdom — tests that
// care about the windowed range drive it explicitly rather than via layout.
if (typeof globalThis.ResizeObserver === 'undefined') {
  class ResizeObserverStub implements ResizeObserver {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  }
  globalThis.ResizeObserver =
    ResizeObserverStub as unknown as typeof ResizeObserver;
}

// jsdom performs no layout, so every element reports `offsetHeight`/`offsetWidth`
// of 0. The virtualizer reads the scroll element's `offsetHeight` to size its
// window; a 0-height viewport yields an empty window and no session rows render
// under test. Give elements a non-zero default viewport so the virtualizer
// renders a realistic window in jsdom (a real browser reports true sizes).
const VIRTUAL_VIEWPORT_HEIGHT = 600;
const VIRTUAL_VIEWPORT_WIDTH = 288; // matches the navigator pane width (w-72)

if (
  Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetHeight')
    ?.configurable !== false
) {
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
    configurable: true,
    get() {
      return VIRTUAL_VIEWPORT_HEIGHT;
    },
  });
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', {
    configurable: true,
    get() {
      return VIRTUAL_VIEWPORT_WIDTH;
    },
  });
}

// jsdom does not implement `Element.scrollTo`, but call sites that animate a
// scroll (e.g. the thread-timeline playhead's edge re-centre) use it to opt
// into the browser's smooth-scroll animation. Without a stub these call sites
// throw `scrollTo is not a function` and crash unrelated tests. Mirror the
// requested `left`/`top` onto `scrollLeft`/`scrollTop` so tests that assert on
// the post-scroll position keep working; tests that care about the smooth
// contract install their own `vi.fn()` over this default to spy on calls.
//
// After mirroring, dispatch a `scroll` event, as a real browser does when a
// programmatic scroll changes the offset. `@tanstack/react-virtual` learns its
// scroll position ONLY through that event (its `scrollToIndex` calls
// `scrollTo`, then waits for the resulting `scroll` to re-window) — without the
// dispatch a virtualized list never re-windows to a programmatic scroll target
// in jsdom, so transcript timeline-jump / breadcrumb-landing tests could never
// mount an off-window row. The mirrored value is set before dispatch so any
// listener reads the post-scroll offset.
if (typeof HTMLElement.prototype.scrollTo !== 'function') {
  Object.defineProperty(HTMLElement.prototype, 'scrollTo', {
    configurable: true,
    writable: true,
    value: function scrollTo(
      this: HTMLElement,
      options?: ScrollToOptions | number,
      maybeY?: number,
    ): void {
      let moved = false;
      if (typeof options === 'object' && options !== null) {
        if (typeof options.left === 'number') {
          this.scrollLeft = options.left;
          moved = true;
        }
        if (typeof options.top === 'number') {
          this.scrollTop = options.top;
          moved = true;
        }
      } else if (typeof options === 'number') {
        this.scrollLeft = options;
        moved = true;
        if (typeof maybeY === 'number') {
          this.scrollTop = maybeY;
        }
      }
      if (moved) {
        // Dispatch asynchronously, as a real browser does — a programmatic
        // scroll fires its `scroll` event on a later task, never synchronously
        // inside the caller. A synchronous dispatch here would re-enter
        // `@tanstack/react-virtual`'s `flushSync` while React is mid-lifecycle
        // (its `scrollToIndex` calls `scrollTo` from an effect), producing a
        // spurious "flushSync was called from inside a lifecycle method"
        // warning that never occurs in the browser. A microtask keeps the
        // re-window within the same `act()`/`waitFor` flush the tests await.
        queueMicrotask(() => this.dispatchEvent(new Event('scroll')));
      }
    },
  });
}
