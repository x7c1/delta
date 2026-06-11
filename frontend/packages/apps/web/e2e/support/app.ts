import type { Page } from '@playwright/test';
import type { SessionEvent } from '@delta/wire-gen';
import {
  MOCK_EVENT_CONTROL_KEY,
  MOCK_EVENT_SOURCE_KEY,
} from '../../src/data/mockEventControl';

/**
 * End-to-end support helpers.
 *
 * The app's mock-mode event source reads optional overrides from
 * `window[MOCK_EVENT_CONTROL_KEY]` before it boots (see `mockEventControl.ts`).
 * These helpers install that override via `addInitScript` — which runs before
 * any page script on every navigation, including reloads — so the source never
 * auto-plays its 1500 ms script and the spec drives events explicitly. This
 * keeps every run fast and deterministic.
 */

/**
 * Put the mock event source under manual control for the rest of the page's
 * life. Must be called before the first `goto`. Survives reloads.
 */
export async function useManualEventControl(page: Page): Promise<void> {
  await page.addInitScript((key: string) => {
    (window as unknown as Record<string, unknown>)[key] = { autoPlay: false };
  }, MOCK_EVENT_CONTROL_KEY);
}

/**
 * Feed one scripted event to the app, exactly as the live channel would. Only
 * valid under {@link useManualEventControl}, after the app has mounted (which
 * is what publishes the source on `window`).
 */
export async function emitEvent(
  page: Page,
  event: SessionEvent,
): Promise<void> {
  await page.evaluate(
    ([key, evt]) => {
      const source = (
        window as unknown as Record<string, { emit(e: unknown): void } | undefined>
      )[key];
      if (!source) {
        throw new Error('mock event source is not available yet');
      }
      source.emit(evt);
    },
    [MOCK_EVENT_SOURCE_KEY, event] as const,
  );
}
