import { test, expect } from '@playwright/test';
import { SESSION_ID } from '@delta/api-mocks';
import { emitEvent, useManualEventControl } from './support/app';

/**
 * The pending/"running" indicator appears while a turn is in progress and
 * clears once the turn completes.
 *
 * Driving the event source manually, this sends a message (creating a queued
 * pending item, send id 1), then feeds `turn_started` — which promotes the item
 * to in-progress and shows the navigator's "running" indicator — then
 * `turn_completed`, which clears both the item and the indicator.
 */
test('the running indicator appears then clears as a turn completes', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  await page.getByRole('textbox').fill('drive a turn');
  await page.getByRole('button', { name: 'Send' }).click();

  // Optimistically queued, and not yet running.
  await expect(page.getByTestId('pending-item')).toHaveCount(1);
  const running = page.getByText('running', { exact: true });
  await expect(running).toHaveCount(0);

  // The first mock send is assigned id 1; the turn starts against it.
  await emitEvent(page, {
    kind: 'turn_started',
    session_id: SESSION_ID,
    pending_send_id: 1,
    matched_uuid: null,
  });
  await expect(running).toBeVisible();

  await emitEvent(page, {
    kind: 'turn_completed',
    session_id: SESSION_ID,
    stop_reason: null,
  });
  await expect(page.getByTestId('pending-item')).toHaveCount(0);
  await expect(running).toHaveCount(0);
});

/**
 * Interrupting an in-flight turn clears the pending chip just like completion.
 *
 * On interrupt Claude's `Stop` hook never fires, so no `turn_completed` arrives;
 * the backend instead detects the `[Request interrupted by user]` transcript
 * line and emits `turn_interrupted`. This drives the same drain: the running
 * indicator and the optimistic pending item both clear.
 */
test('the running indicator clears when a turn is interrupted', async ({
  page,
}) => {
  await useManualEventControl(page);
  await page.goto('/');

  await page.getByRole('textbox').fill('drive a turn');
  await page.getByRole('button', { name: 'Send' }).click();

  await expect(page.getByTestId('pending-item')).toHaveCount(1);
  const running = page.getByText('running', { exact: true });
  await expect(running).toHaveCount(0);

  await emitEvent(page, {
    kind: 'turn_started',
    session_id: SESSION_ID,
    pending_send_id: 1,
    matched_uuid: null,
  });
  await expect(running).toBeVisible();

  // The user interrupts instead of the turn completing.
  await emitEvent(page, {
    kind: 'turn_interrupted',
    session_id: SESSION_ID,
  });
  await expect(page.getByTestId('pending-item')).toHaveCount(0);
  await expect(running).toHaveCount(0);
});
