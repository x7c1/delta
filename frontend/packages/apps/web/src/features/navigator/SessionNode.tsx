import type { SessionListItem, Thread } from '@delta/model';
import { Menu, StatusDot, cn } from '@delta/ui-kit';
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
 * Format a stored UTC ISO-8601 timestamp as absolute local time
 * (`YYYY-MM-DD HH:mm`) in the browser's timezone. Returns `null` when the input
 * is absent or unparseable so the caller can render nothing.
 */
function formatLastActivity(iso: string | null): string | null {
  if (!iso) {
    return null;
  }
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    return null;
  }
  const pad = (n: number) => String(n).padStart(2, '0');
  const y = date.getFullYear();
  const mo = pad(date.getMonth() + 1);
  const d = pad(date.getDate());
  const h = pad(date.getHours());
  const mi = pad(date.getMinutes());
  return `${y}-${mo}-${d} ${h}:${mi}`;
}

/**
 * One top-level navigator node: a session. Shows an open/closed indicator and
 * the session label, focuses on click, and exposes a kebab actions menu. The
 * kebab slot occupies a fixed width on every row (enabled only when the session
 * is open) so the last-activity timestamp aligns in a stable column regardless
 * of which sessions are open. When the session is focused, its
 * {@link ThreadTree} is rendered nested beneath it.
 */
export function SessionNode({
  item,
  isFocused,
  threads,
  onFocus,
  onClose,
}: SessionNodeProps) {
  const lastActivity = formatLastActivity(item.last_activity_at);
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
          className="flex min-w-0 flex-1 items-center gap-2 text-left text-sm"
          aria-current={isFocused ? 'true' : undefined}
          data-testid="session-node"
        >
          <StatusDot
            tone={item.open ? 'green' : 'slate'}
            title={item.open ? 'Open' : 'Closed'}
          />
          <span
            className={cn('truncate', isFocused && 'font-medium text-indigo-800')}
          >
            {label}
          </span>
        </button>
        <span className="shrink-0 text-xs tabular-nums text-slate-400">
          {lastActivity}
        </span>
        {/* Fixed-width slot keeps the timestamp column aligned across rows. */}
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
