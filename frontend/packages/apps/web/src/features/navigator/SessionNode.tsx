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
 * One top-level navigator node: a session, rendered as a card. The card holds a
 * header row — the focus button (a two-line block: line 1 is the open/closed
 * indicator plus the session label, line 2 is the right-aligned last-activity
 * timestamp, omitted when there is none) plus the kebab actions menu in a
 * fixed-width slot at the right end, enabled only when the session is open. The
 * focused card is lifted with an indigo border, tint, and ring. When the focused
 * session has branched into sub-threads, its {@link ThreadTree} is rendered in a
 * divided section inside the same card.
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
  // The standalone "main" node is redundant until the session has branched;
  // once it has sub-threads, "main" must appear so the user can navigate back
  // to the main thread. A sub-thread is any thread with a parent.
  const hasSubThreads =
    threads?.some((t) => t.parent_thread_id !== null) ?? false;
  return (
    <li className="mx-2 mb-1.5">
      <div
        className={cn(
          'rounded-lg border bg-white shadow-sm transition-colors',
          isFocused
            ? 'border-indigo-300 bg-indigo-50/70 ring-1 ring-indigo-200'
            : 'border-slate-200 hover:border-slate-300',
        )}
      >
        <div className="flex items-center gap-2 px-2 py-2">
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

        {isFocused && hasSubThreads && threads && (
          <div className="border-t border-indigo-200 px-2 py-1.5">
            <ThreadTree threads={threads} />
          </div>
        )}
      </div>
    </li>
  );
}
