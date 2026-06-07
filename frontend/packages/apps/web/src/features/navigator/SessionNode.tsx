import type { SessionListItem, Thread } from '@delta/model';
import { Badge, StatusDot, cn } from '@delta/ui-kit';
import { ThreadTree } from './ThreadTree';

export interface SessionNodeProps {
  item: SessionListItem;
  isFocused: boolean;
  /** True while a Send-driven resume of this session is awaiting confirmation. */
  resuming: boolean;
  /**
   * The session's thread tree, supplied only for the focused session (whose
   * threads are loaded). Expanding shows it; non-focused sessions show no tree.
   */
  threads: Thread[] | undefined;
  onFocus: () => void;
  onClose: () => void;
}

/** A short, readable stand-in for a session that has no title yet. */
function sessionLabel(item: SessionListItem): string {
  if (item.session.title) {
    return item.session.title;
  }
  // Show a short slice of the id so distinct sessions remain distinguishable.
  return `session ${item.session.id.slice(0, 8)}`;
}

/**
 * One top-level navigator node: a session. Shows an open/closed indicator and
 * the session label, focuses on click, and exposes a Close affordance. When the
 * session is focused, its {@link ThreadTree} is rendered nested beneath it.
 */
export function SessionNode({
  item,
  isFocused,
  resuming,
  threads,
  onFocus,
  onClose,
}: SessionNodeProps) {
  return (
    <li>
      <div
        className={cn(
          'flex items-center justify-between gap-2 px-2 py-1.5',
          isFocused && 'bg-indigo-50',
        )}
      >
        <button
          type="button"
          onClick={onFocus}
          className="flex min-w-0 flex-1 items-center gap-2 text-left text-sm"
          aria-current={isFocused ? 'true' : undefined}
          data-testid="session-node"
        >
          <StatusDot
            tone={item.open ? 'green' : 'slate'}
            label={item.open ? 'open' : 'closed'}
          />
          <span
            className={cn('truncate', isFocused && 'font-medium text-indigo-800')}
          >
            {sessionLabel(item)}
          </span>
          {resuming && <Badge tone="warning">resuming…</Badge>}
          {!item.open && !resuming && <Badge tone="neutral">view</Badge>}
        </button>
        {item.open && (
          <button
            type="button"
            onClick={onClose}
            aria-label={`Close ${sessionLabel(item)}`}
            className="shrink-0 rounded px-1 text-xs text-slate-400 hover:bg-slate-200 hover:text-slate-700"
          >
            Close
          </button>
        )}
      </div>

      {isFocused && threads && threads.length > 0 && (
        <div className="pl-2">
          <ThreadTree threads={threads} />
        </div>
      )}
    </li>
  );
}
