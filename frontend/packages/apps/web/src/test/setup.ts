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
