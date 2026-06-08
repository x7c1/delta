import { beforeEach, describe, expect, it } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import type { Thread } from '@delta/model';
import { ThreadTree } from './ThreadTree';
import { useNavStore } from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';

const threads: Thread[] = [
  {
    id: 1,
    session_id: 's',
    title: 'main',
    parent_thread_id: null,
    root_message_uuid: null,
    created_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 2,
    session_id: 's',
    title: 'branch one',
    parent_thread_id: 1,
    root_message_uuid: 'uuid-a',
    created_at: '2026-01-01T00:01:00Z',
  },
];

describe('ThreadTree', () => {
  beforeEach(() => {
    useNavStore.setState({ activeThreadId: null });
    useLiveStore.setState({ unread: {} });
  });

  it('lists sub-threads only (not main) and selecting one sets it active', () => {
    render(<ThreadTree threads={threads} />);

    // The main thread is reached via the session header, not listed in the
    // tree; only its sub-threads appear.
    expect(screen.queryByText('main')).not.toBeInTheDocument();
    expect(screen.getByText('branch one')).toBeInTheDocument();

    fireEvent.click(screen.getByText('branch one'));
    expect(useNavStore.getState().activeThreadId).toBe(2);
  });

  it('shows an unread badge for inactive threads and hides it for the active one', () => {
    useLiveStore.setState({ unread: { 2: 3 } });
    const { rerender } = render(<ThreadTree threads={threads} />);

    expect(screen.getByText('3')).toBeInTheDocument();

    // Activation hides the badge on the active thread. (Clearing the stored
    // count is centralized in the workspace, not the tree.)
    act(() => {
      useNavStore.setState({ activeThreadId: 2 });
    });
    rerender(<ThreadTree threads={threads} />);
    expect(screen.queryByText('3')).not.toBeInTheDocument();
  });
});
