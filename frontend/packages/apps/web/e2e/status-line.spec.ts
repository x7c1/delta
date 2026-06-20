import { test, expect } from '@playwright/test';
import { SESSION_ID } from '@delta/api-mocks';
import type { StatusSnapshot } from '@delta/wire-gen';
import { useManualEventControl, emitEvent } from './support/app';

/**
 * The status-line snapshot (`status_updated`) drives two pieces of the UI: the
 * account-wide 5h/7d rate-limit meters in the navigator footer, and the focused
 * session's context-window usage as a thin fill along the composer card's top
 * edge. This drives mock mode, feeds a snapshot through the fake event source,
 * and asserts both reflect it — then feeds a snapshot WITHOUT rate limits and
 * asserts the footer rows disappear (no empty bars).
 */

/** A full snapshot, with the fields not under test defaulted to null. */
function snapshot(overrides: Partial<StatusSnapshot>): StatusSnapshot {
  return {
    model_id: null,
    model_display_name: null,
    context_used_percentage: null,
    context_window_size: null,
    context_current_usage: null,
    total_input_tokens: null,
    five_hour: null,
    seven_day: null,
    total_cost_usd: null,
    current_dir: null,
    ...overrides,
  };
}

test('the footer meters and composer context bar reflect a status snapshot', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  // The open session ("sess-mock-1") is auto-focused, so its composer is shown.
  await expect(page.getByTestId('composer-card')).toBeVisible();

  // No snapshot yet: neither the rate-limit rows nor the context bar exist.
  await expect(page.getByTestId('rate-limits')).toHaveCount(0);
  await expect(page.getByTestId('composer-context-bar')).toHaveCount(0);

  const now = Math.floor(Date.now() / 1000);
  // The countdown floors to its smallest shown unit (minutes for the 5h
  // window, hours for the 7d), and a few seconds elapse between capturing
  // `now` here and the footer rendering. Seat each target in the MIDDLE of its
  // floor bucket (+30s for the 5h minute, +30m for the 7d hour) so drift cannot
  // tip it across a boundary and make the exact-text assertion flaky.
  await emitEvent(page, {
    kind: 'status_updated',
    session_id: SESSION_ID,
    snapshot: snapshot({
      context_used_percentage: 47,
      five_hour: {
        used_percentage: 35,
        resets_at: now + 2 * 3600 + 13 * 60 + 30,
      },
      seven_day: {
        used_percentage: 8,
        resets_at: now + 5 * 86400 + 4 * 3600 + 30 * 60,
      },
    }),
  });

  // Footer: both rate-limit meters render with their percentages and resets.
  await expect(page.getByTestId('rate-limit-5h-pct')).toHaveText('035%');
  await expect(page.getByTestId('rate-limit-5h-reset')).toHaveText('↻ 02h13m');
  await expect(page.getByTestId('rate-limit-7d-pct')).toHaveText('008%');
  await expect(page.getByTestId('rate-limit-7d-reset')).toHaveText('↻ 05d04h');

  // Composer: the top-edge context bar fills to the focused session's usage.
  const fill = page.getByTestId('composer-context-fill');
  await expect(fill).toHaveAttribute('aria-valuenow', '47');
  await expect(page.getByTestId('composer-context-bar')).toContainText('47%');

  // A later snapshot WITHOUT rate limits hides both footer rows (no empty bars).
  await emitEvent(page, {
    kind: 'status_updated',
    session_id: SESSION_ID,
    snapshot: snapshot({ context_used_percentage: 47 }),
  });
  await expect(page.getByTestId('rate-limits')).toHaveCount(0);
  await expect(page.getByTestId('rate-limit-5h')).toHaveCount(0);
  await expect(page.getByTestId('rate-limit-7d')).toHaveCount(0);
});
