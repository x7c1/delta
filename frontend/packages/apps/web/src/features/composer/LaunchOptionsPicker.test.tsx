import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
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

    // Deselecting one leaves the rest, order preserved.
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
});
