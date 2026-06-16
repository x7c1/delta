import {
  buildThreadTree,
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
 */
export function ThreadTree({
  threads,
  runningThreads,
  runningSubagents,
  onSelectThread,
}: ThreadTreeProps) {
  const roots = buildThreadTree(threads);
  const subThreads = roots.flatMap((root) => root.children);
  return (
    <ul className="py-1">
      {subThreads.map((node) => (
        <ThreadTreeNode
          key={node.thread.id}
          node={node}
          depth={0}
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
  runningThreads,
  runningSubagents,
  onSelectThread,
}: {
  node: ThreadNode<Thread>;
  depth: number;
  runningThreads?: Record<ThreadId, true>;
  runningSubagents?: SubagentActivity[];
  onSelectThread: (threadId: ThreadId) => void;
}) {
  const activeThreadId = useNavStore((state) => state.activeThreadId);
  const unread = useLiveStore((state) => state.unread[node.thread.id] ?? 0);

  const isActive = activeThreadId === node.thread.id;
  // A thread is running when it has an in-flight turn OR a still-running
  // subagent it launched (the latter outlives the turn for a background
  // subagent), so the spinner and the unread suppression below both account for
  // a working subagent.
  const running = threadIsRunning(
    runningThreads,
    runningSubagents,
    node.thread.id,
  );

  return (
    <li>
      <button
        type="button"
        onClick={() => onSelectThread(node.thread.id)}
        style={{ paddingLeft: `${0.5 + depth * 0.85}rem` }}
        className={cn(
          'flex w-full items-center justify-between gap-2 py-0.5 pr-2 text-left text-[13px] leading-5 hover:bg-slate-100',
          isActive && 'bg-indigo-50 font-medium text-indigo-800',
        )}
        aria-current={isActive ? 'true' : undefined}
      >
        <span className="truncate">
          {/* Every node rendered here is a sub-thread (main is not listed), so
              all levels get the branch marker — including the first level, now
              lifted to depth 0. Indentation still grows with depth. */}
          <span className="text-slate-400">⤷ </span>
          {node.thread.title}
        </span>
        <span className="flex shrink-0 items-center gap-1.5">
          {running && (
            // Per-thread running spinner: this exact thread has an in-flight
            // turn. Mirrors the session row's spinner but scoped to the thread,
            // so a user can see which branch is processing.
            <span data-testid="thread-running">
              <Spinner />
              <span className="sr-only">running</span>
            </span>
          )}
          {/* Suppress the unread badge while this thread is running (mirrors the
              session row's `unread && !running`): a thread whose launched
              subagent is still working reads as running, not "done while you
              were away" — the badge surfaces once the subagent finishes and the
              spinner clears. */}
          {unread > 0 && !isActive && !running && (
            <Badge tone="count">{unread}</Badge>
          )}
        </span>
      </button>
      {node.children.length > 0 && (
        <ul>
          {node.children.map((child) => (
            <ThreadTreeNode
              key={child.thread.id}
              node={child}
              depth={depth + 1}
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
