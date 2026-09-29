import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type { LaunchOption } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useComposerStore } from '../../store/composerStore';
import { LaunchOptionsPicker } from './LaunchOptionsPicker';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function renderPicker() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <LaunchOptionsPicker />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('LaunchOptionsPicker', () => {
  beforeEach(() => {
    // A fresh, not-yet-seeded new-session compose state on the Claude default.
    useComposerStore.setState({
      newSessionProvider: 'claude',
      newSessionProviderSeeded: false,
      newSessionLaunchOptionIds: [],
      newSessionLaunchOptionsSeeded: false,
    });
  });

  it('renders nothing when the registry is empty', async () => {
    server.use(
      http.get('*/api/launch-options', () =>
        HttpResponse.json({ launch_options: [] }),
      ),
    );
    renderPicker();
    // Give the query a tick; the picker stays absent with no options.
    await waitFor(() => {
      expect(
        screen.queryByTestId('launch-options-picker'),
      ).not.toBeInTheDocument();
    });
  });

  it('seeds the initial selection from the default_enabled options', async () => {
    renderPicker();
    // The fixture marks `--plugin-dir` (id 1) `default_enabled`, so it is
    // pre-checked once the registry loads; `--permission-mode` (id 2) is not.
    const pluginDir = await screen.findByTestId('launch-option-1');
    const permissionMode = screen.getByTestId('launch-option-2');
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([1]);
    });
    expect(pluginDir).toBeChecked();
    expect(permissionMode).not.toBeChecked();
  });

  it('records selections in click order and drops them on deselect', async () => {
    // Start from an already-seeded, empty selection so click order is the only
    // thing under test (no default seeding interferes).
    useComposerStore.setState({
      newSessionLaunchOptionIds: [],
      newSessionLaunchOptionsSeeded: true,
    });
    renderPicker();
    // The two seeded options (`--permission-mode auto` = id 2,
    // `--plugin-dir` = id 1) appear once the query resolves.
    const permissionMode = await screen.findByTestId('launch-option-2');
    const pluginDir = screen.getByTestId('launch-option-1');

    // Click the higher id first to prove the stored order follows clicks, not id.
    fireEvent.click(permissionMode);
    fireEvent.click(pluginDir);
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([
        2, 1,
      ]);
    });

    // Clearing one leaves the rest, order preserved. `--permission-mode` is
    // single-valued, but its group holds one row, so it is a plain checkbox
    // cleared by clicking it again.
    fireEvent.click(permissionMode);
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([
        1,
      ]);
    });
  });

  it('does not re-seed after the user unchecks a default-enabled option', async () => {
    renderPicker();
    const pluginDir = await screen.findByTestId('launch-option-1');
    // Seeded on (id 1 is default_enabled).
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([1]);
    });

    // The user unchecks it: the selection is now empty but seeded, so it must
    // stay empty rather than re-seeding back to the defaults.
    fireEvent.click(pluginDir);
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([]);
    });
    // Give any stray seed effect a chance to (incorrectly) fire.
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionLaunchOptionsSeeded).toBe(
        true,
      );
    });
    expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([]);
  });

  it('shows only the selected provider\'s options', async () => {
    // Default provider is Claude: the two Claude fixtures (ids 1, 2) show; the
    // Codex fixture (id 3, `model gpt-5`) is filtered out.
    renderPicker();
    await screen.findByTestId('launch-option-1');
    expect(screen.getByTestId('launch-option-2')).toBeInTheDocument();
    expect(screen.queryByTestId('launch-option-3')).not.toBeInTheDocument();
  });

  it('seeds default_enabled only from the selected provider', async () => {
    // Start on Codex: no Codex fixture is default_enabled, so nothing is
    // pre-checked even though a Claude option (id 1) is default_enabled.
    useComposerStore.setState({
      newSessionProvider: 'codex',
      newSessionProviderSeeded: true,
      newSessionLaunchOptionIds: [],
      newSessionLaunchOptionsSeeded: false,
    });
    renderPicker();
    await screen.findByTestId('launch-option-3');
    await waitFor(() => {
      expect(
        useComposerStore.getState().newSessionLaunchOptionsSeeded,
      ).toBe(true);
    });
    expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([]);
  });

  it('re-filters and drops cross-provider selections when the provider switches', async () => {
    // Seeded on Claude: id 1 (default_enabled) is pre-selected.
    renderPicker();
    await screen.findByTestId('launch-option-1');
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([1]);
    });

    // Switch to Codex mid-compose (as the provider selector would).
    act(() => {
      useComposerStore.getState().setNewSessionProvider('codex');
    });

    // The Claude options disappear, the Codex option appears, and the Claude
    // selection (id 1) is dropped — reset to the Codex provider's defaults
    // (none default_enabled → empty), so a Codex send never carries a Claude id.
    await screen.findByTestId('launch-option-3');
    expect(screen.queryByTestId('launch-option-1')).not.toBeInTheDocument();
    expect(screen.queryByTestId('launch-option-2')).not.toBeInTheDocument();
    await waitFor(() => {
      expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([]);
    });
  });

  describe('a dangerous option', () => {
    /**
     * A registry holding one dangerous Claude option that still says
     * `default_enabled: true` — the shape a row registered before that rule can
     * have — beside one benign default-enabled option.
     */
    function serveADangerousDefaultEnabledOption() {
      server.use(
        http.get('*/api/launch-options', () =>
          HttpResponse.json({
            launch_options: [
              {
                id: 7,
                label: 'Skip permissions',
                name: '--dangerously-skip-permissions',
                value: null,
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
                default_enabled: true,
                created_at: '2026-01-05T00:00:00Z',
                provider: 'claude',
                builtin: false,
                dangerous: false,
                choice_group: '--model',
              },
            ],
          }),
        ),
      );
    }

    it('does not auto-check a dangerous option even when its stored row is default_enabled', async () => {
      serveADangerousDefaultEnabledOption();
      renderPicker();

      const dangerous = await screen.findByTestId('launch-option-7');
      const benign = screen.getByTestId('launch-option-8');
      // The benign default *is* seeded, so the seeding itself ran — the
      // dangerous option is filtered out of it rather than the seed having been
      // skipped altogether.
      await waitFor(() => {
        expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([
          8,
        ]);
      });
      expect(dangerous).not.toBeChecked();
      expect(benign).toBeChecked();
      // Marked, so the user can see why it was left alone.
      expect(screen.getByText('Dangerous')).toBeInTheDocument();
      // And nothing is warned about until something is actually selected.
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    });

    it('reveals an inline warning naming the option once it is checked', async () => {
      serveADangerousDefaultEnabledOption();
      renderPicker();
      const dangerous = await screen.findByTestId('launch-option-7');

      fireEvent.click(dangerous);
      await waitFor(() => {
        expect(useComposerStore.getState().newSessionLaunchOptionIds).toContain(
          7,
        );
      });
      const alert = await screen.findByRole('alert');
      expect(alert).toHaveTextContent('Skip permissions');
      expect(alert).toHaveTextContent('safety mechanism');

      // Unchecking it takes the warning away again.
      fireEvent.click(dangerous);
      await waitFor(() => {
        expect(screen.queryByRole('alert')).not.toBeInTheDocument();
      });
    });
  });
  describe('a choice group', () => {
    /** A Claude launch-option row, with only what a test varies spelled out. */
    function row(
      id: number,
      overrides: Partial<LaunchOption> & Pick<LaunchOption, 'name'>,
    ): LaunchOption {
      return {
        id,
        label: null,
        value: null,
        default_enabled: false,
        created_at: '2026-01-05T00:00:00Z',
        provider: 'claude',
        builtin: false,
        dangerous: false,
        choice_group: null,
        ...overrides,
      };
    }

    /**
     * Two `--model` rows in one group (the shipped one first, then the user's
     * own), with an independent `--plugin-dir` row between them in list order
     * — so the group has to gather its second row from further down — and a
     * dangerous row alone in a group of its own.
     */
    function serveAModelGroup(defaults: { opus?: boolean; custom?: boolean } = {}) {
      server.use(
        http.get('*/api/launch-options', () =>
          HttpResponse.json({
            launch_options: [
              row(10, {
                label: 'Opus',
                name: '--model',
                value: 'opus',
                builtin: true,
                choice_group: '--model',
                default_enabled: defaults.opus ?? false,
              }),
              row(12, { name: '--plugin-dir', value: '/p' }),
              row(11, {
                name: '--model',
                value: 'e',
                choice_group: '--model',
                default_enabled: defaults.custom ?? false,
              }),
              row(13, {
                name: '--permission-mode',
                value: 'bypassPermissions',
                dangerous: true,
                choice_group: '--permission-mode',
              }),
            ],
          }),
        ),
      );
    }

    it('renders as a radio group headed by its key with an "Agent default" option', async () => {
      serveAModelGroup();
      renderPicker();

      const group = await screen.findByTestId('launch-option-group---model');
      expect(group).toHaveAttribute('role', 'radiogroup');
      expect(screen.getByRole('radiogroup', { name: '--model' })).toBe(group);
      const none = within(group).getByTestId('launch-option-group---model-none');
      expect(none).toHaveAttribute('type', 'radio');
      expect(within(group).getByText('Agent default')).toBeInTheDocument();
      // Nothing is selected, so "Agent default" is the checked choice.
      expect(none).toBeChecked();
      // Both `--model` rows are radios inside the group, in list order.
      const radios = within(group).getAllByRole('radio');
      expect(radios.map((radio) => radio.dataset.testid)).toEqual([
        'launch-option-group---model-none',
        'launch-option-10',
        'launch-option-11',
      ]);
    });

    it('keeps an ungrouped row a checkbox, placed by its list position', async () => {
      serveAModelGroup();
      renderPicker();
      const pluginDir = await screen.findByTestId('launch-option-12');
      expect(pluginDir).toHaveAttribute('type', 'checkbox');
      // The group sits where its first row does, before `--plugin-dir`.
      const group = screen.getByTestId('launch-option-group---model');
      expect(
        group.compareDocumentPosition(pluginDir) &
          Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
    });

    it('renders a group holding a single row as a plain checkbox', async () => {
      serveAModelGroup();
      renderPicker();
      // `--permission-mode` is single-valued, but no sibling is registered, so
      // there is no exclusivity to show: no radio group, no "Agent default".
      const lone = await screen.findByTestId('launch-option-13');
      expect(lone).toHaveAttribute('type', 'checkbox');
      expect(
        screen.queryByTestId('launch-option-group---permission-mode'),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByTestId('launch-option-group---permission-mode-none'),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole('radiogroup', { name: '--permission-mode' }),
      ).not.toBeInTheDocument();
      // The two-row `--model` group is still the only radio group.
      expect(screen.getAllByRole('radiogroup')).toEqual([
        screen.getByTestId('launch-option-group---model'),
      ]);
    });

    it('replaces a sibling when another row of the group is chosen', async () => {
      useComposerStore.setState({
        newSessionLaunchOptionIds: [],
        newSessionLaunchOptionsSeeded: true,
      });
      serveAModelGroup();
      renderPicker();
      fireEvent.click(await screen.findByTestId('launch-option-12'));
      fireEvent.click(screen.getByTestId('launch-option-10'));
      await waitFor(() => {
        expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([
          12, 10,
        ]);
      });

      // Choosing the sibling drops the first choice; the rest keeps its order.
      fireEvent.click(screen.getByTestId('launch-option-11'));
      await waitFor(() => {
        expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([
          12, 11,
        ]);
      });
      expect(screen.getByTestId('launch-option-10')).not.toBeChecked();
      expect(screen.getByTestId('launch-option-11')).toBeChecked();

      // "Agent default" clears the group and nothing else.
      fireEvent.click(screen.getByTestId('launch-option-group---model-none'));
      await waitFor(() => {
        expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([
          12,
        ]);
      });
    });

    it('seeds only the first default of a group that carries two', async () => {
      // Rows stored before the one-default rule can both say `default_enabled`.
      serveAModelGroup({ opus: true, custom: true });
      renderPicker();
      await screen.findByTestId('launch-option-10');
      await waitFor(() => {
        expect(useComposerStore.getState().newSessionLaunchOptionIds).toEqual([
          10,
        ]);
      });
      expect(screen.getByTestId('launch-option-10')).toBeChecked();
      expect(screen.getByTestId('launch-option-11')).not.toBeChecked();
    });

    it('still warns when a dangerous grouped row is chosen', async () => {
      serveAModelGroup();
      renderPicker();
      fireEvent.click(await screen.findByTestId('launch-option-13'));
      const alert = await screen.findByRole('alert');
      expect(alert).toHaveTextContent('--permission-mode');

      fireEvent.click(screen.getByTestId('launch-option-13'));
      await waitFor(() => {
        expect(screen.queryByRole('alert')).not.toBeInTheDocument();
      });
    });
  });
});
