import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers, mockStorageWorktrees } from '@delta/api-mocks';
import { WorktreesBlock } from './WorktreesBlock';
import { renderWithApi } from './testSupport';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
beforeEach(() => {
  server.resetHandlers(...createHandlers());
  server.events.removeAllListeners();
});
afterAll(() => server.close());

const [clean, dirty, unregistered, owned] = mockStorageWorktrees;

/** The row of the worktree at `path`. */
async function rowOf(path: string) {
  const rows = await screen.findAllByTestId('storage-worktree');
  const row = rows.find((candidate) => within(candidate).queryByTitle(path, { exact: true }));
  if (!row) {
    throw new Error(`no row for ${path}`);
  }
  return row;
}

/** Record every removal request's body. */
function recordRemovals(): unknown[] {
  const bodies: unknown[] = [];
  server.events.on('request:start', async ({ request }) => {
    if (request.method === 'DELETE' && request.url.endsWith('/api/storage/worktrees')) {
      bodies.push(await request.clone().json());
    }
  });
  return bodies;
}

describe('WorktreesBlock', () => {
  it('lists every worktree with its state and offers removal only on leftovers', async () => {
    renderWithApi(<WorktreesBlock active />);

    const expectations: [typeof clean, string, boolean][] = [
      [clean, 'Clean', true],
      [dirty, 'Has uncommitted changes', true],
      [unregistered, 'Not registered with git', true],
      [owned, 'In use by a listed session', false],
    ];
    for (const [worktree, state, removable] of expectations) {
      const row = await rowOf(worktree.path);
      expect(within(row).getByTestId('storage-worktree-state')).toHaveTextContent(state);
      expect(within(row).queryByRole('button', { name: /^Remove worktree/ }) !== null).toBe(
        removable,
      );
    }
    expect(within(await rowOf(clean.path)).getByText(clean.repo_root!)).toBeInTheDocument();
  });

  it('removes a clean leftover after a plain confirmation, without force', async () => {
    const bodies = recordRemovals();
    renderWithApi(<WorktreesBlock active />);
    const row = await rowOf(clean.path);

    fireEvent.click(within(row).getByRole('button', { name: /^Remove worktree/ }));
    const confirm = within(row).getByTestId('storage-worktree-confirm');
    expect(within(confirm).queryByLabelText('Directory name to confirm')).toBeNull();
    fireEvent.click(within(confirm).getByRole('button', { name: 'Remove worktree' }));

    await waitFor(() =>
      expect(screen.queryByTitle(clean.path, { exact: true })).toBeNull(),
    );
    expect(bodies).toEqual([{ path: clean.path, force: false }]);
  });

  it('holds a forced removal back until the directory name is typed', async () => {
    const bodies = recordRemovals();
    renderWithApi(<WorktreesBlock active />);
    const row = await rowOf(dirty.path);
    const name = dirty.path.slice(dirty.path.lastIndexOf('/') + 1);

    fireEvent.click(within(row).getByRole('button', { name: /^Remove worktree/ }));
    const confirm = within(row).getByTestId('storage-worktree-confirm');
    expect(confirm).toHaveTextContent('Removing it deletes them');
    const remove = within(confirm).getByRole('button', { name: 'Remove and lose changes' });
    expect(remove).toBeDisabled();
    const input = within(confirm).getByLabelText('Directory name to confirm');
    fireEvent.change(input, { target: { value: 'wrong' } });
    expect(remove).toBeDisabled();
    fireEvent.change(input, { target: { value: name } });
    expect(remove).toBeEnabled();
    fireEvent.click(remove);

    await waitFor(() =>
      expect(screen.queryByTitle(dirty.path, { exact: true })).toBeNull(),
    );
    expect(bodies).toEqual([{ path: dirty.path, force: true }]);
  });

  it('says why when the server refuses the removal', async () => {
    server.use(
      http.delete('*/api/storage/worktrees', () =>
        HttpResponse.json(
          { error: 'a listed session still works in this worktree', code: 'worktree_in_use' },
          { status: 409 },
        ),
      ),
    );
    renderWithApi(<WorktreesBlock active />);
    const row = await rowOf(clean.path);

    fireEvent.click(within(row).getByRole('button', { name: /^Remove worktree/ }));
    fireEvent.click(within(row).getByRole('button', { name: 'Remove worktree' }));

    expect(await within(row).findByRole('alert')).toHaveTextContent(
      'A listed session works in it now. Remove that session instead.',
    );
  });

  it('asks for the name in the open panel when a clean worktree gained changes', async () => {
    let listed = 0;
    server.use(
      http.get('*/api/storage/worktrees', () => {
        listed += 1;
        return HttpResponse.json({
          worktrees: [listed === 1 ? clean : { ...clean, dirty: true }],
        });
      }),
      http.delete('*/api/storage/worktrees', () =>
        HttpResponse.json(
          { error: 'this worktree has uncommitted changes', code: 'worktree_dirty' },
          { status: 409 },
        ),
      ),
    );
    renderWithApi(<WorktreesBlock active />);
    const row = await rowOf(clean.path);

    fireEvent.click(within(row).getByRole('button', { name: /^Remove worktree/ }));
    expect(within(row).getByRole('button', { name: 'Cancel' })).toHaveFocus();
    fireEvent.click(within(row).getByRole('button', { name: 'Remove worktree' }));

    expect(await within(row).findByRole('alert')).toHaveTextContent(
      'To remove it and lose them, type its name above.',
    );
    expect(await within(row).findByLabelText('Directory name to confirm')).toBeInTheDocument();
    expect(within(row).getByRole('button', { name: 'Remove and lose changes' })).toBeDisabled();
  });

  it('says so when there are no worktrees, and fetches nothing while inactive', async () => {
    let fetched = 0;
    server.use(
      http.get('*/api/storage/worktrees', () => {
        fetched += 1;
        return HttpResponse.json({ worktrees: [] });
      }),
    );
    const { unmount } = renderWithApi(<WorktreesBlock active={false} />);
    expect(fetched).toBe(0);
    unmount();

    renderWithApi(<WorktreesBlock active />);
    expect(await screen.findByTestId('storage-no-worktrees')).toHaveTextContent('No worktrees.');
  });
});
