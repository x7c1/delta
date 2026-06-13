import {
  afterAll,
  afterEach,
  beforeAll,
  describe,
  expect,
  it,
} from 'vitest';
import type { ComponentProps } from 'react';
import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import { createHandlers, SESSION_ID } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type { SessionListItem } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { SessionNode } from './SessionNode';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

const item: SessionListItem = {
  session: {
    id: SESSION_ID,
    cwd: '/home/dev/project',
    transcript_path: '',
    title: null,
    status: 'active',
    created_at: '2026-01-01T00:00:00Z',
  },
  open: true,
  main_thread_id: 1,
  last_activity_at: '2026-01-01T00:00:00Z',
};

function renderNode(props: Partial<ComponentProps<typeof SessionNode>>) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <SessionNode
          item={item}
          isFocused={false}
          onFocus={() => {}}
          onClose={() => {}}
          {...props}
        />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('SessionNode running indicator', () => {
  it('renders the running indicator when running is true', () => {
    renderNode({ running: true });

    const running = screen.getByTestId('session-running');
    expect(running).toBeInTheDocument();
    // The compact glyph is aria-hidden, so an accessible "running" label is
    // paired with it for assistive tech.
    expect(running).toHaveTextContent('running');
  });

  it('does not render the running indicator when running is false', () => {
    renderNode({ running: false });

    expect(screen.queryByTestId('session-running')).not.toBeInTheDocument();
  });

  it('shows the running indicator and the permission badge together', () => {
    renderNode({ running: true, needsPermission: true });

    expect(screen.getByTestId('session-running')).toBeInTheDocument();
    expect(
      screen.getByTestId('session-permission-badge'),
    ).toBeInTheDocument();
  });
});
