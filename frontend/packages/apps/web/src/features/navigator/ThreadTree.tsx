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
 */
export function ThreadTree({ threads }: ThreadTreeProps) {
  const roots = buildThreadTree(threads);
  return (
    <ul className="py-1">
      {roots.map((node) => (
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
          'flex w-full items-center justify-between gap-2 py-1 pr-2 text-left text-sm hover:bg-slate-100',
          isActive && 'bg-indigo-50 font-medium text-indigo-800',
        )}
        aria-current={isActive ? 'true' : undefined}
      >
        <span className="truncate">
          {depth > 0 && <span className="text-slate-400">⤷ </span>}
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
