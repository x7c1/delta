import type { SessionListItem, Thread } from '@delta/model';
import { Menu, StatusDot, cn } from '@delta/ui-kit';
import { formatLocalDateTime } from '../../utils/formatLocalDateTime';
import { ThreadTree } from './ThreadTree';

export interface SessionNodeProps {
  item: SessionListItem;
  isFocused: boolean;
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
 * One top-level navigator node: a session. The focus button holds a two-line
 * block — line 1 is the open/closed indicator plus the session label, line 2 is
 * the right-aligned last-activity timestamp (omitted when there is none, leaving
 * a single line). Clicking either line focuses the session. The kebab actions
 * menu sits in a fixed-width slot at the row's right end, vertically centered
 * against the block, and is enabled only when the session is open. When the
 * session is focused, its {@link ThreadTree} is rendered nested beneath it.
 */
export function SessionNode({
  item,
  isFocused,
  threads,
  onFocus,
  onClose,
}: SessionNodeProps) {
  const lastActivity = formatLocalDateTime(item.last_activity_at);
  const label = sessionLabel(item);
  return (
    <li>
      <div
        className={cn(
          'flex items-center gap-2 px-2 py-1.5',
          isFocused && 'bg-indigo-50',
        )}
      >
        <button
          type="button"
          onClick={onFocus}
          className="flex min-w-0 flex-1 flex-col gap-0.5 text-left text-sm"
          aria-current={isFocused ? 'true' : undefined}
          data-testid="session-node"
        >
          <span className="flex min-w-0 items-center gap-2">
            <StatusDot
              tone={item.open ? 'green' : 'slate'}
              title={item.open ? 'Open' : 'Closed'}
            />
            <span
              className={cn(
                'truncate',
                isFocused && 'font-medium text-indigo-800',
              )}
            >
              {label}
            </span>
          </span>
          {/* Right-aligned so the timestamp column stays aligned across rows. */}
          {lastActivity && (
            <span className="text-right text-xs tabular-nums text-slate-400">
              {lastActivity}
            </span>
          )}
        </button>
        {/* Fixed-width slot, vertically centered against the two-line block. */}
        <Menu
          label={`Session actions for ${label}`}
          disabled={!item.open}
          items={
            item.open
              ? [{ label: 'Close', onSelect: onClose, tone: 'danger' }]
              : []
          }
        />
      </div>

      {isFocused && threads && threads.length > 0 && (
        <div className="pl-2">
          <ThreadTree threads={threads} />
        </div>
      )}
    </li>
  );
}
