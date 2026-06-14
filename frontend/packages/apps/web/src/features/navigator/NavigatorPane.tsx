import { useEffect, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { SessionListItem } from '@delta/wire-gen';
import { Button, cn, Panel, Spinner, StatusDot, type DotTone } from '@delta/ui-kit';
import {
  useCloseSessionMutation,
  type ConnectionStatus,
} from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { noticeOf, useLiveStore } from '../../store/liveStore';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useComposerStore } from '../../store/composerStore';
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
 * Plus glyph marking the "New session" header action so it reads as an
 * affordance to create something. Decorative — always `aria-hidden`, so the
 * button's accessible name stays its "New session" label. This file is the
 * only user.
 */
function PlusIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  );
}

/**
 * Gear glyph marking the footer "Settings" entry so it reads as a button.
 * Decorative — always `aria-hidden`, so the button's accessible name stays its
 * "Settings" label. This file is the only user.
 */
function SettingsIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}

/**
 * The left pane: a session → thread nested tree, plus a "New" affordance and
 * the live connection status. Each session's open/closed state is shown by its
 * status dot, so no separate count is rendered. Per-session state — a pending
 * permission request and an in-flight turn (running) — is surfaced on the
 * owning session's row rather than globally, so it is clear which session it
 * refers to. Top-level nodes are sessions; every session that has branched
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
  //
  // `getItemKey` keys the measurement cache by session id, not by index. The
  // list is recency-ordered, so sending a message bumps that session to the top
  // and reindexes the rest. With the default index key, each row would inherit
  // the cached height of whichever session previously sat at its index — a tall
  // threaded card's height landing on a short collapsed one, leaving gaps (and
  // vice versa, overlaps). Keying by id makes a measured height travel with its
  // session across reorders, so the spacers stay correct.
  const virtualizer = useVirtualizer({
    count: sessions.length,
    getScrollElement: () => scrollBodyRef.current,
    estimateSize: () => ESTIMATED_SESSION_NODE_HEIGHT,
    overscan: SESSION_OVERSCAN,
    getItemKey: (index) => sessions[index].session.id,
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
  // Per-session notices. A pending permission request drives a badge on its
  // session's row (the notice card itself lives in the focused session's
  // conversation pane), so a request on a non-focused session is still
  // discoverable. A dismissed notice keeps its badge: the request is still
  // genuinely awaiting an answer.
  const notices = useLiveStore((state) => state.notices);
  // Per-session in-flight-turn set. Each session's row shows its own running
  // indicator (see SessionNode), so it is clear *which* session is processing —
  // a single global footer spinner could not tell them apart.
  const activeTurns = useLiveStore((state) => state.activeTurns);
  // Per-session unread set. A session whose turn finished while the user was
  // viewing a different one carries a static unread dot on its row (see
  // SessionNode), distinct from the running spinner, cleared once focused.
  const unreadSessions = useLiveStore((state) => state.unreadSessions);

  const focusedSessionId = useNavStore((state) => state.focusedSessionId);
  const settingsOpen = useNavStore((state) => state.settingsOpen);
  const openSettings = useNavStore((state) => state.openSettings);
  const setFocusedSession = useNavStore((state) => state.setFocusedSession);
  const startNewSession = useNavStore((state) => state.startNewSession);
  const setActiveThread = useNavStore((state) => state.setActiveThread);
  const setNewSessionWorkdir = useComposerStore(
    (state) => state.setNewSessionWorkdir,
  );
  const openWorkdirDialog = useComposerStore(
    (state) => state.openWorkdirDialog,
  );

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
        // The header holds the primary action: a full-width "New session" CTA,
        // styled as a filled (secondary) button so it clearly reads as a button
        // at rest. It always (re)starts the new-session flow even when already in
        // that state — changing focus is not enough, the picker's open state
        // lives in the store so it can open without a focus transition; reset any
        // prior selection for a clean directory choice.
        <Button
          variant="secondary"
          size="sm"
          className="w-full justify-start"
          onClick={() => {
            startNewSession();
            setNewSessionWorkdir(null);
            openWorkdirDialog();
          }}
        >
          <PlusIcon className="h-3.5 w-3.5" />
          New session
        </Button>
      }
      footer={
        // A quiet utility bar, distinct from the primary action up top: the live
        // connection status (dot + "Delta" running label) on the left, and an
        // icon-only Settings entry on the right (claude.ai-style, opens the
        // settings dialog overlaid on the workspace).
        <div className="flex items-center justify-between gap-2">
          {/*
            data-connection exposes the live connection state structurally so the
            e2e suites can wait on disconnect/reconnect transitions without
            depending on the dot's color classes or title wording.
          */}
          <span className="inline-flex items-center gap-1.5">
            <span
              className="inline-flex px-1"
              data-testid="connection-indicator"
              data-connection={connection}
            >
              <StatusDot
                tone={CONNECTION_TONE[connection]}
                title={CONNECTION_TITLE[connection]}
              />
            </span>
            <span className="text-xs text-slate-500">Delta</span>
          </span>
          {/*
            Icon-only Settings button: aria-label carries the accessible name
            since the gear glyph has no text, while data-testid and aria-pressed
            keep the existing wiring and the e2e/unit hooks stable.
          */}
          <Button
            variant="ghost"
            size="sm"
            className={cn('px-1.5', settingsOpen && 'bg-slate-100 text-slate-900')}
            data-testid="settings-entry"
            aria-label="Settings"
            aria-pressed={settingsOpen}
            onClick={openSettings}
          >
            <SettingsIcon className="h-4 w-4" />
          </Button>
        </div>
      }
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
              needsPermission={
                noticeOf(notices, item.session.id, 'permission') !== null
              }
              running={!!activeTurns[item.session.id]}
              unread={
                !!unreadSessions[item.session.id] &&
                focusedSessionId !== item.session.id
              }
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
