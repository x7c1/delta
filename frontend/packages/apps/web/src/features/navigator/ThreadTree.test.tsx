import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { QueryClient } from '@tanstack/react-query';
import type { Thread } from '@delta/wire-gen';
import { ThreadTree } from './ThreadTree';
import { applySessionEvent } from '../../data/applySessionEvent';
import { useNavStore } from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';

const threads: Thread[] = [
  {
    id: 1,
    session_id: 's',
    title: 'main',
    parent_thread_id: null,
    root_message_uuid: null,
    last_activity_at: null,
    created_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 2,
    session_id: 's',
    title: 'branch one',
    parent_thread_id: 1,
    root_message_uuid: 'uuid-a',
    last_activity_at: null,
    created_at: '2026-01-01T00:01:00Z',
  },
];

describe('ThreadTree', () => {
  beforeEach(() => {
    useNavStore.setState({ activeThreadId: null });
    useLiveStore.setState({ unread: {}, threadActivity: {} });
  });

  /** The row button carrying `title`, whose classes hold every row signal. */
  const row = (title: string) => {
    const button = screen.getByText(title).closest('button');
    expect(button).not.toBeNull();
    return button as HTMLButtonElement;
  };

  /** Every rendered row's className, in render order. */
  const rowClasses = () =>
    screen.getAllByRole('button').map((button) => button.className);

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

  describe('the most-recently-active mark', () => {
    // Two sub-threads plus main, so "exactly one row is marked" is a real
    // assertion and the tie/ordering cases have something to choose between.
    const withRecency = (
      main: string | null,
      one: string | null,
      two: string | null,
    ): Thread[] => [
      { ...threads[0], last_activity_at: main },
      { ...threads[1], last_activity_at: one },
      {
        id: 3,
        session_id: 's',
        title: 'branch two',
        parent_thread_id: 1,
        root_message_uuid: 'uuid-b',
        last_activity_at: two,
        created_at: '2026-01-01T00:02:00Z',
      },
    ];

    it('marks exactly the sub-thread with the newest activity, with weight only', () => {
      render(
        <ThreadTree
          threads={withRecency(
            '2026-01-01T00:00:00Z',
            '2026-01-01T00:01:00Z',
            '2026-01-01T00:09:00Z',
          )}
          onSelectThread={() => {}}
        />,
      );

      expect(
        rowClasses().filter((c) => c.includes('font-semibold')),
      ).toHaveLength(1);
      expect(row('branch two').className).toContain('font-semibold');
      // Weight alone: the accent belongs to the ACTIVE row, so the two signals
      // stay readable when they land on different rows.
      expect(row('branch two').className).not.toContain('text-accent');
      expect(row('branch two').className).not.toContain('bg-accent');
      // Weight is invisible to assistive tech, so the marked row — and only it
      // — also carries a visually-hidden label.
      expect(screen.getAllByTestId('thread-newest')).toHaveLength(1);
      expect(row('branch two')).toContainElement(
        screen.getByTestId('thread-newest'),
      );
    });

    it('marks no row when the main thread is the newest', () => {
      // Main is not rendered by this tree, so naming it is exactly how "the last
      // message landed on main" is expressed: nothing is marked. A row is active
      // meanwhile, because the active row carries a weight of its own — the mark
      // sits a step above it so that "no mark" keeps meaning "main is newest".
      useNavStore.setState({ activeThreadId: 2 });
      render(
        <ThreadTree
          threads={withRecency(
            '2026-01-01T00:09:00Z',
            '2026-01-01T00:01:00Z',
            '2026-01-01T00:02:00Z',
          )}
          onSelectThread={() => {}}
        />,
      );

      expect(rowClasses().some((c) => c.includes('font-semibold'))).toBe(false);
      expect(row('branch one').className).toContain('font-medium');
      expect(screen.queryByTestId('thread-newest')).not.toBeInTheDocument();
    });

    it('marks no row when no thread has any activity', () => {
      render(
        <ThreadTree
          threads={withRecency(null, null, null)}
          onSelectThread={() => {}}
        />,
      );

      expect(screen.getByText('branch one')).toBeInTheDocument();
      expect(rowClasses().some((c) => c.includes('font-semibold'))).toBe(false);
    });

    it('coexists with the running spinner and with the active row styling', () => {
      const marked = withRecency(
        null,
        '2026-01-01T00:01:00Z',
        '2026-01-01T00:09:00Z',
      );
      // The marked row is also running: the mark is static, so it neither
      // replaces nor suppresses the spinner.
      const { rerender } = render(
        <ThreadTree
          threads={marked}
          runningThreads={{ 3: true }}
          onSelectThread={() => {}}
        />,
      );
      expect(row('branch two').className).toContain('font-semibold');
      expect(screen.getAllByTestId('thread-running')).toHaveLength(1);

      // The marked row is also the active one: the accent styling still lands,
      // and the mark survives on it — the active row's own weight does not
      // swallow the heavier mark.
      act(() => {
        useNavStore.setState({ activeThreadId: 3 });
      });
      rerender(<ThreadTree threads={marked} onSelectThread={() => {}} />);
      expect(row('branch two').className).toContain('font-semibold');
      expect(row('branch two').className).toContain('text-accent');
      expect(row('branch two').className).toContain('bg-accent/10');
    });

    it('leaves the row order alone whichever row is marked', () => {
      // Sub-threads stay in creation order; recency changes what is marked,
      // never where a row sits.
      // The visible label, not the row's whole `textContent`: the marked row
      // also carries a visually-hidden label, which would make the two renders
      // differ for a reason that has nothing to do with order.
      const order = () =>
        screen
          .getAllByRole('button')
          .map((button) => button.querySelector('.truncate')?.textContent);

      const { rerender } = render(
        <ThreadTree
          threads={withRecency(null, '2026-01-01T00:09:00Z', null)}
          onSelectThread={() => {}}
        />,
      );
      const marksFirst = order();
      expect(row('branch one').className).toContain('font-semibold');

      rerender(
        <ThreadTree
          threads={withRecency(null, null, '2026-01-01T00:09:00Z')}
          onSelectThread={() => {}}
        />,
      );
      expect(row('branch two').className).toContain('font-semibold');
      expect(order()).toEqual(marksFirst);
    });

    it('moves the mark on a live session event, with no refetch, even for the thread being viewed', () => {
      // The query supplies the initial values: `branch one` is the newest.
      const seeded = withRecency(
        null,
        '2026-01-01T00:01:00Z',
        '2026-01-01T00:00:30Z',
      );
      useNavStore.setState({ activeThreadId: 3 });
      const { rerender } = render(
        <ThreadTree threads={seeded} onSelectThread={() => {}} />,
      );
      expect(row('branch one').className).toContain('font-semibold');

      // A turn ends on `branch two` — the very thread being viewed, which the
      // unread bump deliberately skips and this mark deliberately does not.
      // The same `threads` array is passed back in: no refetch is involved.
      act(() => {
        applySessionEvent(
          {
            kind: 'turn_completed',
            session_id: 's',
            thread_id: 3,
            stop_reason: null,
          },
          new QueryClient(),
          3,
          's',
        );
      });
      rerender(<ThreadTree threads={seeded} onSelectThread={() => {}} />);

      // The mark moves onto the active row and is still visible there, so the
      // tree never falls back to the unmarked state that means "main is newest".
      expect(row('branch two').className).toContain('font-semibold');
      expect(row('branch one').className).not.toContain('font-semibold');
      expect(
        rowClasses().filter((c) => c.includes('font-semibold')),
      ).toHaveLength(1);
    });
  });
});
