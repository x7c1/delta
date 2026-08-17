import { test, expect, type Locator, type Page } from '@playwright/test';
import { useManualEventControl } from './support/app';

/**
 * The comms-log pane: the right-pane window a headless provider's session gets
 * in place of a terminal.
 *
 * A Codex session has no terminal to attach, so without this pane there is
 * nothing at all in the right column — no window into what the agent is doing.
 * This spec walks the user's path to it against the real stylesheet and the real
 * layout: focus the Codex session, open the pane from its own toggle, and read
 * the frames.
 *
 * What it proves that a jsdom test cannot: the pane is actually *visible* in the
 * right column (a real viewport, real CSS), the frames are laid out one per row
 * with direction and method legible without expanding anything, and expanding
 * one reveals its payload. The frames come from the mock-mode scripted exchange
 * (`mockCommsFrames`), since MSW cannot mock a WebSocket — the same reason the
 * `/ws` channel is a fake in this suite.
 */

/** The session card whose launch-time branch is `branch`. */
function rowByBranch(page: Page, branch: string): Locator {
  return page.getByTestId('session-node').filter({ hasText: branch });
}

/**
 * Scroll the windowed session list until `row` is mounted. One screen per
 * attempt — never a jump to `scrollHeight`, which would march the window past a
 * row that was already reachable (see `provider-marker.spec.ts` for the full
 * reasoning; the Codex seed sits on page 2 with filler sessions below it).
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

test('a Codex session opens a comms pane listing its frames in order', async ({
  page,
}) => {
  await useManualEventControl(page);
  // A wide viewport so the right pane is the persistent column (>= lg).
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');

  // Focus the Codex session (it sorts onto page 2 of the windowed list).
  const codexRow = rowByBranch(page, 'feat/codex-adapter');
  await scrollUntilVisible(page, codexRow);
  await codexRow.click();

  // A Codex session is offered the comms toggle, never the terminal one: the
  // choice comes from the provider's capability profile.
  const commsToggle = page.getByRole('button', { name: 'Comms' });
  await expect(commsToggle).toBeVisible();
  await expect(page.getByRole('button', { name: 'Terminal' })).toHaveCount(0);

  await commsToggle.click();

  const pane = page.getByTestId('comms-pane');
  await expect(pane).toBeVisible();

  // The frames render one per row, in server order — the sequence is the signal.
  const frames = page.getByTestId('comms-frame');
  await expect(frames.first()).toBeVisible();
  const count = await frames.count();
  expect(count).toBeGreaterThan(4);

  // Direction is on each row and both directions are present, so a reader can
  // tell Delta's calls from the server's pushes at a glance.
  const directions = await frames.evaluateAll((rows) =>
    rows.map((row) => (row as HTMLElement).dataset.direction),
  );
  expect(directions).toContain('to_agent');
  expect(directions).toContain('from_agent');

  // The methods are visible without expanding anything, and read as the real
  // exchange: the launch first, the turn's completion last.
  const methods = await page
    .getByTestId('comms-frame-method')
    .evaluateAll((cells) => cells.map((cell) => cell.textContent?.trim()));
  expect(methods[0]).toBe('thread/start');
  expect(methods).toContain('turn/completed');

  // The scripted streaming burst folds into one group row — the flood-control
  // that keeps requests and approvals visible during a long answer.
  const group = page.getByTestId('comms-frame-group');
  await expect(group).toBeVisible();
  await expect(group.getByTestId('comms-frame-group-count')).toHaveText('×4');
  await group.locator('summary').first().click();
  await expect(group.getByTestId('comms-frame').first()).toBeVisible();
  await group.locator('summary').first().click();

  // The payload is one click away: expanding a frame reveals its JSON.
  const firstFrame = frames.first();
  await expect(firstFrame.locator('pre')).toBeHidden();
  await firstFrame.locator('summary').click();
  await expect(firstFrame.locator('pre')).toContainText('"thread/start"');

  // And the pane closes from its own control (floating over the frame list,
  // dressed like the terminal's), returning the toggle.
  await page.getByRole('button', { name: 'Close communication log' }).click();
  await expect(pane).toHaveCount(0);
  await expect(commsToggle).toBeVisible();
});

test('a Claude session keeps the terminal pane, not the comms one', async ({
  page,
}) => {
  // The other half of the capability split, on the same build: the provider with
  // a terminal is untouched by the comms pane's existence.
  await useManualEventControl(page);
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');

  // The cold-load focus is the open Claude session.
  const terminalToggle = page.getByRole('button', { name: 'Terminal' });
  await expect(terminalToggle).toBeVisible();
  await expect(page.getByRole('button', { name: 'Comms' })).toHaveCount(0);

  await terminalToggle.click();
  await expect(page.getByRole('separator', { name: 'Resize terminal' })).toBeVisible();
  await expect(page.getByTestId('comms-pane')).toHaveCount(0);
});

test('the comms log leaks no scrollable overflow past its own scroll box', async ({
  page,
}) => {
  // Every frame row carries absolutely positioned `sr-only` direction spans.
  // Unless they are anchored INSIDE the pane's scroll container, they escape
  // its clip (the scroller is a static box), pile up below the viewport at
  // their unscrolled row positions, and hand the workspace shell thousands of
  // px of invisible scrollable overflow — the raw material for the whole-app
  // shift pinned in workspace-shell.spec.ts. A viewport shorter than the
  // scripted exchange makes the leak measurable: rows must overflow the pane
  // for their spans to land below the shell's bottom edge.
  await useManualEventControl(page);
  await page.setViewportSize({ width: 1280, height: 300 });
  await page.goto('/');

  const codexRow = rowByBranch(page, 'feat/codex-adapter');
  await scrollUntilVisible(page, codexRow);
  await codexRow.click();

  // Baseline first: at this tiny viewport the shell already carries a few px
  // of ordinary min-height overflow that has nothing to do with the comms
  // pane, so the assertion below is a delta, not an absolute zero.
  const shellOverflow = () =>
    page
      .getByTestId('workspace-shell')
      .evaluate((shell) => shell.scrollHeight - shell.clientHeight);
  const baseline = await shellOverflow();

  await page.getByRole('button', { name: 'Comms' }).click();
  await expect(page.getByTestId('comms-frame').first()).toBeVisible();

  // The pane itself must be the thing that scrolls…
  const paneOverflow = await page
    .getByTestId('comms-pane')
    .evaluate((pane) => {
      const scroller = pane.parentElement;
      if (!scroller) {
        throw new Error('the comms pane is not inside a Panel body');
      }
      return scroller.scrollHeight - scroller.clientHeight;
    });
  expect(paneOverflow).toBeGreaterThan(0);

  // …and none of that below-the-fold content may register as scrollable
  // overflow on the workspace shell.
  expect(await shellOverflow()).toBe(baseline);
});
