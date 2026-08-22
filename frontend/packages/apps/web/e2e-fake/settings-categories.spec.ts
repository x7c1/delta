import { test, expect } from './support/fixtures';

/**
 * The Settings dialog hosts a VS Code-style 2-pane layout: a vertical category
 * rail on the left and the active category's content on the right. Clicking a
 * category swaps the right pane, and the previously visible section unmounts
 * so two categories never render simultaneously.
 *
 * The spec opens the dialog from the navigator gear, asserts the default
 * landing pane (Launch options) is the only one rendered, switches to Clone
 * roots, asserts the rail's selection follows and the right pane content has
 * swapped, and switches back to confirm the round-trip.
 *
 * No backend scripting is required — the spec touches only the registry UI
 * (the launch-options list and the clone-roots empty state), both of which
 * render purely from REST data the real backend already serves.
 */
test('Settings dialog swaps the right pane when categories are clicked', async ({
  page,
}) => {
  await page.goto('/');

  // The cold-start placeholder OR an existing session node settles the app
  // before reaching for the gear. Either is fine — both render with the
  // navigator's settings entry mounted in the footer.
  await expect(
    page.getByTestId('session-node').first().or(page.getByTestId('new-session-empty')),
  ).toBeVisible();

  await page.getByTestId('settings-entry').click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();

  // Default landing pane is Launch options; the clone-roots section is not
  // mounted yet.
  await expect(
    dialog.getByTestId('settings-category-launch-options'),
  ).toHaveAttribute('aria-selected', 'true');
  await expect(dialog.getByTestId('launch-options-section')).toBeVisible();
  await expect(dialog.getByTestId('clone-roots-section')).toHaveCount(0);

  // Switch to Clone roots: the rail's selection follows, the clone-roots
  // section appears, and the launch-options section unmounts.
  await dialog.getByTestId('settings-category-clone-roots').click();
  await expect(
    dialog.getByTestId('settings-category-clone-roots'),
  ).toHaveAttribute('aria-selected', 'true');
  await expect(
    dialog.getByTestId('settings-category-launch-options'),
  ).toHaveAttribute('aria-selected', 'false');
  await expect(dialog.getByTestId('clone-roots-section')).toBeVisible();
  await expect(dialog.getByTestId('launch-options-section')).toHaveCount(0);

  // And back: the launch-options pane returns.
  await dialog.getByTestId('settings-category-launch-options').click();
  await expect(dialog.getByTestId('launch-options-section')).toBeVisible();
  await expect(dialog.getByTestId('clone-roots-section')).toHaveCount(0);

  await dialog.getByTestId('settings-close').click();
  await expect(dialog).toHaveCount(0);
});

/**
 * The prompt-template registry end to end: the category sits in the rail right
 * after Launch options, opens on its (initially empty) list, and a New → Save
 * round-trip persists through the real `POST /api/prompt-templates` and comes
 * back in the refetched list.
 *
 * The body deliberately spans several lines with a blank line in the middle —
 * the shape a real template takes — so the assertions cover both halves of the
 * category's central promise: the editor accepts a long multi-paragraph body,
 * and the list row shows the label *only*, never a preview of that body.
 *
 * The suite runs against a per-run database with nothing seeded, so the empty
 * state is the honest starting point here.
 */
test('Settings registers a prompt template and lists it by label', async ({
  page,
}) => {
  await page.goto('/');
  await expect(
    page.getByTestId('session-node').first().or(page.getByTestId('new-session-empty')),
  ).toBeVisible();

  await page.getByTestId('settings-entry').click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();

  // The rail lists the category directly after Launch options.
  await expect(dialog.getByRole('tab')).toHaveText([
    'Launch options',
    'Prompt templates',
    'Clone roots',
    'Appearance',
    'Default provider',
  ]);

  await dialog.getByTestId('settings-category-prompt-templates').click();
  await expect(
    dialog.getByTestId('settings-category-prompt-templates'),
  ).toHaveAttribute('aria-selected', 'true');
  await expect(dialog.getByTestId('launch-options-section')).toHaveCount(0);
  await expect(dialog.getByTestId('prompt-templates-empty')).toHaveText(
    'No prompt templates yet.',
  );

  const body = [
    'Read the diff before touching anything.',
    '',
    'Then, in order: correctness, then the tests.',
  ].join('\n');

  await dialog.getByTestId('prompt-template-new').click();
  const editor = dialog.getByTestId('prompt-template-editor');
  await expect(editor).toBeVisible();
  await expect(editor.getByRole('button', { name: 'Save' })).toBeDisabled();

  await editor.getByLabel('Label', { exact: true }).fill('Review checklist');
  await editor.getByLabel('Text', { exact: true }).fill(body);
  await editor.getByRole('button', { name: 'Save' }).click();

  // Back on the list, with the new template as a row: the label shows, the
  // body does not.
  const list = dialog.getByTestId('prompt-templates-list');
  await expect(list.getByText('Review checklist')).toBeVisible();
  await expect(dialog.getByTestId('prompt-template-editor')).toHaveCount(0);
  await expect(
    dialog.getByTestId('prompt-templates-section'),
  ).not.toContainText('Read the diff before touching anything.');

  // Reopening the row's editor proves the whole body round-tripped through the
  // backend, newlines and blank line intact.
  await list
    .getByRole('button', { name: 'Edit prompt template Review checklist' })
    .click();
  await expect(
    dialog.getByTestId('prompt-template-editor').getByLabel('Text', { exact: true }),
  ).toHaveValue(body);

  await dialog.getByTestId('settings-close').click();
  await expect(dialog).toHaveCount(0);
});

/**
 * The dialog's box must not depend on which category is showing. The five
 * categories differ wildly in natural height — a two-option radio group versus
 * a form plus an unbounded list — so a content-sized panel resized on every
 * click, and because the backdrop centers it, both edges moved: the rail button
 * the user had just clicked slid out from under the cursor. The fix gives the
 * dialog a viewport-derived frame and lets the right pane scroll.
 *
 * Measuring the real box in a browser is the only way to state this: jsdom has
 * no layout. The assertion is on the rendered geometry rather than a class, so
 * it survives a refactor of how the height is expressed. It is also immune to
 * font-loading shifts, which is precisely the property a fixed frame buys.
 */
test('Settings dialog keeps a fixed frame across category switches', async ({
  page,
}) => {
  await page.goto('/');
  await expect(
    page.getByTestId('session-node').first().or(page.getByTestId('new-session-empty')),
  ).toBeVisible();

  await page.getByTestId('settings-entry').click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();

  // Every category, paired with the section its pane mounts, so each
  // measurement happens after the swap has settled.
  const categories = [
    { id: 'launch-options', section: 'launch-options-section' },
    { id: 'prompt-templates', section: 'prompt-templates-section' },
    { id: 'clone-roots', section: 'clone-roots-section' },
    { id: 'appearance', section: 'appearance-section' },
    { id: 'default-provider', section: 'default-provider-section' },
  ];

  // The rail is inside the panel, so a moving panel drags the rail with it.
  // Tracking the first rail button's top edge alongside the panel's box is what
  // pins the user-facing invariant: the thing you click stays where you clicked.
  const railButton = dialog.getByTestId('settings-category-launch-options');

  let frame: { height: number; top: number; railTop: number } | null = null;
  for (const category of categories) {
    await dialog.getByTestId(`settings-category-${category.id}`).click();
    await expect(dialog.getByTestId(category.section)).toBeVisible();

    const panelBox = await dialog.boundingBox();
    const railBox = await railButton.boundingBox();
    expect(panelBox).not.toBeNull();
    expect(railBox).not.toBeNull();
    // Subpixel noise is not what this guards against; whole pixels are.
    const measured = {
      height: Math.round(panelBox!.height),
      top: Math.round(panelBox!.y),
      railTop: Math.round(railBox!.y),
    };

    if (frame === null) {
      frame = measured;
      // A frame that collapsed to nothing would trivially satisfy the equality
      // below, so anchor the first measurement against a real height.
      expect(measured.height).toBeGreaterThan(200);
    } else {
      expect(measured, `category ${category.id} moved the dialog`).toEqual(frame);
    }
  }

  await dialog.getByTestId('settings-close').click();
  await expect(dialog).toHaveCount(0);
});
