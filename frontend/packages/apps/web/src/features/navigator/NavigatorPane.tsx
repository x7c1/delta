import { useEffect, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { SessionListItem } from '@delta/model';
import { Button, Panel, Spinner, StatusDot, type DotTone } from '@delta/ui-kit';
import {
  useCloseSessionMutation,
  type ConnectionStatus,
} from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { useLiveStore } from '../../store/liveStore';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { SessionNode } from './SessionNode';

/**
 * Estimated height of a collapsed session card, in pixels. Only a seed for the
 * virtualizer's spacer math: each rendered row reports its true height back via
 * `measureElement` (ResizeObserver-backed), so the focused card expanding its
 * thread tree is measured rather than assumed. The session list's scrollbar is
 * hidden (`scrollbar-none`), so the brief estimate-vs-actual jitter that the
 * spacer approach can cause is not visible.
 */
const ESTIMATED_SESSION_NODE_HEIGHT = 64;

/**
 * Extra rows rendered above and below the visible window. A small buffer keeps
 * scrolling smooth (rows exist before they scroll into view) while still
 * recycling off-screen DOM so the node count stays bounded regardless of how
 * many sessions have loaded.
 */
const SESSION_OVERSCAN = 8;

export interface NavigatorPaneProps {
  /** The loaded sessions so far, ordered most-recently-active first. */
  sessions: SessionListItem[];
  /** Whether more session pages remain to be fetched. */
  hasMoreSessions: boolean;
  /** Whether the next session page is currently in flight. */
  isLoadingMoreSessions: boolean;
  /** Request the next session page (cursor-paginated). */
  onLoadMoreSessions: () => void;
}

const CONNECTION_TONE: Record<ConnectionStatus, DotTone> = {
  connecting: 'amber',
  open: 'green',
  closed: 'red',
};

const CONNECTION_TITLE: Record<ConnectionStatus, string> = {
  connecting: 'Server connection: connecting…',
  open: 'Server connection: connected',
  closed: 'Server connection: disconnected',
};

/**
 * The left pane: a session → thread nested tree, plus a "New" affordance, the
 * permission notice, a running indicator, and the live connection status. Each
 * session's open/closed state is shown by its status dot, so no separate count
 * is rendered. Top-level nodes are sessions; every session that has branched
 * shows its thread tree expanded (each row fetches its own — see SessionNode).
 */
export function NavigatorPane({
  sessions,
  hasMoreSessions,
  isLoadingMoreSessions,
  onLoadMoreSessions,
}: NavigatorPaneProps) {
  const client = useApiClient();
  const closeSession = useCloseSessionMutation(client);

  // The Panel body is the scroll container; the virtualizer reads its scroll
  // position and viewport height to decide which rows to render.
  const scrollBodyRef = useRef<HTMLDivElement>(null);
  // The default `measureElement` is ResizeObserver-backed: each rendered row
  // reports its real height, so the focused card growing/shrinking its thread
  // tree re-measures instead of corrupting the positions of the rows below it.
  // Rows opt into measurement by attaching `virtualizer.measureElement` as their
  // ref (see below); `estimateSize` is only the spacer seed before measurement.
  const virtualizer = useVirtualizer({
    count: sessions.length,
    getScrollElement: () => scrollBodyRef.current,
    estimateSize: () => ESTIMATED_SESSION_NODE_HEIGHT,
    overscan: SESSION_OVERSCAN,
  });

  const virtualItems = virtualizer.getVirtualItems();

  // Scroll-triggered page loading, driven off the virtual range rather than a
  // DOM sentinel: once the window reaches the last loaded session, fetch the
  // next page. This replaces the earlier IntersectionObserver trigger, which
  // could not see a sentinel that windowing keeps unmounted until scrolled to.
  useEffect(() => {
    const lastItem = virtualItems[virtualItems.length - 1];
    if (!lastItem) {
      return;
    }
    if (
      lastItem.index >= sessions.length - 1 &&
      hasMoreSessions &&
      !isLoadingMoreSessions
    ) {
      onLoadMoreSessions();
    }
  }, [
    virtualItems,
    sessions.length,
    hasMoreSessions,
    isLoadingMoreSessions,
    onLoadMoreSessions,
  ]);

  const connection = useLiveStore((state) => state.connection);
  // Per-session permission requests. The notice itself now lives in the focused
  // session's conversation pane (above the composer); here it only drives a
  // badge on each session's row so a request on a non-focused session is still
  // discoverable.
  const permissions = useLiveStore((state) => state.permission);
  const hasInProgress = useLiveStore((state) =>
    state.pending.some((item) => item.status === 'in_progress'),
  );

  const focusedSessionId = useNavStore((state) => state.focusedSessionId);
  const setFocusedSession = useNavStore((state) => state.setFocusedSession);
  const setActiveThread = useNavStore((state) => state.setActiveThread);

  return (
    <Panel
      className="border-r border-slate-200"
      bodyRef={scrollBodyRef}
      // The session list is a side panel; hide its scrollbar entirely (no bar,
      // no reserved column) so it never shows a stray blank strip. It still
      // scrolls via wheel/trackpad. The transcript pane keeps its hover-reveal
      // scrollbar (Panel's default).
      bodyClassName="scrollbar-none"
      header={
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <StatusDot
              tone={CONNECTION_TONE[connection]}
              title={CONNECTION_TITLE[connection]}
            />
            <span className="text-sm font-semibold text-slate-700">
              Sessions
            </span>
          </div>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => setFocusedSession(NEW_SESSION_FOCUS)}
          >
            New
          </Button>
        </div>
      }
      footer={hasInProgress ? <Spinner label="running" /> : undefined}
    >
      {focusedSessionId === NEW_SESSION_FOCUS && (
        <div
          className="mx-2 mb-1.5 mt-1.5 rounded-lg border border-indigo-300 bg-indigo-50/70 px-2 py-2 text-xs text-indigo-700 shadow-sm ring-1 ring-indigo-200"
          data-testid="new-session-node"
        >
          New session — send the first message to start it.
        </div>
      )}

      {/*
        Windowed session list. The `<ul>` is a spacer sized to the full virtual
        height (getTotalSize); only the rows in the current window (+overscan)
        are mounted, each absolutely positioned by the virtualizer. This bounds
        the live DOM-node count regardless of how many sessions have loaded.
      */}
      <ul
        data-testid="sessions-list"
        className="relative w-full"
        style={{ height: virtualizer.getTotalSize() }}
      >
        {virtualItems.map((virtualRow) => {
          const item = sessions[virtualRow.index];
          return (
            <SessionNode
              key={item.session.id}
              rowRef={virtualizer.measureElement}
              index={virtualRow.index}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                right: 0,
                transform: `translateY(${virtualRow.start}px)`,
              }}
              item={item}
              isFocused={focusedSessionId === item.session.id}
              needsPermission={Boolean(permissions[item.session.id])}
              onFocus={() => {
                setFocusedSession(item.session.id);
                // The main thread is not listed in the tree, so clicking the
                // session card is how you return to it. Always select main —
                // this also covers re-clicking the already-focused session
                // while viewing one of its sub-threads.
                setActiveThread(item.main_thread_id);
              }}
              onClose={() => closeSession.mutate(item.session.id)}
            />
          );
        })}
      </ul>

      {/*
        While the next page is in flight, surface a small loading row below the
        windowed list. The fetch itself is triggered from the virtualizer's
        range (see the effect above), not from this element.
      */}
      {hasMoreSessions && isLoadingMoreSessions && (
        <div
          data-testid="sessions-load-more"
          className="flex justify-center px-3 pb-3 pt-1 text-xs text-slate-400"
        >
          <Spinner label="loading more" />
        </div>
      )}
    </Panel>
  );
}
