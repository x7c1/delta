import { buildThreadTree, type Thread, type ThreadNode } from '@delta/model';
import { Badge, cn } from '@delta/ui-kit';
import { useNavStore } from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';

export interface ThreadTreeProps {
  threads: Thread[];
}

/**
 * The thread navigator tree (named nodes only, no messages). Clicking a node
 * sets it active; activation clears the thread's unread badge (handled centrally
 * by the workspace, so every activation path clears it). Siblings keep creation
 * order.
 *
 * The main thread is intentionally not listed: it is always present, so a row
 * for it is redundant. The main thread is reached instead by clicking the
 * session card header (see {@link NavigatorPane}). Only main's sub-threads are
 * rendered, lifted to depth 0 so they sit directly under the session without a
 * redundant indent level.
 */
export function ThreadTree({ threads }: ThreadTreeProps) {
  const roots = buildThreadTree(threads);
  const subThreads = roots.flatMap((root) => root.children);
  return (
    <ul className="py-1">
      {subThreads.map((node) => (
        <ThreadTreeNode key={node.thread.id} node={node} depth={0} />
      ))}
    </ul>
  );
}

function ThreadTreeNode({ node, depth }: { node: ThreadNode; depth: number }) {
  const activeThreadId = useNavStore((state) => state.activeThreadId);
  const setActiveThread = useNavStore((state) => state.setActiveThread);
  const unread = useLiveStore((state) => state.unread[node.thread.id] ?? 0);

  const isActive = activeThreadId === node.thread.id;

  return (
    <li>
      <button
        type="button"
        onClick={() => setActiveThread(node.thread.id)}
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
            />
          ))}
        </ul>
      )}
    </li>
  );
}
