import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers, mockStorageWorktrees } from '@delta/api-mocks';
import { exactTime } from './formatAge';
import { WorktreesBlock } from './WorktreesBlock';
import { renderWithApi } from './testSupport';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledFrame: 'error' }));
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
      [clean, 'No uncommitted changes', true],
      [dirty, 'Has uncommitted changes', true],
      [unregistered, 'Not registered with git, so changes are unknown', true],
      [owned, 'No uncommitted changes', false],
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

  it('groups the leftovers first and the worktrees in use after, each newest first', async () => {
    renderWithApi(<WorktreesBlock active />);

    const leftovers = await screen.findByTestId('storage-worktrees-leftover');
    const inUse = screen.getByTestId('storage-worktrees-in-use');
    const list = screen.getByTestId('storage-worktree-list');
    expect(Array.from(list.children)).toEqual([leftovers, inUse]);
    expect(within(leftovers).getByRole('heading')).toHaveTextContent('Leftovers (3)');
    expect(within(inUse).getByRole('heading')).toHaveTextContent(
      'In use by a listed session (1)',
    );
    const paths = (group: HTMLElement) =>
      within(group)
        .getAllByTestId('storage-worktree')
        .map((row) => within(row).getByTitle(/\/worktrees\//).getAttribute('title'));
    // The fixtures' modification times: dirty, then clean, then unregistered.
    expect(paths(leftovers)).toEqual([dirty.path, clean.path, unregistered.path]);
    expect(paths(inUse)).toEqual([owned.path]);
  });

  it('puts a worktree without a modification time last, with no age', async () => {
    server.use(
      http.get('*/api/storage/worktrees', () =>
        HttpResponse.json({
          worktrees: [
            { ...clean, modified_at: null },
            { ...dirty, modified_at: '2026-01-01T00:00:00Z' },
          ],
        }),
      ),
    );
    renderWithApi(<WorktreesBlock active />);

    const rows = await screen.findAllByTestId('storage-worktree');
    expect(within(rows[0]).getByTitle(dirty.path, { exact: true })).toBeInTheDocument();
    expect(within(rows[0]).getByTestId('storage-worktree-age')).toHaveTextContent(
      /^Modified .+ ago$/,
    );
    expect(within(rows[1]).getByTitle(clean.path, { exact: true })).toBeInTheDocument();
    expect(within(rows[1]).queryByTestId('storage-worktree-age')).toBeNull();
  });

  it('shows each worktree\'s age, with the exact time in a tooltip', async () => {
    vi.useFakeTimers({ toFake: ['Date'], now: new Date('2026-10-13T09:00:00Z') });
    try {
      renderWithApi(<WorktreesBlock active />);

      const age = within(await rowOf(dirty.path)).getByTestId('storage-worktree-age');
      expect(age).toHaveTextContent('Modified 3 days ago');
      expect(age).toHaveAttribute('title', exactTime(new Date(dirty.modified_at!)));
    } finally {
      vi.useRealTimers();
    }
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

  it('keeps a refused row and its panel among the leftovers when a session took it', async () => {
    let listed = 0;
    server.use(
      http.get('*/api/storage/worktrees', () => {
        listed += 1;
        return HttpResponse.json({
          worktrees: [listed === 1 ? clean : { ...clean, in_use: true }],
        });
      }),
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
    await waitFor(() =>
      expect(within(row).getByRole('button', { name: 'Remove worktree' })).toBeDisabled(),
    );
    expect(row).toBeInTheDocument();
    expect(within(row).getByRole('alert')).toBeInTheDocument();
    expect(screen.queryByTestId('storage-worktrees-in-use')).toBeNull();

    fireEvent.click(within(row).getByRole('button', { name: 'Cancel' }));

    const inUse = await screen.findByTestId('storage-worktrees-in-use');
    expect(within(inUse).getByTitle(clean.path, { exact: true })).toBeInTheDocument();
    expect(screen.queryByTestId('storage-worktrees-leftover')).toBeNull();
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

  it('gives the focus back to the row\'s Remove when the confirmation is cancelled', async () => {
    renderWithApi(<WorktreesBlock active />);
    const row = await rowOf(clean.path);
    const opener = () => within(row).getByRole('button', { name: /^Remove worktree/ });

    fireEvent.click(opener());
    fireEvent.click(within(row).getByRole('button', { name: 'Cancel' }));

    expect(within(row).queryByTestId('storage-worktree-confirm')).toBeNull();
    expect(opener()).toHaveFocus();

    fireEvent.click(opener());
    fireEvent.keyDown(within(row).getByRole('button', { name: 'Cancel' }), { key: 'Escape' });

    expect(within(row).queryByTestId('storage-worktree-confirm')).toBeNull();
    expect(opener()).toHaveFocus();
  });

  it('leaves out a group with no worktrees', async () => {
    server.use(
      http.get('*/api/storage/worktrees', () => HttpResponse.json({ worktrees: [owned] })),
    );
    renderWithApi(<WorktreesBlock active />);

    expect(await screen.findByTestId('storage-worktrees-in-use')).toBeInTheDocument();
    expect(screen.queryByTestId('storage-worktrees-leftover')).toBeNull();
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
