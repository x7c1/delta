import { test, expect, type Locator, type Page } from '@playwright/test';
import { SESSION_ID, SESSION_ID_4 } from '@delta/api-mocks';
import type { StatusSnapshot } from '@delta/wire-gen';
import { useManualEventControl, emitEvent } from './support/app';

/**
 * The usage snapshot (`status_updated`) drives two pieces of the UI: the
 * account-wide rate-limit meters in the navigator footer, and the focused
 * session's context-window usage as a thin fill along the composer card's top
 * edge. This drives mock mode, feeds a snapshot through the fake event source,
 * and asserts both reflect it — then feeds a snapshot WITHOUT rate limits and
 * asserts the footer rows disappear (no empty bars).
 *
 * The second test is the provider-scoping one: rate limits belong to an
 * account, so the footer must show the FOCUSED session's provider's windows and
 * never another provider's. Both providers are driven here against the real
 * layout, with no `provider === …` branch anywhere in the app — the store keys
 * by the snapshot's provider and the rows label themselves from each window's
 * duration.
 */

/** A full snapshot, with the fields not under test defaulted to null. */
function snapshot(overrides: Partial<StatusSnapshot>): StatusSnapshot {
  return {
    provider: 'claude',
    model_id: null,
    model_display_name: null,
    context_used_percentage: null,
    context_window_size: null,
    context_current_usage: null,
    total_input_tokens: null,
    rate_limits: null,
    total_cost_usd: null,
    current_dir: null,
    ...overrides,
  };
}

const FIVE_HOURS = 5 * 60 * 60;
const SEVEN_DAYS = 7 * 24 * 60 * 60;

/** The session card whose launch-time branch is `branch`. */
function rowByBranch(page: Page, branch: string): Locator {
  return page.getByTestId('session-node').filter({ hasText: branch });
}

/**
 * Scroll the windowed session list until `row` is mounted. One screen per
 * attempt — never a jump to `scrollHeight`, which would march the window past a
 * row that was already reachable (the Codex seed sits on page 2 with filler
 * sessions below it).
 */
async function scrollUntilVisible(page: Page, row: Locator): Promise<void> {
  const scrollBody = page.getByTestId('sessions-list').locator('..');
  await expect(async () => {
    if ((await row.count()) === 0) {
      await scrollBody.evaluate((el) => {
        el.scrollTop += el.clientHeight;
      });
    }
    await expect(row).toBeVisible({ timeout: 500 });
  }).toPass();
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
      rate_limits: [
        {
          duration_seconds: FIVE_HOURS,
          used_percentage: 35,
          resets_at: now + 2 * 3600 + 13 * 60 + 30,
        },
        {
          duration_seconds: SEVEN_DAYS,
          used_percentage: 8,
          resets_at: now + 5 * 86400 + 4 * 3600 + 30 * 60,
        },
      ],
    }),
  });

  // Footer: both rate-limit meters render with their percentages and resets.
  // The `5h` / `7d` row identities come from each window's own duration.
  await expect(page.getByTestId('rate-limit-5h-pct')).toHaveText('35%');
  await expect(page.getByTestId('rate-limit-5h-reset')).toHaveText('↻ 02h13m');
  await expect(page.getByTestId('rate-limit-7d-pct')).toHaveText('8%');
  await expect(page.getByTestId('rate-limit-7d-reset')).toHaveText('↻ 05d04h');

  // Composer: the top-edge context bar fills to the focused session's usage.
  const fill = page.getByTestId('composer-context-fill');
  await expect(fill).toHaveAttribute('aria-valuenow', '47');
  await expect(page.getByTestId('composer-context-bar')).toContainText('47%');

  // A later snapshot reporting NO windows hides both footer rows (no empty
  // bars). An empty list is a statement — "this account has none" — unlike the
  // `null` that means "this frame says nothing about them".
  await emitEvent(page, {
    kind: 'status_updated',
    session_id: SESSION_ID,
    snapshot: snapshot({ context_used_percentage: 47, rate_limits: [] }),
  });
  await expect(page.getByTestId('rate-limits')).toHaveCount(0);
  await expect(page.getByTestId('rate-limit-5h')).toHaveCount(0);
  await expect(page.getByTestId('rate-limit-7d')).toHaveCount(0);
});

test('rate limits follow the focused session provider, with no cross-provider leak', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await expect(page.getByTestId('composer-card')).toBeVisible();

  const now = Math.floor(Date.now() / 1000);
  // Both accounts report, on their own sessions. Codex's windows deliberately
  // differ in length from Claude's, so the rows below can only be right if the
  // labels came from the data.
  await emitEvent(page, {
    kind: 'status_updated',
    session_id: SESSION_ID,
    snapshot: snapshot({
      rate_limits: [
        {
          duration_seconds: FIVE_HOURS,
          used_percentage: 35,
          resets_at: now + 3600,
        },
      ],
    }),
  });
  await emitEvent(page, {
    kind: 'status_updated',
    session_id: SESSION_ID_4,
    snapshot: snapshot({
      provider: 'codex',
      // A Codex account's `primary` / `secondary` pair, as its adapter reports
      // them: identified by `windowDurationMins`, here 24h and 30m.
      rate_limits: [
        {
          duration_seconds: 24 * 60 * 60,
          used_percentage: 61,
          resets_at: now + 3600,
        },
        { duration_seconds: 30 * 60, used_percentage: 12, resets_at: null },
      ],
    }),
  });

  // The Claude session is focused, so only Claude's window is shown — the Codex
  // update that arrived after it must not have replaced the display.
  await expect(page.getByTestId('rate-limit-5h-pct')).toHaveText('35%');
  await expect(page.getByTestId('rate-limit-1d')).toHaveCount(0);
  await expect(page.getByTestId('rate-limit-30m')).toHaveCount(0);

  // Focus the Codex session: the footer swaps to its account's windows, labeled
  // from durations the app has no hardcoded rows for.
  const codexRow = rowByBranch(page, 'feat/codex-adapter');
  await scrollUntilVisible(page, codexRow);
  await codexRow.click();

  await expect(page.getByTestId('rate-limit-1d-pct')).toHaveText('61%');
  await expect(page.getByTestId('rate-limit-30m-pct')).toHaveText('12%');
  await expect(page.getByTestId('rate-limit-5h')).toHaveCount(0);

  // The Codex token-usage frame: it fills this session's context bar and says
  // NOTHING about rate limits, so the account's rows must survive it. A
  // provider that reports the two separately would otherwise wipe its own
  // footer on every turn.
  await emitEvent(page, {
    kind: 'status_updated',
    session_id: SESSION_ID_4,
    snapshot: snapshot({ provider: 'codex', context_used_percentage: 25 }),
  });
  await expect(page.getByTestId('composer-context-fill')).toHaveAttribute(
    'aria-valuenow',
    '25',
  );
  await expect(page.getByTestId('rate-limit-1d-pct')).toHaveText('61%');
});
