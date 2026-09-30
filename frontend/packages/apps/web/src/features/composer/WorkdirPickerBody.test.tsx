import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import { createHandlers } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { WorkdirPickerBody } from './WorkdirPickerBody';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

/** Every `GET /api/workdir/list` URL the picker requested, in order. */
function recordListRequests(): URL[] {
  const urls: URL[] = [];
  server.events.on('request:start', ({ request }) => {
    const url = new URL(request.url);
    if (url.pathname === '/api/workdir/list') {
      urls.push(url);
    }
  });
  return urls;
}

function renderPicker() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  const setCandidate = vi.fn();
  render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <WorkdirPickerBody
          active
          candidate={null}
          setCandidate={setCandidate}
          onConfirm={vi.fn()}
        />
      </ApiProvider>
    </QueryClientProvider>,
  );
  return { setCandidate };
}

describe('WorkdirPickerBody', () => {
  afterEach(() => server.events.removeAllListeners());

  it('hides dot-directories until "Show hidden" is turned on', async () => {
    const requests = recordListRequests();
    renderPicker();

    // Off by default: $HOME lists its visible directories only.
    const toggle = await screen.findByRole('checkbox', { name: 'Show hidden' });
    expect(toggle).not.toBeChecked();
    await screen.findByTitle('/home/dev/projects');
    expect(screen.queryByTitle('/home/dev/.config')).not.toBeInTheDocument();
    expect(
      requests.every((url) => !url.searchParams.has('hidden')),
    ).toBe(true);

    // Turning it on re-lists the same directory with the hidden flag, and the
    // dot-directory renders like any other row.
    fireEvent.click(toggle);
    expect(toggle).toBeChecked();
    // While the hidden-inclusive listing loads, the rows already on screen
    // stay put rather than flashing to a loading state.
    expect(screen.getByTitle('/home/dev/projects')).toBeInTheDocument();
    const hiddenRow = await screen.findByTitle('/home/dev/.config');
    expect(hiddenRow).toHaveTextContent('.config/');
    await waitFor(() =>
      expect(
        requests.some((url) => url.searchParams.get('hidden') === 'true'),
      ).toBe(true),
    );
    expect(screen.getByTitle('/home/dev/projects')).toBeInTheDocument();
  });
});
