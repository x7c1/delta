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
    useComposerStore.setState({ newSessionLaunchOptionIds: [] });
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

  it('records selections in click order and drops them on deselect', async () => {
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
});
