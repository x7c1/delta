import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers, mockStorage } from '@delta/api-mocks';
import { StorageSection } from './StorageSection';
import { renderWithApi } from './testSupport';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
beforeEach(() => server.resetHandlers(...createHandlers()));
afterAll(() => server.close());

const [first, second] = mockStorage.snapshots;

describe('SnapshotList', () => {
  it('deletes a snapshot after a confirmation naming the file and its size', async () => {
    renderWithApi(<StorageSection active />);
    const snapshots = await screen.findByTestId('storage-snapshots');

    fireEvent.click(
      within(snapshots).getByRole('button', { name: 'Delete snapshot delta.db.bak-v3' }),
    );
    const confirm = within(snapshots).getByTestId('storage-snapshot-confirm');
    expect(confirm).toHaveTextContent('Delete delta.db.bak-v3 (1.0 MB)?');
    fireEvent.click(within(confirm).getByRole('button', { name: 'Delete snapshot' }));

    await waitFor(() =>
      expect(screen.queryByTitle(first.path, { exact: true })).toBeNull(),
    );
    expect(screen.getByTitle(second.path, { exact: true })).toBeInTheDocument();
  });

  it('keeps the snapshot and says why when the deletion fails', async () => {
    server.use(
      http.delete('*/api/storage/snapshots', () =>
        HttpResponse.json({ error: 'not a listed migration snapshot' }, { status: 404 }),
      ),
    );
    renderWithApi(<StorageSection active />);
    const snapshots = await screen.findByTestId('storage-snapshots');

    fireEvent.click(
      within(snapshots).getByRole('button', { name: 'Delete snapshot delta.db.bak-v5' }),
    );
    fireEvent.click(within(snapshots).getByRole('button', { name: 'Delete snapshot' }));

    expect(await within(snapshots).findByRole('alert')).toHaveTextContent(
      'not a listed migration snapshot',
    );
    expect(screen.getByTitle(second.path, { exact: true })).toBeInTheDocument();
  });

  it('cancelling puts the Delete control back and deletes nothing', async () => {
    renderWithApi(<StorageSection active />);
    const snapshots = await screen.findByTestId('storage-snapshots');

    fireEvent.click(
      within(snapshots).getByRole('button', { name: 'Delete snapshot delta.db.bak-v3' }),
    );
    fireEvent.click(within(snapshots).getByRole('button', { name: 'Cancel' }));

    expect(within(snapshots).queryByTestId('storage-snapshot-confirm')).toBeNull();
    expect(
      within(snapshots).getByRole('button', { name: 'Delete snapshot delta.db.bak-v3' }),
    ).toBeInTheDocument();
    expect(screen.getByTitle(first.path, { exact: true })).toBeInTheDocument();
  });
});
