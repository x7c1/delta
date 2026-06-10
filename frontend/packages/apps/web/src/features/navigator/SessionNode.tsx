import { useState, type CSSProperties, type Ref } from 'react';
import type { SessionListItem, ThreadId } from '@delta/model';
import { useSessionThreadsQuery } from '@delta/api-client';
import { Badge, Menu, StatusDot, cn } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import { formatLocalDateTime } from '../../utils/formatLocalDateTime';
import { pathTail } from '../../utils/pathTail';
import { ThreadTree } from './ThreadTree';

export interface SessionNodeProps {
  item: SessionListItem;
  isFocused: boolean;
  /**
   * Whether this session has a pending permission request (a tool blocked on a
   * prompt in its terminal). Surfaced as a badge so a request on a non-focused
   * session is discoverable from the list; the actionable notice lives in the
   * session's conversation pane.
   */
  needsPermission?: boolean;
  onFocus: () => void;
  onClose: () => void;
  /**
   * Ref to the row's `<li>`, used by the virtualizer's `measureElement` to read
   * the card's real height (it varies: the focused card expands its thread
   * tree). Omitted when the list is rendered without windowing.
   */
  rowRef?: Ref<HTMLLIElement>;
  /**
   * Virtual-row index, mirrored onto `data-index` so the virtualizer can map a
   * measured element back to its row. Paired with {@link rowRef}.
   */
  index?: number;
  /**
   * Inline style for the row's `<li>`. The virtualizer absolutely positions each
   * row via `transform: translateY(...)`, which it supplies here.
   */
  style?: CSSProperties;
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
 * indicator plus the working-directory tail, which is the primary identifier
 * (left-truncated, full path on hover; falls back to the session label when
 * there is no directory); line 2 shows the session id and the last-activity
 * time, right-aligned) plus the kebab actions menu in a fixed-width slot at the
 * right end, enabled only when the session is open. The focused card is lifted
 * with an indigo border, tint, and ring.
 *
 * Every session that has branched into sub-threads shows its {@link ThreadTree}
 * expanded by default — focused or not — so the whole visible list reads as a
 * navigable session → thread tree. Each mounted row fetches its own thread tree;
 * because the list is windowed, that fetch is bounded to the visible window, and
 * it shares the focused session's query key so the two are deduped into one
 * request per session. Clicking a sub-thread in a non-focused session focuses
 * that session and activates the thread, switching the center pane to it.
 */
export function SessionNode({
  item,
  isFocused,
  needsPermission = false,
  onFocus,
  onClose,
  rowRef,
  index,
  style,
}: SessionNodeProps) {
  const client = useApiClient();
  const setFocusedSession = useNavStore((state) => state.setFocusedSession);
  const setActiveThread = useNavStore((state) => state.setActiveThread);
  // The kebab menu's dropdown opens below the trigger, but each windowed row is
  // an absolutely-positioned `transform` stacking context, so the dropdown is
  // painted under the next row's card. While the menu is open, lift this row
  // above its siblings so the dropdown is visible (see the `zIndex` on `<li>`).
  const [menuOpen, setMenuOpen] = useState(false);
  // Fetch this row's thread tree. Mounted only for sessions in the windowed
  // viewport (+overscan), so the number of in-flight thread queries is bounded
  // by the visible window, not the full session list. Shares the focused
  // session's query key, so React Query serves both from one request.
  const threadsQuery = useSessionThreadsQuery(client, item.session.id);
  const threads = threadsQuery.data?.threads;

  const lastActivity = formatLocalDateTime(item.last_activity_at);
  const cwdTail = pathTail(item.session.cwd);
  const label = sessionLabel(item);
  // Show the sub-thread list only once the session has branched. The main
  // thread itself is never listed (it is reached by clicking this card's
  // header — see NavigatorPane); a session with no sub-threads shows no tree at
  // all. A sub-thread is any thread with a parent.
  const hasSubThreads =
    threads?.some((t) => t.parent_thread_id !== null) ?? false;

  // Selecting a sub-thread switches the center pane to it. Focus the owning
  // session first (a focus switch clears the active thread), then set the
  // active thread — order matters so the activation is not cleared. Re-selecting
  // within the already-focused session is a no-op focus, leaving the active
  // thread set as expected.
  const selectThread = (threadId: ThreadId) => {
    setFocusedSession(item.session.id);
    setActiveThread(threadId);
  };

  return (
    // Horizontal inset (px-2) and the inter-card gap (pb-1.5) live *inside* the
    // measured box: the virtualizer measures `getBoundingClientRect().height`,
    // which excludes margins, so spacing expressed as margins would not be
    // accounted for and rows would overlap. Padding is included, so the gap is
    // preserved under windowing.
    <li
      ref={rowRef}
      data-index={index}
      // Spread the virtualizer's positioning style, then lift this row above its
      // siblings while its menu is open so the dropdown is not covered by the
      // next row's card (sibling rows are z-auto, painting in DOM order).
      style={menuOpen ? { ...style, zIndex: 20 } : style}
      // pb-1.5 is the inter-card gap (baked into each measured row). The first
      // card also needs that gap above it: the windowed rows are absolutely
      // positioned, so a `pt` on the list container is ignored — give the top
      // row a matching pt-1.5 so it is not flush against the panel top.
      className={cn('px-2 pb-1.5', index === 0 && 'pt-1.5')}
    >
      <div
        className={cn(
          'rounded-lg border bg-white shadow-sm transition-colors',
          isFocused
            ? 'border-indigo-300 bg-indigo-50/70 ring-1 ring-indigo-200'
            : 'border-slate-300 hover:border-slate-400',
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
                  'min-w-0 truncate text-left [direction:rtl]',
                  isFocused && 'font-medium text-indigo-800',
                )}
                title={item.session.cwd}
              >
                {cwdTail || label}
              </span>
              {needsPermission && (
                <span className="shrink-0" data-testid="session-permission-badge">
                  <Badge tone="warning">permission</Badge>
                </span>
              )}
            </span>
            {/* Secondary line: session id + last-activity time, right-aligned. The id is
                a long UUID, so only its first 8 chars are shown, with the full value
                in its title. */}
            <span className="flex items-baseline justify-end gap-2 text-xs text-slate-400">
              <span className="font-mono" title={item.session.id}>
                {item.session.id.slice(0, 8)}
              </span>
              {lastActivity && (
                <span className="shrink-0 tabular-nums">{lastActivity}</span>
              )}
            </span>
          </button>
          {/* Fixed-width slot, vertically centered against the two-line block. */}
          <Menu
            label={`Session actions for ${label}`}
            onOpenChange={setMenuOpen}
            disabled={!item.open}
            items={
              item.open
                ? [{ label: 'Close', onSelect: onClose, tone: 'danger' }]
                : []
            }
          />
        </div>

        {hasSubThreads && threads && (
          <div
            className={cn(
              'border-t px-2 py-1.5',
              isFocused ? 'border-indigo-200' : 'border-slate-200',
            )}
          >
            <ThreadTree threads={threads} onSelectThread={selectThread} />
          </div>
        )}
      </div>
    </li>
  );
}
