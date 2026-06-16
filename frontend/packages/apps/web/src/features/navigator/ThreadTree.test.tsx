import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import type { Thread } from '@delta/wire-gen';
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

  it('lists sub-threads only (not main) and selecting one invokes the callback', () => {
    const onSelectThread = vi.fn();
    render(<ThreadTree threads={threads} onSelectThread={onSelectThread} />);

    // The main thread is reached via the session header, not listed in the
    // tree; only its sub-threads appear.
    expect(screen.queryByText('main')).not.toBeInTheDocument();
    expect(screen.getByText('branch one')).toBeInTheDocument();

    // The tree delegates selection to the session card (which focuses the
    // owning session and activates the thread) rather than touching the store
    // directly, so a click on a non-focused session's tree can switch focus.
    fireEvent.click(screen.getByText('branch one'));
    expect(onSelectThread).toHaveBeenCalledWith(2);
  });

  it('shows a per-thread running spinner only on running threads', () => {
    render(
      <ThreadTree
        threads={threads}
        runningThreads={{ 2: true }}
        onSelectThread={() => {}}
      />,
    );

    // The sub-thread (id 2) is running, so its row carries the spinner; with a
    // single sub-thread rendered, exactly one spinner is present.
    const spinners = screen.getAllByTestId('thread-running');
    expect(spinners).toHaveLength(1);
    expect(spinners[0]).toHaveTextContent('running');
  });

  it('shows no per-thread spinner when no thread is running', () => {
    render(<ThreadTree threads={threads} onSelectThread={() => {}} />);

    expect(screen.queryByTestId('thread-running')).not.toBeInTheDocument();
  });

  it('shows the per-thread spinner when only a launched subagent is running', () => {
    // The sub-thread (id 2) has no in-flight turn but launched a background
    // subagent that is still running, so the thread reads as running.
    render(
      <ThreadTree
        threads={threads}
        runningSubagents={[
          {
            threadId: 2,
            toolUseId: 'toolu_bg',
            subagentType: null,
            description: null,
            background: true,
          },
        ]}
        onSelectThread={() => {}}
      />,
    );

    const spinners = screen.getAllByTestId('thread-running');
    expect(spinners).toHaveLength(1);
  });

  it('suppresses the per-thread unread badge while a launched subagent runs', () => {
    // The thread's turn completed (unread 3) but its background subagent is
    // still working: the badge is held back until the subagent finishes.
    useLiveStore.setState({ unread: { 2: 3 } });
    render(
      <ThreadTree
        threads={threads}
        runningSubagents={[
          {
            threadId: 2,
            toolUseId: 'toolu_bg',
            subagentType: null,
            description: null,
            background: true,
          },
        ]}
        onSelectThread={() => {}}
      />,
    );

    expect(screen.getByTestId('thread-running')).toBeInTheDocument();
    expect(screen.queryByText('3')).not.toBeInTheDocument();
  });

  it('shows an unread badge for inactive threads and hides it for the active one', () => {
    useLiveStore.setState({ unread: { 2: 3 } });
    const { rerender } = render(
      <ThreadTree threads={threads} onSelectThread={() => {}} />,
    );

    expect(screen.getByText('3')).toBeInTheDocument();

    // Activation hides the badge on the active thread. (Clearing the stored
    // count is centralized in the workspace, not the tree.)
    act(() => {
      useNavStore.setState({ activeThreadId: 2 });
    });
    rerender(<ThreadTree threads={threads} onSelectThread={() => {}} />);
    expect(screen.queryByText('3')).not.toBeInTheDocument();
  });
});
