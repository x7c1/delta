import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { fireEvent, screen, within } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers } from '@delta/api-mocks';
import type { PruneSessionsResponse } from '@delta/wire-gen';
import { PruneSessionsBlock } from './PruneSessionsBlock';
import { renderWithApi } from './testSupport';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
beforeEach(() => server.resetHandlers(...createHandlers()));
afterAll(() => server.close());

/** Answer the preview with `count` sessions, recording each query string. */
function previewAnswering(count: number, queries: string[] = []) {
  server.use(
    http.get('*/api/sessions/prune', ({ request }) => {
      queries.push(new URL(request.url).search);
      return HttpResponse.json({
        count,
        session_ids: Array.from({ length: count }, (_, i) => `s-${i}`),
      });
    }),
  );
}

describe('PruneSessionsBlock', () => {
  it('previews the count for the chosen age and statuses', async () => {
    const queries: string[] = [];
    previewAnswering(12, queries);
    renderWithApi(<PruneSessionsBlock active />);

    const preview = screen.getByTestId('storage-prune-preview');
    expect(await within(preview).findByText('12 closed sessions older than 30 days.')).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText('Which closed sessions'), {
      target: { value: 'failed' },
    });
    fireEvent.change(screen.getByLabelText('Days since last activity'), {
      target: { value: '7' },
    });

    expect(await within(preview).findByText('12 failed sessions older than 7 days.')).toBeInTheDocument();
    expect(queries).toContain('?older_than_days=30&statuses=ended%2Cfailed');
    expect(queries).toContain('?older_than_days=7&statuses=failed');
  });

  it('asks for a whole number of days and offers no removal until it gets one', async () => {
    previewAnswering(3);
    renderWithApi(<PruneSessionsBlock active />);

    fireEvent.change(screen.getByLabelText('Days since last activity'), {
      target: { value: '' },
    });

    expect(screen.getByTestId('storage-prune-preview')).toHaveTextContent(
      'Enter a whole number of days.',
    );
    expect(screen.getByRole('button', { name: 'Remove…' })).toBeDisabled();
  });

  it('offers no removal when nothing matches', async () => {
    previewAnswering(0);
    renderWithApi(<PruneSessionsBlock active />);

    expect(await screen.findByText('0 closed sessions older than 30 days.')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Remove…' })).toBeDisabled();
  });

  it('removes only after a confirmation naming the count, then reports the result', async () => {
    previewAnswering(2);
    let posted: unknown = null;
    const report: PruneSessionsResponse = {
      removed: 2,
      removed_ids: ['s-0', 's-1'],
      skipped: [{ session_id: 's-open', reason: 'open', detail: null }],
      kept: [
        {
          session_id: 's-1',
          kind: 'worktree',
          target: '/w/x7c1-delta-s-1',
          reason: 'dirty',
          detail: null,
        },
        {
          session_id: 's-1',
          kind: 'branch',
          target: 'delta-s-1',
          reason: 'worktree_kept',
          detail: null,
        },
      ],
    };
    server.use(
      http.post('*/api/sessions/prune', async ({ request }) => {
        posted = await request.json();
        return HttpResponse.json(report);
      }),
    );
    renderWithApi(<PruneSessionsBlock active />);
    await screen.findByText('2 closed sessions older than 30 days.');

    fireEvent.click(screen.getByRole('button', { name: 'Remove…' }));
    const confirm = screen.getByTestId('storage-prune-confirm');
    expect(confirm).toHaveTextContent('Remove 2 closed sessions older than 30 days from Delta?');
    expect(posted).toBeNull();
    fireEvent.click(within(confirm).getByRole('button', { name: 'Remove 2 sessions' }));

    const result = await screen.findByTestId('storage-prune-result');
    expect(posted).toEqual({ older_than_days: 30, statuses: ['ended', 'failed'] });
    expect(result).toHaveTextContent('Removed 2 sessions; skipped 1.');
    expect(within(result).getByTestId('storage-prune-skipped')).toHaveTextContent(
      'Skipped s-open: it is open',
    );
    const kept = within(result).getByTestId('storage-prune-kept');
    expect(kept).toHaveTextContent(
      'Worktree /w/x7c1-delta-s-1, because it has uncommitted or untracked files.',
    );
    expect(kept).toHaveTextContent('Branch delta-s-1, because its worktree was kept.');
    expect(screen.queryByTestId('storage-prune-confirm')).toBeNull();
  });

  it('cancelling the confirmation removes nothing', async () => {
    previewAnswering(1);
    let posts = 0;
    server.use(
      http.post('*/api/sessions/prune', () => {
        posts += 1;
        return HttpResponse.json({ removed: 0, removed_ids: [], skipped: [], kept: [] });
      }),
    );
    renderWithApi(<PruneSessionsBlock active />);
    await screen.findByText('1 closed session older than 30 days.');

    fireEvent.click(screen.getByRole('button', { name: 'Remove…' }));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(screen.queryByTestId('storage-prune-confirm')).toBeNull();
    expect(posts).toBe(0);
  });

  it('does not fetch the preview while inactive', () => {
    const queries: string[] = [];
    previewAnswering(5, queries);
    renderWithApi(<PruneSessionsBlock active={false} />);

    expect(screen.getByTestId('storage-prune-preview')).toHaveTextContent('Counting…');
    expect(queries).toEqual([]);
  });
});
