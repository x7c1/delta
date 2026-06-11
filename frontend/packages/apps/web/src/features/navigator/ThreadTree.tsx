import {
  buildThreadTree,
  type ThreadId,
  type ThreadNode,
} from '@delta/model';
import type { Thread } from '@delta/wire-gen';
import { Badge, cn } from '@delta/ui-kit';
import { useNavStore } from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';

export interface ThreadTreeProps {
  threads: Thread[];
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
export function ThreadTree({ threads, onSelectThread }: ThreadTreeProps) {
  const roots = buildThreadTree(threads);
  const subThreads = roots.flatMap((root) => root.children);
  return (
    <ul className="py-1">
      {subThreads.map((node) => (
        <ThreadTreeNode
          key={node.thread.id}
          node={node}
          depth={0}
          onSelectThread={onSelectThread}
        />
      ))}
    </ul>
  );
}

function ThreadTreeNode({
  node,
  depth,
  onSelectThread,
}: {
  node: ThreadNode<Thread>;
  depth: number;
  onSelectThread: (threadId: ThreadId) => void;
}) {
  const activeThreadId = useNavStore((state) => state.activeThreadId);
  const unread = useLiveStore((state) => state.unread[node.thread.id] ?? 0);

  const isActive = activeThreadId === node.thread.id;

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
        {unread > 0 && !isActive && <Badge tone="count">{unread}</Badge>}
      </button>
      {node.children.length > 0 && (
        <ul>
          {node.children.map((child) => (
            <ThreadTreeNode
              key={child.thread.id}
              node={child}
              depth={depth + 1}
              onSelectThread={onSelectThread}
            />
          ))}
        </ul>
      )}
    </li>
  );
}
