import { test, expect } from './support/fixtures';
import { startNewSession } from './support/app';
import { fetchMessageCount, latestSession } from './support/rest';

/**
 * Inserting a prompt template from the composer rail, end to end against the
 * real backend: the registry the button reads is the one the REST API serves,
 * and what lands in the textarea is the stored text byte-for-byte.
 *
 * The suite runs against a per-run database with nothing seeded, so the two
 * templates are registered through `POST /api/prompt-templates` first — the
 * same call the settings editor makes. The second is deliberately
 * multi-paragraph: the whole point of the two-column popover is that a template
 * is a BLOCK of prose, and the only way to prove the block survives the trip
 * (blank line, bullets and all) is to insert a real one.
 *
 * Scenario `first-send`: the fake holds the turn briefly and replies, which is
 * all this needs — it wants a live thread to type into, not a conversation.
 */
const SHORT_TEMPLATE = 'Once CI is green, merge the PR.';
const MULTILINE_TEMPLATE = [
  "Review the diff on this branch with a critic's eye.",
  '',
  'Check, in order:',
  '- correctness first: error paths that swallow a failure;',
  '- then the tests: does a failing assertion say what broke?',
  '',
  'Report what you found as a short list, worst first.',
].join('\n');

/**
 * The ids this spec registered, so it can hand the registry back empty. One
 * backend and one database serve the whole serial suite, so a template left
 * behind would be a template another spec did not ask for — the settings spec
 * asserts on the empty state.
 */
const registered: number[] = [];

test.afterEach(async ({ page }) => {
  while (registered.length > 0) {
    const id = registered.pop();
    await page.request.delete(`/api/prompt-templates/${id}`);
  }
});

test('a prompt template is inserted into the draft at the caret, and nothing is sent', async ({
  page,
}) => {
  await page.goto('/');

  // Register the registry the popover will list, through the real API.
  for (const [label, text] of [
    ['Merge when green', SHORT_TEMPLATE],
    ['Review checklist', MULTILINE_TEMPLATE],
  ]) {
    const response = await page.request.post('/api/prompt-templates', {
      data: { label, text },
    });
    expect(response.ok()).toBe(true);
    registered.push(((await response.json()) as { id: number }).id);
  }

  await startNewSession(page, 'first-send hold then answer');

  // Let the opening turn finish before counting, so the baseline cannot drift
  // under the assertion at the end.
  await expect(page.getByText('done thinking')).toBeVisible({
    timeout: 15_000,
  });
  const session = await latestSession(page);
  const before = await fetchMessageCount(page, session.mainThreadId);
  expect(before).toBeGreaterThan(0);

  // Type a draft and park the caret in the middle of it, where the template
  // will be spliced in.
  const textarea = page.getByRole('textbox');
  await textarea.fill('before after');
  await textarea.click();
  await textarea.evaluate((node: HTMLTextAreaElement) => {
    node.setSelectionRange('before '.length, 'before '.length);
  });

  await page.getByTestId('prompt-templates-button').click();
  const popover = page.getByTestId('prompt-templates-popover');
  await expect(popover).toBeVisible();

  // The list names the templates and shows no body text at all; the preview
  // pane beside it carries the focused template's text in full.
  const list = popover.getByTestId('prompt-templates-popover-list');
  await expect(list.getByRole('menuitem')).toHaveText([
    'Merge when green',
    'Review checklist',
  ]);
  await expect(list).not.toContainText('Review the diff on this branch');

  await list.getByRole('menuitem', { name: 'Review checklist' }).hover();
  await expect(
    popover.getByTestId('prompt-templates-popover-preview'),
  ).toContainText('Report what you found as a short list');

  await list.getByRole('menuitem', { name: 'Review checklist' }).click();
  await expect(popover).toHaveCount(0);

  // The whole multi-paragraph body landed at the caret, verbatim, with the
  // draft's own text intact on either side and no separator invented between.
  await expect(textarea).toHaveValue(
    `before ${MULTILINE_TEMPLATE}after`,
  );
  // The caret sits right after what was inserted, so typing continues there.
  expect(
    await textarea.evaluate((node: HTMLTextAreaElement) => node.selectionStart),
  ).toBe('before '.length + MULTILINE_TEMPLATE.length);
  await expect(textarea).toBeFocused();

  // Inserting is a draft edit, never a send: the thread has grown by nothing.
  expect(await fetchMessageCount(page, session.mainThreadId)).toBe(before);
});
