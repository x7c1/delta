import '@testing-library/jest-dom/vitest';

// jsdom does not implement IntersectionObserver, which the navigator's
// scroll-to-load sentinel constructs. Provide an inert stub so components that
// observe an element mount without throwing. It never reports an intersection,
// so unit tests drive pagination explicitly rather than via simulated scroll.
if (typeof globalThis.IntersectionObserver === 'undefined') {
  class IntersectionObserverStub implements IntersectionObserver {
    readonly root: Element | null = null;
    readonly rootMargin: string = '';
    readonly thresholds: ReadonlyArray<number> = [];
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
    takeRecords(): IntersectionObserverEntry[] {
      return [];
    }
  }
  globalThis.IntersectionObserver =
    IntersectionObserverStub as unknown as typeof IntersectionObserver;
}
