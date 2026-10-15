/**
 * The launch options category: listing, adding, defaulting, grouping,
 * duplicating and deleting options per provider.
 *
 * Shared fixtures live in SettingsView.testkit.tsx.
 */
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { createHandlers } from '@delta/api-mocks';
import type { LaunchOption } from '@delta/wire-gen';
import { useSettingsStore } from '../../store/settingsStore';
import {
  DEFAULT_ENABLED_LABEL,
  findList,
  providerRadio,
  renderSettings,
  resetSettingsState,
  selectProvider,
  server,
  unstubGlobals,
} from './SettingsView.testkit';

beforeAll(() => server.listen({ onUnhandledFrame: 'error' }));
// A fresh mock registry per test, not just a reset of the per-test overrides:
// `createHandlers()` closes over an in-memory store, so one shared instance
// would let a template (or launch option) created by one test leak into the
// next one's list and make the file order-dependent.
beforeEach(() => server.resetHandlers(...createHandlers()));
afterAll(() => server.close());

describe('SettingsView', () => {
  beforeEach(resetSettingsState);
  afterEach(unstubGlobals);

  it('lists the seeded launch options of the selected provider', async () => {
    renderSettings();
    const list = await findList();
    // The section opens on the configured default provider (Claude, per the
    // beforeEach), so only the Claude fixtures are listed — the user's two flags
    // plus the shipped `--model opus` — and the Codex ones (`model`, `config`)
    // are filtered out.
    expect(within(list).getAllByText('--permission-mode')[0]).toBeInTheDocument();
    expect(within(list).getByText('--plugin-dir')).toBeInTheDocument();
    expect(within(list).getAllByText('--model')[0]).toBeInTheDocument();
    expect(within(list).queryByText('model')).toBeNull();
    expect(within(list).queryByText('config')).toBeNull();
  });

  it('opens scoped to the configured default provider', async () => {
    // A Codex-first user (Settings → Default provider = Codex) must land on
    // their own options, not on Claude's list.
    useSettingsStore.setState({ defaultProvider: 'codex' });
    renderSettings();
    const list = await findList();
    expect(providerRadio('codex')).toBeChecked();
    expect(within(list).getAllByText('model')[0]).toBeInTheDocument();
    expect(within(list).queryByText('--permission-mode')).toBeNull();
  });

  it('adds a launch option through the form', async () => {
    renderSettings();
    const list = await findList();
    // Sanity: the new flag is not present before submission.
    expect(within(list).queryByText('--append-system-prompt')).toBeNull();

    fireEvent.change(screen.getByLabelText('Name (the flag)'), {
      target: { value: '--append-system-prompt' },
    });
    fireEvent.change(screen.getByLabelText('Value (optional)'), {
      target: { value: 'Be brief.' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Add option' }));

    await waitFor(() =>
      expect(within(list).getAllByText('--append-system-prompt')[0]).toBeInTheDocument(),
    );
  });

  it('omits the provider name from the option rows', async () => {
    renderSettings();
    const list = await findList();
    expect(within(list).queryByText('Claude Code')).toBeNull();
    expect(within(list).queryByText('Codex')).toBeNull();
  });

  it('swaps the listed options when the provider selector changes', async () => {
    renderSettings();
    await findList();

    selectProvider('codex');

    // Only the Codex fixture (`model gpt-5`) remains listed.
    const codexList = await findList();
    expect(within(codexList).getAllByText('model')[0]).toBeInTheDocument();
    expect(within(codexList).queryByText('--permission-mode')).toBeNull();
    expect(within(codexList).queryByText('--plugin-dir')).toBeNull();

    // And back: the Claude options return.
    selectProvider('claude');
    const claudeList = await findList();
    expect(within(claudeList).getAllByText('--permission-mode')[0]).toBeInTheDocument();
    expect(within(claudeList).queryByText('model')).toBeNull();
  });

  it('shows a provider-scoped empty state when only the other provider has options', async () => {
    // A registry holding Claude options only: on Codex the list must not claim
    // the whole registry is empty, nor leak the Claude rows.
    server.use(
      http.get('*/api/launch-options', () =>
        HttpResponse.json({
          launch_options: [
            {
              id: 1,
              label: 'My plugins',
              name: '--plugin-dir',
              value: '/home/dev/plugins',
              default_enabled: true,
              created_at: '2026-01-01T00:00:00Z',
              provider: 'claude',
              builtin: false,
              dangerous: false,
              choice_group: null,
            },
          ],
        }),
      ),
    );
    renderSettings();
    await findList();

    selectProvider('codex');

    const empty = await screen.findByTestId('launch-options-empty');
    expect(empty.textContent).toBe('No launch options registered for Codex yet.');
    expect(screen.queryByTestId('launch-options-list')).toBeNull();
  });

  it('registers a launch option for the selected provider and stays scoped to it', async () => {
    renderSettings();
    await findList();

    // Pick Codex in the section's provider selector, then add an option. Codex
    // takes field-style options, so the name input is labelled for a field.
    selectProvider('codex');
    fireEvent.change(screen.getByLabelText('Label (optional)'), {
      target: { value: 'Reasoning' },
    });
    fireEvent.change(screen.getByLabelText('Name (the field)'), {
      target: { value: 'reasoning-effort' },
    });
    fireEvent.change(screen.getByLabelText('Value (optional)'), {
      target: { value: 'high' },
    });
    fireEvent.click(screen.getByLabelText(DEFAULT_ENABLED_LABEL));
    fireEvent.click(screen.getByRole('button', { name: 'Add option' }));

    // The new option appears in the still-Codex-scoped list, proving the chosen
    // provider was sent and round-tripped.
    const list = await findList();
    await within(list).findAllByText('reasoning-effort');
    expect(within(list).queryByText('--permission-mode')).toBeNull();

    // The selector stays on Codex while the other fields are cleared.
    expect(providerRadio('codex')).toBeChecked();
    expect(providerRadio('claude')).not.toBeChecked();
    expect(screen.getByLabelText('Label (optional)')).toHaveValue('');
    expect(screen.getByLabelText('Name (the field)')).toHaveValue('');
    expect(screen.getByLabelText('Value (optional)')).toHaveValue('');
    expect(screen.getByLabelText(DEFAULT_ENABLED_LABEL)).not.toBeChecked();
  });

  it('words the add form for the selected provider launch-option style', async () => {
    renderSettings();
    await findList();

    // Claude takes CLI flags: the name input is labelled and exemplified as one.
    const flagName = screen.getByLabelText('Name (the flag)');
    expect(flagName).toHaveAttribute('placeholder', '--permission-mode');
    expect(screen.getByLabelText('Value (optional)')).toHaveAttribute(
      'placeholder',
      'auto',
    );
    expect(screen.getByTestId('launch-options-section').textContent).toContain(
      'Register custom CLI flags',
    );

    selectProvider('codex');

    // Codex takes session-start request fields: a user must write `model`, not
    // `--model`, so nothing in the form may still say "flag".
    await waitFor(() =>
      expect(screen.queryByLabelText('Name (the flag)')).toBeNull(),
    );
    expect(screen.getByLabelText('Name (the field)')).toHaveAttribute(
      'placeholder',
      'model',
    );
    expect(screen.getByLabelText('Value (optional)')).toHaveAttribute(
      'placeholder',
      'gpt-5.6-sol',
    );
    const section = screen.getByTestId('launch-options-section');
    expect(section.textContent).toContain('Register custom session-start settings');
    expect(section.textContent).not.toContain('CLI flags');
  });

  it('disables Add until a non-blank name is entered', async () => {
    renderSettings();
    await findList();
    const addButton = screen.getByRole('button', { name: 'Add option' });
    expect(addButton).toBeDisabled();

    fireEvent.change(screen.getByLabelText('Name (the flag)'), {
      target: { value: '   ' },
    });
    expect(addButton).toBeDisabled();

    fireEvent.change(screen.getByLabelText('Name (the flag)'), {
      target: { value: '--verbose' },
    });
    expect(addButton).toBeEnabled();
  });

  it('toggles a launch option default_enabled flag and persists it', async () => {
    renderSettings();
    const list = await findList();
    // `--plugin-dir` (id 1) is repeatable, so its default is an independent
    // checkbox; it starts on.
    const pluginDirToggle = within(list).getByRole('checkbox', {
      name: 'Enable launch option --plugin-dir by default',
    });
    expect(pluginDirToggle).toBeChecked();

    // Toggling it off round-trips through the mock store (the list refetches).
    fireEvent.click(pluginDirToggle);
    await waitFor(() =>
      expect(
        within(list).getByRole('checkbox', {
          name: 'Enable launch option --plugin-dir by default',
        }),
      ).not.toBeChecked(),
    );
  });

  it('lists a group holding a single row like an ungrouped row', async () => {
    renderSettings();
    const list = await findList();
    // `--permission-mode` (id 2) is single-valued, but no sibling is
    // registered, so there is no exclusivity to show: the row keeps the plain
    // default checkbox, with no radio group and no "No default" option.
    expect(
      within(list).queryByTestId('launch-option-group---permission-mode'),
    ).not.toBeInTheDocument();
    expect(
      within(list).queryByTestId('launch-option-group---permission-mode-none'),
    ).not.toBeInTheDocument();
    expect(within(list).queryByRole('radiogroup')).not.toBeInTheDocument();
    expect(within(list).getByTestId('launch-option-row-2')).toBeInTheDocument();
    const toggle = within(list).getByRole('checkbox', {
      name: 'Enable launch option --permission-mode by default',
    });
    expect(toggle).not.toBeChecked();

    fireEvent.click(toggle);
    await waitFor(() =>
      expect(
        within(list).getByRole('checkbox', {
          name: 'Enable launch option --permission-mode by default',
        }),
      ).toBeChecked(),
    );
  });

  it('switches a group default by clearing the old one, then setting the new one', async () => {
    const rows: LaunchOption[] = [
      {
        id: 20,
        label: 'Fable',
        name: '--model',
        value: 'fable',
        default_enabled: true,
        created_at: '2026-01-05T00:00:00Z',
        provider: 'claude',
        builtin: true,
        dangerous: false,
        choice_group: '--model',
      },
      {
        id: 21,
        label: null,
        name: '--model',
        value: 'e',
        default_enabled: false,
        created_at: '2026-01-05T00:00:00Z',
        provider: 'claude',
        builtin: false,
        dangerous: false,
        choice_group: '--model',
      },
    ];
    const patches: Array<[number, unknown]> = [];
    server.use(
      http.get('*/api/launch-options', () =>
        HttpResponse.json({ launch_options: rows }),
      ),
      http.patch('*/api/launch-options/:id', async ({ params, request }) => {
        const id = Number(params.id);
        const body = (await request.json()) as { default_enabled: boolean };
        patches.push([id, body]);
        const row = rows.find((each) => each.id === id)!;
        row.default_enabled = body.default_enabled;
        return HttpResponse.json(row);
      }),
    );
    renderSettings();
    const list = await findList();

    // Two rows share the group, so they are listed together under one default
    // radio group headed by the key.
    const group = within(list).getByTestId('launch-option-group---model');
    expect(within(group).getByRole('radiogroup', { name: '--model' }))
      .toBeInTheDocument();
    expect(within(group).getByTestId('launch-option-row-20')).toBeInTheDocument();
    expect(within(group).getByTestId('launch-option-row-21')).toBeInTheDocument();

    const custom = within(list).getByRole('radio', {
      name: 'Enable launch option --model e by default',
    });
    expect(
      within(list).getByRole('radio', {
        name: 'Enable launch option --model fable by default',
      }),
    ).toBeChecked();

    fireEvent.click(custom);
    await waitFor(() => {
      expect(patches).toEqual([
        [20, { default_enabled: false }],
        [21, { default_enabled: true }],
      ]);
    });
    await waitFor(() =>
      expect(
        within(list).getByRole('radio', {
          name: 'Enable launch option --model e by default',
        }),
      ).toBeChecked(),
    );

    // "No default" is a single clear.
    fireEvent.click(within(list).getByTestId('launch-option-group---model-none'));
    await waitFor(() => {
      expect(patches.slice(2)).toEqual([[21, { default_enabled: false }]]);
    });
  });

  it('keeps focus in a group default radio group while arrowing across a switch', async () => {
    // A native radio group moves the selection on every arrow key. If the
    // group disabled itself while each switch was saved, a keyboard user would
    // lose focus after one step. The radios stay enabled, the selection follows
    // the keys, and the saves coalesce onto the option the user stopped on.
    const row = (id: number, value: string, isDefault: boolean): LaunchOption => ({
      id,
      label: null,
      name: '--model',
      value,
      default_enabled: isDefault,
      created_at: '2026-01-05T00:00:00Z',
      provider: 'claude',
      builtin: false,
      dangerous: false,
      choice_group: '--model',
    });
    const rows = [row(30, 'a', true), row(31, 'b', false), row(32, 'c', false)];
    const patches: Array<[number, boolean]> = [];
    // Hold every PATCH until the test lets it through, so the first switch is
    // still in flight while the user keeps arrowing.
    let release: () => void = () => {};
    let gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    server.use(
      http.get('*/api/launch-options', () =>
        HttpResponse.json({ launch_options: rows }),
      ),
      http.patch('*/api/launch-options/:id', async ({ params, request }) => {
        await gate;
        const id = Number(params.id);
        const body = (await request.json()) as { default_enabled: boolean };
        patches.push([id, body.default_enabled]);
        const target = rows.find((each) => each.id === id)!;
        target.default_enabled = body.default_enabled;
        return HttpResponse.json(target);
      }),
    );
    const user = userEvent.setup();
    renderSettings();
    const list = await findList();
    const group = within(list).getByRole('radiogroup', { name: '--model' });
    const radio = (value: string) =>
      within(group).getByRole('radio', {
        name: `Enable launch option --model ${value} by default`,
      });

    radio('a').focus();
    await user.keyboard('{ArrowDown}');
    // The first step's save is in flight; the group stays operable and focused.
    expect(radio('b')).toBeChecked();
    expect(radio('b')).toHaveFocus();
    for (const each of within(group).getAllByRole('radio')) {
      expect(each).toBeEnabled();
    }

    await user.keyboard('{ArrowDown}');
    expect(radio('c')).toBeChecked();
    expect(radio('c')).toHaveFocus();

    // Let every save through: the first step lands, then one more switch goes
    // straight to the option the user stopped on.
    release();
    gate = Promise.resolve();
    await waitFor(() =>
      expect(patches).toEqual([
        [30, false],
        [31, true],
        [31, false],
        [32, true],
      ]),
    );
    await waitFor(() =>
      expect(group.getAttribute('aria-busy')).toBe('false'),
    );
    expect(radio('c')).toBeChecked();
    expect(radio('c')).toHaveFocus();
  });

  it('deletes a launch option', async () => {
    renderSettings();
    const list = await findList();
    const target = within(list).getAllByText('--permission-mode')[0];

    fireEvent.click(
      within(list).getByRole('button', {
        name: 'Delete launch option --permission-mode',
      }),
    );

    await waitFor(() => expect(target).not.toBeInTheDocument());
    expect(within(list).getByText('--plugin-dir')).toBeInTheDocument();
  });

  it('badges a built-in row and gives it no delete control', async () => {
    renderSettings();
    const list = await findList();
    // The shipped Claude row (`--model opus`) against the user's own
    // `--plugin-dir`: one badge, one Delete, never both on either.
    const shipped = within(list).getByTestId('launch-option-row-100');
    const mine = within(list).getByTestId('launch-option-row-1');

    expect(within(shipped).getByText('Built-in')).toBeInTheDocument();
    expect(
      within(shipped).queryByRole('button', {
        name: 'Delete launch option --model',
      }),
    ).toBeNull();

    expect(within(mine).queryByText('Built-in')).toBeNull();
    expect(
      within(mine).getByRole('button', {
        name: 'Delete launch option --plugin-dir',
      }),
    ).toBeInTheDocument();
  });

  it('keeps the default_enabled control operable on a built-in row', async () => {
    // Ticking a shipped option is the point of shipping it, so the one control
    // a built-in row keeps must actually round-trip. The shipped `--model` row
    // is alone in the `--model` choice group, so that control is the plain
    // default checkbox.
    renderSettings();
    const list = await findList();
    const toggle = within(list).getByRole('checkbox', {
      name: 'Enable launch option --model by default',
    });
    expect(toggle).not.toBeChecked();

    fireEvent.click(toggle);

    await waitFor(() =>
      expect(
        within(list).getByRole('checkbox', {
          name: 'Enable launch option --model by default',
        }),
      ).toBeChecked(),
    );
  });

  it('shows a long value in full with no control to reveal it', async () => {
    // The shipped Codex `config` row: a JSON object far wider than the row.
    // The rendered text must still be the whole value — not a truncation of it —
    // and getting to it must take no interaction.
    useSettingsStore.setState({ defaultProvider: 'codex' });
    renderSettings();
    const list = await findList();
    const row = within(list).getByTestId('launch-option-row-101');

    expect(
      within(row).getByTestId('launch-option-value-101').textContent,
    ).toBe(
      '{"model_reasoning_summary": "auto", "sandbox_workspace_write": {"writable_roots": ["/home/dev/repos/delta", "/home/dev/repos/scratch"]}}',
    );
    expect(within(row).queryByRole('button', { name: /value/i })).toBeNull();
  });

  it('shows a short value in full too, with no control to hide it', async () => {
    // The same treatment regardless of length: no threshold decides which rows
    // show their value.
    renderSettings();
    const list = await findList();
    const row = within(list).getByTestId('launch-option-row-2');

    expect(within(row).getByTestId('launch-option-value-2').textContent).toBe(
      'auto',
    );
    expect(within(row).queryByRole('button', { name: /value/i })).toBeNull();
  });

  it('populates the add form from a built-in row via Duplicate', async () => {
    useSettingsStore.setState({ defaultProvider: 'codex' });
    renderSettings();
    const list = await findList();

    fireEvent.click(
      within(within(list).getByTestId('launch-option-row-101')).getByRole(
        'button',
        { name: 'Duplicate launch option config' },
      ),
    );

    expect(screen.getByLabelText('Label (optional)')).toHaveValue(
      'Config: reasoning summary',
    );
    expect(screen.getByLabelText('Name (the field)')).toHaveValue(
      'config',
    );
    expect(screen.getByLabelText('Value (optional)')).toHaveValue(
      '{"model_reasoning_summary": "auto", "sandbox_workspace_write": {"writable_roots": ["/home/dev/repos/delta", "/home/dev/repos/scratch"]}}',
    );
  });

  it('populates the add form from a user row via Duplicate', async () => {
    renderSettings();
    const list = await findList();

    fireEvent.click(
      within(within(list).getByTestId('launch-option-row-1')).getByRole(
        'button',
        { name: 'Duplicate launch option --plugin-dir' },
      ),
    );

    expect(screen.getByLabelText('Label (optional)')).toHaveValue('My plugins');
    expect(screen.getByLabelText('Name (the flag)')).toHaveValue('--plugin-dir');
    expect(screen.getByLabelText('Value (optional)')).toHaveValue(
      '/home/dev/plugins',
    );
  });

  it('submits a multi-line value from the add form verbatim', async () => {
    // The value control is a textarea, so a JSON object spread over several
    // lines can be entered — and must reach the server exactly as typed, since
    // Codex parses it.
    const multiline = '{\n  "model_reasoning_summary": "auto"\n}';
    let body: unknown = null;
    server.use(
      http.post('*/api/launch-options', async ({ request }) => {
        body = await request.json();
        return HttpResponse.json(
          {
            id: 42,
            label: null,
            name: 'config',
            value: multiline,
            default_enabled: false,
            created_at: '2026-01-04T00:00:00Z',
            provider: 'codex',
            builtin: false,
            dangerous: false,
            choice_group: null,
          },
          { status: 201 },
        );
      }),
    );
    renderSettings();
    await findList();

    const valueInput = screen.getByLabelText('Value (optional)');
    expect(valueInput.tagName).toBe('TEXTAREA');
    fireEvent.change(screen.getByLabelText('Name (the flag)'), {
      target: { value: 'config' },
    });
    fireEvent.change(valueInput, { target: { value: multiline } });
    fireEvent.click(screen.getByRole('button', { name: 'Add option' }));

    await waitFor(() => expect(body).not.toBeNull());
    expect(body).toMatchObject({ name: 'config', value: multiline });
  });
});
