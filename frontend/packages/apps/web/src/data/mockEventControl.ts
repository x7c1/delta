import type { SessionEvent } from '@delta/wire-gen';
import {
  FakeEventSource,
  mockApi,
  type FakeEventSourceOptions,
} from '@delta/api-mocks';

/**
 * Test seam for the mock-mode event source.
 *
 * In mock mode the app drives its live channel from a {@link FakeEventSource}.
 * By default that source auto-replays a scripted sequence on a 1500 ms timer —
 * fine for human-facing dev, but too slow and unobservable for an automated
 * end-to-end run. This module lets an external driver (a Playwright spec)
 * override the source's options and, when auto-play is disabled, feed events one
 * at a time interleaved with user actions.
 *
 * The seam is engaged only when a test sets {@link MOCK_EVENT_CONTROL_KEY} on
 * `window` before the app boots. With no override present the source behaves
 * exactly as in production dev, so this has zero effect on the shipped app.
 */

/** `window` property a test sets (pre-boot) to override the fake event source. */
export const MOCK_EVENT_CONTROL_KEY = '__deltaMockEventControl';

/** `window` property the app sets so a test can drive events manually. */
export const MOCK_EVENT_SOURCE_KEY = '__deltaMockEventSource';

/** Overrides a test may set on `window[MOCK_EVENT_CONTROL_KEY]` before boot. */
export interface MockEventControl {
  /** Delay between scripted events, in ms. */
  intervalMs?: number;
  /** Replace the scripted sequence. */
  script?: SessionEvent[];
  /** When `false`, do not auto-replay; the test drives events via `emit`. */
  autoPlay?: boolean;
}

declare global {
  interface Window {
    [MOCK_EVENT_CONTROL_KEY]?: MockEventControl;
    [MOCK_EVENT_SOURCE_KEY]?: FakeEventSource;
  }
}

/** Read any test-provided overrides for the fake event source. */
function readControl(): MockEventControl {
  if (typeof window === 'undefined') {
    return {};
  }
  return window[MOCK_EVENT_CONTROL_KEY] ?? {};
}

/**
 * Build the mock-mode {@link FakeEventSource}, honouring any test overrides. If
 * the test disabled auto-play it also publishes the source on `window` so the
 * test can feed events with {@link FakeEventSource.emit}.
 */
export function createMockEventSource(): FakeEventSource {
  const control = readControl();
  const options: FakeEventSourceOptions = {};
  if (control.intervalMs !== undefined) {
    options.intervalMs = control.intervalMs;
  }
  if (control.script !== undefined) {
    options.script = control.script;
  }
  if (control.autoPlay !== undefined) {
    options.autoPlay = control.autoPlay;
  }

  const source = new FakeEventSource(options);
  // Mirror every event into the shared mock REST store BEFORE the app's own
  // subscriber sees it (listeners fire in subscription order, and this one is
  // registered first): the app reacts to events by refetching, and the
  // refetch must observe the state the event implies — e.g. `turn_completed`
  // resolves the session's open sends, exactly as the real backend's
  // ingestion would have during the turn.
  source.onEvent((event) => mockApi.applyEvent(event));
  if (typeof window !== 'undefined') {
    window[MOCK_EVENT_SOURCE_KEY] = source;
  }
  return source;
}
