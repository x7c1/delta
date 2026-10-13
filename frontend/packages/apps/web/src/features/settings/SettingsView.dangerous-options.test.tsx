/**
 * Launch options that turn off the agent's own safety mechanism, which may not
 * be enabled by default.
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
import { http, HttpResponse } from 'msw';
import { createHandlers } from '@delta/api-mocks';
import {
  DEFAULT_ENABLED_LABEL,
  findList,
  renderSettings,
  resetSettingsState,
  server,
  unstubGlobals,
} from './SettingsView.testkit';

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
// A fresh mock registry per test, not just a reset of the per-test overrides:
// `createHandlers()` closes over an in-memory store, so one shared instance
// would let a template (or launch option) created by one test leak into the
// next one's list and make the file order-dependent.
beforeEach(() => server.resetHandlers(...createHandlers()));
afterAll(() => server.close());

describe('SettingsView', () => {
  beforeEach(resetSettingsState);
  afterEach(unstubGlobals);

  /**
   * Three rows, because the rule has three cases: a dangerous row the user
   * could otherwise tick (locked), a dangerous row that *already* carries the
   * flag from before the rule existed (locking that one shut would be a dead
   * end, so it stays live), and an ordinary row (untouched).
   */
  function serveDangerousRegistryRows() {
    server.use(
      http.get('*/api/launch-options', () =>
        HttpResponse.json({
          launch_options: [
            {
              id: 7,
              label: null,
              name: '--dangerously-skip-permissions',
              value: null,
              default_enabled: false,
              created_at: '2026-01-05T00:00:00Z',
              provider: 'claude',
              builtin: false,
              dangerous: true,
              choice_group: null,
            },
            {
              id: 9,
              label: null,
              name: '--permission-mode',
              value: 'bypassPermissions',
              default_enabled: true,
              created_at: '2026-01-05T00:00:00Z',
              provider: 'claude',
              builtin: false,
              dangerous: true,
              choice_group: null,
            },
            {
              id: 8,
              label: 'Opus',
              name: '--model',
              value: 'opus',
              default_enabled: false,
              created_at: '2026-01-05T00:00:00Z',
              provider: 'claude',
              builtin: false,
              dangerous: false,
              choice_group: null,
            },
          ],
        }),
      ),
    );
  }

  it('disables default-enabled for a dangerous option and badges it', async () => {
    serveDangerousRegistryRows();
    renderSettings();
    const list = await findList();

    const dangerousRow = within(list).getByTestId('launch-option-row-7');
    expect(within(dangerousRow).getByText('Dangerous')).toBeInTheDocument();
    expect(
      within(dangerousRow).getByLabelText(
        'Enable launch option --dangerously-skip-permissions by default',
      ),
    ).toBeDisabled();

    // The control is live on an ordinary row, so the disabling is the rule and
    // not a broken column.
    const benignRow = within(list).getByTestId('launch-option-row-8');
    expect(within(benignRow).queryByText('Dangerous')).toBeNull();
    expect(
      within(benignRow).getByLabelText(
        'Enable launch option --model by default',
      ),
    ).toBeEnabled();
  });

  it('still lets a dangerous option that predates the rule be un-defaulted', async () => {
    // Only *setting* the flag is refused, so a row that already carries it has
    // to keep a live checkbox: otherwise deleting the row would be the only way
    // to clear a stored default the picker already ignores.
    serveDangerousRegistryRows();
    renderSettings();
    const list = await findList();

    const staleRow = within(list).getByTestId('launch-option-row-9');
    expect(within(staleRow).getByText('Dangerous')).toBeInTheDocument();
    const checkbox = within(staleRow).getByLabelText(
      'Enable launch option --permission-mode by default',
    );
    expect(checkbox).toBeEnabled();
    expect(checkbox).toBeChecked();
    // The tick is inert, and the row says so without needing a hover. Only this
    // row: the locked row (id 7) has no stale promise to withdraw.
    expect(
      within(staleRow).getByText('(no longer pre-checked)'),
    ).toBeInTheDocument();
    expect(
      within(list).getByTestId('launch-option-row-7').textContent,
    ).not.toContain('no longer pre-checked');

    let patched: unknown = null;
    server.use(
      http.patch('*/api/launch-options/9', async ({ request }) => {
        patched = await request.json();
        return HttpResponse.json({
          id: 9,
          label: null,
          name: '--permission-mode',
          value: 'bypassPermissions',
          default_enabled: false,
          created_at: '2026-01-05T00:00:00Z',
          provider: 'claude',
          builtin: false,
          dangerous: true,
          choice_group: null,
        });
      }),
    );
    fireEvent.click(checkbox);
    await waitFor(() => {
      expect(patched).toMatchObject({ default_enabled: false });
    });
  });

  it('explains the server refusal when a dangerous option is added as default', async () => {
    // The add form cannot pre-disable its default checkbox — whether a typed
    // name is dangerous is only knowable server-side — so the refusal has to
    // arrive as copy the user can act on rather than "please try again".
    renderSettings();
    await findList();

    fireEvent.change(screen.getByLabelText('Name (the flag)'), {
      target: { value: '--dangerously-skip-permissions' },
    });
    fireEvent.click(screen.getByLabelText(DEFAULT_ENABLED_LABEL));
    fireEvent.click(screen.getByRole('button', { name: 'Add option' }));

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Could not add the launch option.');
    expect(alert).toHaveTextContent('cannot be enabled by default');
  });
});
