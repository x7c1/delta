import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
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
    // A fresh, not-yet-seeded new-session compose state.
    useComposerStore.setState({
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
});
