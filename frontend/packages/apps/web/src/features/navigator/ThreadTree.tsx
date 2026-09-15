import {
  buildThreadTree,
  newestThreadId,
  threadDisplayName,
  threadTooltip,
  type ThreadId,
  type ThreadNode,
} from '@delta/model';
import type { Thread } from '@delta/wire-gen';
import { Badge, Spinner, cn } from '@delta/ui-kit';
import { useNavStore } from '../../store/navStore';
import {
  threadIsRunning,
  useLiveStore,
  type SubagentActivity,
} from '../../store/liveStore';

export interface ThreadTreeProps {
  threads: Thread[];
  /**
   * The session's running threads (the `runningThreads[sessionId]` record), so
   * each node can show its own running spinner. Passed down rather than read per
   * node so the whole tree reads one consistent snapshot. `undefined` when no
   * thread of the session is running.
   */
  runningThreads?: Record<ThreadId, true>;
  /**
   * The session's running subagents (the `runningSubagents[sessionId]` list).
   * A thread that launched a still-running subagent — a BACKGROUND one in
   * particular, which outlives its launching turn — reads as "running": its
   * spinner stays lit and its per-thread unread badge is suppressed until the
   * subagent finishes. Passed down (not read per node) so the whole tree reads
   * one consistent snapshot. `undefined` when none is running.
   */
  runningSubagents?: SubagentActivity[];
  /**
   * Select a sub-thread. The session card supplies this so a click can do more
   * than set the active thread — for a non-focused session it also focuses the
   * session, so the center pane switches to it (see {@link SessionNode}).
   */
  onSelectThread: (threadId: ThreadId) => void;
}

/**
 * The thread navigator tree (named nodes only, no messages). Clicking a node
 * invokes {@link ThreadTreeProps.onSelectThread}; activation clears the thread's
 * unread badge (handled centrally by the workspace, so every activation path
 * clears it). Siblings keep creation order.
 *
 * The main thread is intentionally not listed: it is always present, so a row
 * for it is redundant. The main thread is reached instead by clicking the
 * session card header (see {@link NavigatorPane}). Only main's sub-threads are
 * rendered, lifted to depth 0 so they sit directly under the session without a
 * redundant indent level.
 *
 * Exactly one row can carry the "most recently active" mark. The ranking runs
 * over EVERY thread of the session, main included, so when main is where the
 * last message landed the winner is a thread this tree never draws and no row
 * is marked — which is the intended reading of an unmarked tree.
 */
export function ThreadTree({
  threads,
  runningThreads,
  runningSubagents,
  onSelectThread,
}: ThreadTreeProps) {
  // Live activity outranks the query's `last_activity_at`: the threads query
  // supplies the initial values, and these keep the mark current while the
  // session is open, with no refetch in between.
  const threadActivity = useLiveStore((state) => state.threadActivity);
  const newestId = newestThreadId(
    threads.map((thread) => ({
      id: thread.id,
      last_activity_at: threadActivity[thread.id] ?? thread.last_activity_at,
    })),
  );
  const roots = buildThreadTree(threads);
  const subThreads = roots.flatMap((root) => root.children);
  return (
    <ul className="py-1">
      {subThreads.map((node) => (
        <ThreadTreeNode
          key={node.thread.id}
          node={node}
          depth={0}
          newestId={newestId}
          runningThreads={runningThreads}
          runningSubagents={runningSubagents}
          onSelectThread={onSelectThread}
        />
      ))}
    </ul>
  );
}

function ThreadTreeNode({
  node,
  depth,
  newestId,
  runningThreads,
  runningSubagents,
  onSelectThread,
}: {
  node: ThreadNode<Thread>;
  depth: number;
  /**
   * The session's most recently active thread, computed once for the whole tree
   * so every row reads one consistent answer. `undefined` when no thread has
   * any activity.
   */
  newestId: ThreadId | undefined;
  runningThreads?: Record<ThreadId, true>;
  runningSubagents?: SubagentActivity[];
  onSelectThread: (threadId: ThreadId) => void;
}) {
  const activeThreadId = useNavStore((state) => state.activeThreadId);
  const unread = useLiveStore((state) => state.unread[node.thread.id] ?? 0);

  const isActive = activeThreadId === node.thread.id;
  const isNewest = newestId === node.thread.id;
  // A thread is running when it has an in-flight turn OR a still-running
  // subagent it launched (the latter outlives the turn for a background
  // subagent), so the spinner and the unread suppression below both account for
  // a working subagent.
  const running = threadIsRunning(
    runningThreads,
    runningSubagents,
    node.thread.id,
  );
  // Suppress the unread badge while this thread is running (mirrors the session
  // row's `unread && !running`): a thread whose launched subagent is still
  // working reads as running, not "done while you were away" — the badge
  // surfaces once the subagent finishes and the spinner clears.
  const showBadge = unread > 0 && !isActive && !running;

  return (
    <li>
      <button
        type="button"
        onClick={() => onSelectThread(node.thread.id)}
        style={{ paddingLeft: `${0.5 + depth * 0.85}rem` }}
        className={cn(
          'flex w-full items-center justify-between gap-2 py-0.5 pr-2 text-left text-secondary leading-5 hover:bg-surface-elevated-hover',
          isActive && 'bg-accent/10 font-medium text-accent',
        )}
        aria-current={isActive ? 'true' : undefined}
      >
        <span className="truncate" title={threadTooltip(node.thread)}>
          {/* Every node rendered here is a sub-thread (main is not listed), so
              all levels get the branch marker — including the first level, now
              lifted to depth 0. Indentation still grows with depth.

              The arrow's COLOUR carries the most-recently-active mark: the
              marked row draws it in the ordinary foreground instead of the
              subtle grey. The arrow is an element of its own, separate from the
              title, so the mark neither competes with the active row's
              `text-accent` on the title nor disappears when both signals land on
              the same row. Static, so it neither hides nor is hidden by the
              running spinner or the unread badge. */}
          <span className={cn('text-fg-subtle', isNewest && 'text-fg')}>
            ⤷{' '}
          </span>
          {threadDisplayName(node.thread)}
          {isNewest && (
            // The mark itself is purely visual (the arrow's colour above), and
            // colour never reaches assistive tech — so pair it with a
            // visually-hidden label, exactly as the spinner below and the
            // session row's unread dot do.
            //
            // It TRAILS the title rather than sitting inside the arrow: the
            // label joins the row's accessible name, and between arrow and
            // title it would split the "⤷ <title>" opening that locators match
            // a row by (see `e2e/timeline-playhead-follow.spec.ts`).
            <span className="sr-only" data-testid="thread-newest">
              most recent activity
            </span>
          )}
        </span>
        {/* Rendered only when it actually shows something: an empty flex item
            still costs the row's `gap-2`, which truncated long titles a
            character early. */}
        {(running || showBadge) && (
          <span className="flex shrink-0 items-center gap-1.5">
            {running && (
              // Per-thread running spinner: this exact thread has an in-flight
              // turn. Mirrors the session row's spinner but scoped to the
              // thread, so a user can see which branch is processing.
              <span data-testid="thread-running">
                <Spinner />
                <span className="sr-only">running</span>
              </span>
            )}
            {showBadge && <Badge tone="count">{unread}</Badge>}
          </span>
        )}
      </button>
      {node.children.length > 0 && (
        <ul>
          {node.children.map((child) => (
            <ThreadTreeNode
              key={child.thread.id}
              node={child}
              depth={depth + 1}
              newestId={newestId}
              runningThreads={runningThreads}
              runningSubagents={runningSubagents}
              onSelectThread={onSelectThread}
            />
          ))}
        </ul>
      )}
    </li>
  );
}
