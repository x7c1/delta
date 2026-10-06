import { useState } from 'react';
import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { fireEvent, screen, within } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { createHandlers, MOCK_DATA_DIR, MOCK_WORKTREE_BASE } from '@delta/api-mocks';
import { EraseBlock } from './EraseBlock';
import { renderWithApi } from './testSupport';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
beforeEach(() => server.resetHandlers(...createHandlers()));
afterAll(() => server.close());

/** Open the confirmation and return the confirming button. */
function openConfirmation(): HTMLElement {
  fireEvent.click(screen.getByRole('button', { name: 'Erase everything…' }));
  return screen.getByRole('button', { name: 'Erase everything and stop Delta' });
}

/** Open the confirmation, type the word, press the button, and return it. */
function confirmErase(): HTMLElement {
  const confirm = openConfirmation();
  fireEvent.change(screen.getByLabelText('Word to confirm'), {
    target: { value: 'erase' },
  });
  fireEvent.click(confirm);
  return confirm;
}

describe('EraseBlock', () => {
  it('names the worktrees that will stay before anything is confirmed', async () => {
    renderWithApi(<EraseBlock active />);

    const kept = await screen.findByTestId('storage-erase-kept');
    expect(within(kept).getByText('x7c1-delta-leftover-dirty')).toBeInTheDocument();
    expect(within(kept).getByText('x7c1-delta-leftover-unregistered')).toBeInTheDocument();
    expect(within(kept).queryByText('x7c1-delta-leftover-clean')).toBeNull();
    expect(within(kept).queryByText('x7c1-delta-session-1')).toBeNull();
    expect(screen.queryByTestId('storage-erase-confirm')).toBeNull();
  });

  it('holds the button back until the word is typed', async () => {
    renderWithApi(<EraseBlock active />);

    const confirm = openConfirmation();
    const input = screen.getByLabelText('Word to confirm');
    expect(input).toHaveFocus();
    expect(confirm).toBeDisabled();
    fireEvent.change(input, { target: { value: 'eras' } });
    expect(confirm).toBeDisabled();
    fireEvent.change(input, { target: { value: 'erase' } });
    expect(confirm).toBeEnabled();

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByTestId('storage-erase-confirm')).toBeNull();
    expect(openConfirmation()).toBeDisabled();
  });

  it('replaces the app with the report once the erase succeeds', async () => {
    renderWithApi(<EraseBlock active />);

    confirmErase();

    const page = await screen.findByTestId('erased-page');
    expect(screen.queryByTestId('storage-erase')).toBeNull();
    const removed = within(page).getByTestId('erased-removed');
    expect(removed).toHaveTextContent(`${MOCK_WORKTREE_BASE}/x7c1-delta-leftover-clean`);
    expect(removed).toHaveTextContent(MOCK_DATA_DIR);
    const kept = within(page).getByTestId('erased-kept');
    expect(kept).toHaveTextContent(
      `Worktree ${MOCK_WORKTREE_BASE}/x7c1-delta-leftover-dirty, because it has uncommitted or untracked files.`,
    );
    expect(kept).toHaveTextContent(
      `Worktree ${MOCK_WORKTREE_BASE}/x7c1-delta-leftover-unregistered, because git does not know it as a worktree.`,
    );
    expect(within(page).getByRole('status')).toHaveTextContent(
      'Delta has stopped. Close this tab.',
    );
  });

  it('says so when no worktree will stay', async () => {
    server.use(
      http.get('*/api/storage/worktrees', () => HttpResponse.json({ worktrees: [] })),
    );
    renderWithApi(<EraseBlock active />);

    expect(await screen.findByTestId('storage-erase-kept')).toHaveTextContent(
      'No worktree will stay: none has changes.',
    );
  });

  it('says so when the worktrees that will stay cannot be listed', async () => {
    server.use(
      http.get('*/api/storage/worktrees', () =>
        HttpResponse.json({ error: 'boom', code: 'internal' }, { status: 500 }),
      ),
    );
    renderWithApi(<EraseBlock active />);

    expect(await screen.findByTestId('storage-erase-kept-error')).toHaveTextContent(
      'Could not list the worktrees that will stay',
    );
    expect(screen.queryByTestId('storage-erase-kept')).toBeNull();
  });

  it('still shows the report when the block was left while the erase ran', async () => {
    let answer: () => void = () => {};
    const answered = new Promise<void>((resolve) => {
      answer = resolve;
    });
    server.use(
      // Holds the request, then falls through to the default handler's answer.
      http.post('*/api/storage/erase', async () => {
        await answered;
      }),
    );
    function Leavable() {
      const [shown, setShown] = useState(true);
      return (
        <>
          <button onClick={() => setShown(false)}>Leave</button>
          {shown && <EraseBlock active />}
        </>
      );
    }
    renderWithApi(<Leavable />);

    confirmErase();
    fireEvent.click(screen.getByRole('button', { name: 'Leave' }));
    expect(screen.queryByTestId('storage-erase')).toBeNull();
    answer();

    expect(await screen.findByTestId('erased-page')).toBeInTheDocument();
  });

  it('says part may be done and lets the user try again when the erase fails', async () => {
    server.use(
      http.post('*/api/storage/erase', () =>
        HttpResponse.json({ error: 'listing the worktree base failed', code: 'internal' }, { status: 500 }),
      ),
    );
    renderWithApi(<EraseBlock active />);

    const confirm = confirmErase();

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Could not finish erasing');
    expect(alert).toHaveTextContent('try again to erase the rest');
    expect(confirm).toBeEnabled();
  });

  it('says so when an erase is already running', async () => {
    server.use(
      http.post('*/api/storage/erase', () =>
        HttpResponse.json(
          { error: 'an erase is already in progress', code: 'erase_in_progress' },
          { status: 409 },
        ),
      ),
    );
    renderWithApi(<EraseBlock active />);

    confirmErase();

    expect(await screen.findByRole('alert')).toHaveTextContent('An erase is already running.');
    expect(screen.queryByTestId('erased-page')).toBeNull();
  });
});
