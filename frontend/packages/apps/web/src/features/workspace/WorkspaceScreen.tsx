import { useEffect, useMemo } from 'react';
import {
  useSessionsQuery,
  useSessionThreadsQuery,
} from '@delta/api-client';
import type { SessionListItem } from '@delta/wire-gen';
import { Button, ErrorBoundary } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useSessionEvents } from '../../data/useSessionEvents';
import {
  NEW_SESSION_FOCUS,
  useNavStore,
  type FocusedSession,
} from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';
import { useMediaQuery } from '../../hooks/useMediaQuery';
import { NavigatorPane } from '../navigator/NavigatorPane';
import { TranscriptPane } from '../transcript/TranscriptPane';
import { TerminalPane } from '../terminal/TerminalPane';
import { TerminalFallback } from '../terminal/TerminalFallback';
import { TerminalResizeHandle } from '../terminal/TerminalResizeHandle';

/**
 * Pick the session to focus on cold load from the session list: prefer the
 * most-recently-active open session, else the most-recently-active session,
 * else the new-session sentinel when the list is empty. The list is ordered by
 * most recent activity first, so "most recent" is the first element.
 */
function pickInitialFocus(sessions: SessionListItem[]): FocusedSession {
  if (sessions.length === 0) {
    return NEW_SESSION_FOCUS;
  }
  const open = sessions.filter((item) => item.open);
  const pool = open.length > 0 ? open : sessions;
  return pool[0].session.id;
}

/**
 * The top-level session-centric workspace: navigator (session → thread tree) |
 * transcript | terminal. On load it lists every session and focuses one; the
 * composer drives the conversation (new session on cold start, resume on a
 * closed session), so the terminal is no longer required to begin. A focused
 * closed session renders read-only.
 */
export function WorkspaceScreen() {
  const client = useApiClient();
  useSessionEvents();

  const sessionsQuery = useSessionsQuery(client);
  // The session list is cursor-paginated; flatten the loaded pages back into one
  // ordered list. Pages arrive most-recently-active first, so concatenation
  // preserves the global order.
  const sessions = useMemo(
    () => sessionsQuery.data?.pages.flatMap((page) => page.sessions) ?? [],
    [sessionsQuery.data],
  );

  const focusedSessionId = useNavStore((state) => state.focusedSessionId);
  const activeThreadId = useNavStore((state) => state.activeThreadId);
  const setFocusedSession = useNavStore((state) => state.setFocusedSession);
  const setActiveThread = useNavStore((state) => state.setActiveThread);
  const terminalOpen = useNavStore((state) => state.terminalOpen);
  const toggleTerminal = useNavStore((state) => state.toggleTerminal);
  const terminalWidth = useNavStore((state) => state.terminalWidth);
  const clearUnread = useLiveStore((state) => state.clearUnread);
  const clearSessionUnread = useLiveStore((state) => state.clearSessionUnread);
  const spawns = useLiveStore((state) => state.spawns);
  const clearSpawn = useLiveStore((state) => state.clearSpawn);

  const isLargeScreen = useMediaQuery('(min-width: 1024px)');

  const isNewSessionFocus = focusedSessionId === NEW_SESSION_FOCUS;
  const focusedItem =
    focusedSessionId === null || isNewSessionFocus
      ? null
      : sessions.find((item) => item.session.id === focusedSessionId) ?? null;

  // The focused session's id for the thread query (null for new/none/unknown).
  const focusedRealSessionId = focusedItem?.session.id ?? null;
  const threadsQuery = useSessionThreadsQuery(client, focusedRealSessionId);
  const threads = useMemo(
    () => threadsQuery.data?.threads ?? [],
    [threadsQuery.data],
  );

  // Hand a tracked spawn over to normal navigation once it registers. The
  // `POST /api/sends` response named the spawned session's real id, so when
  // that id appears in the refetched session list the spawn is over: focus it
  // directly — but only when the user is still waiting on the new-session
  // screen (they may have navigated elsewhere meanwhile) — and stop tracking
  // it (the pending chip continues from the session's open-send list).
  useEffect(() => {
    if (!sessionsQuery.isSuccess) {
      return;
    }
    const listed = spawns.filter(
      (spawn) =>
        spawn.status === 'spawning' &&
        sessions.some((item) => item.session.id === spawn.sessionId),
    );
    if (listed.length === 0) {
      return;
    }
    if (isNewSessionFocus) {
      // Several spawns can only pile up via quick Retry cycles; the newest is
      // the one the user is waiting on.
      setFocusedSession(listed[listed.length - 1].sessionId);
    }
    for (const spawn of listed) {
      clearSpawn(spawn.sessionId);
    }
  }, [
    sessionsQuery.isSuccess,
    sessions,
    spawns,
    isNewSessionFocus,
    setFocusedSession,
    clearSpawn,
  ]);

  // Resolve focus once the session list loads.
  useEffect(() => {
    if (!sessionsQuery.isSuccess) {
      return;
    }
    if (isNewSessionFocus) {
      // The new-session screen keeps focus until its spawn registers (handled
      // above) or the user navigates away; the cold-start reconciliation below
      // must not stomp it.
      return;
    }
    const stillExists =
      focusedSessionId !== null &&
      sessions.some((item) => item.session.id === focusedSessionId);
    if (!stillExists) {
      // A persisted focus (`focusedSessionId !== null`) that is absent from the
      // loaded pages is ambiguous while more pages remain: the session may live
      // on a not-yet-loaded page rather than being truly gone. Defer
      // reconciliation in that case so a reload does not spuriously refocus the
      // top of page 1 while later pages are still streaming in; once all pages
      // are loaded (`!hasNextPage`), a missing id genuinely no longer exists.
      //
      // Cold start (`focusedSessionId === null`) is never ambiguous: the
      // most-recently-active focus candidate is page-1 top by construction, so
      // pick it immediately rather than waiting for the whole list to load.
      if (focusedSessionId !== null && sessionsQuery.hasNextPage) {
        return;
      }
      setFocusedSession(pickInitialFocus(sessions));
    }
  }, [
    sessionsQuery.isSuccess,
    sessionsQuery.hasNextPage,
    sessions,
    focusedSessionId,
    isNewSessionFocus,
    setFocusedSession,
  ]);

  // Reconcile the active thread against the focused session's threads. Default
  // to the session's main when none is set; fall back to main when a persisted
  // active thread does not belong to this session. Skip while the threads query
  // is in flight so a freshly-branched child (not yet refetched) is not reverted.
  useEffect(() => {
    if (!focusedItem || threadsQuery.isFetching) {
      return;
    }
    const main = focusedItem.main_thread_id;
    if (activeThreadId === null) {
      setActiveThread(main);
      return;
    }
    if (
      threads.length > 0 &&
      !threads.some((thread) => thread.id === activeThreadId)
    ) {
      setActiveThread(main);
    }
  }, [
    focusedItem,
    threads,
    threadsQuery.isFetching,
    activeThreadId,
    setActiveThread,
  ]);

  // Clear the unread badge whenever a thread becomes active.
  useEffect(() => {
    if (activeThreadId !== null) {
      clearUnread(activeThreadId);
    }
  }, [activeThreadId, clearUnread]);

  // Clear the per-session unread dot whenever a real session becomes focused —
  // the user is now looking at it, so whatever finished in the background is no
  // longer unseen. Skip the new-session sentinel (no session to clear) and the
  // null pre-load state.
  useEffect(() => {
    if (focusedSessionId !== null && focusedSessionId !== NEW_SESSION_FOCUS) {
      clearSessionUnread(focusedSessionId);
    }
  }, [focusedSessionId, clearSessionUnread]);

  const activeThread =
    threads.find((thread) => thread.id === activeThreadId) ?? null;

  if (sessionsQuery.isPending) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-slate-400">
        Loading sessions…
      </div>
    );
  }

  if (sessionsQuery.isError) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-sm text-slate-500">
        <p>Could not load sessions.</p>
        <p className="text-xs text-slate-400">
          Make sure the Delta server is running, then reload.
        </p>
        <Button size="sm" variant="secondary" onClick={() => sessionsQuery.refetch()}>
          Retry
        </Button>
      </div>
    );
  }

  const focusedOpen = focusedItem?.open ?? false;

  // Fence the embedded terminal behind an error boundary: its attach runs in an
  // effect that can throw (e.g. an xterm addon failing to load), and without a
  // boundary that exception would unmount the whole app. Isolating it here keeps
  // the conversation usable and shows a recoverable fallback in the pane. The
  // focused id is the reset key, so switching sessions retries the attach.
  const terminal = (
    <ErrorBoundary
      label="terminal"
      resetKey={focusedRealSessionId}
      fallback={() => <TerminalFallback onClose={toggleTerminal} />}
    >
      <TerminalPane sessionId={focusedRealSessionId} attachable={focusedOpen} />
    </ErrorBoundary>
  );

  return (
    <div className="relative flex h-full overflow-hidden">
      {/* Left: navigator (session → thread tree) */}
      <div className="w-72 shrink-0">
        <NavigatorPane
          sessions={sessions}
          hasMoreSessions={sessionsQuery.hasNextPage}
          isLoadingMoreSessions={sessionsQuery.isFetchingNextPage}
          onLoadMoreSessions={sessionsQuery.fetchNextPage}
        />
      </div>

      {/* Center: transcript, or the cold-start / new-session composer state */}
      <div className="min-w-0 flex-1">
        {isNewSessionFocus ? (
          <TranscriptPane
            threads={[]}
            activeThread={null}
            newSession
            readOnly={false}
            // On the very first run there is no session to fall back to, so the
            // directory picker is mandatory (non-dismissable) — the user must
            // choose a directory before reaching the new-session screen.
            workdirMandatory={sessions.length === 0}
          />
        ) : activeThread ? (
          <TranscriptPane
            threads={threads}
            activeThread={activeThread}
            readOnly={!focusedOpen}
          />
        ) : (
          <div className="flex h-full items-center justify-center text-sm text-slate-400">
            Select a session to view its conversation.
          </div>
        )}
      </div>

      {/* Terminal toggle (visible when the terminal is collapsed) */}
      {!terminalOpen && (
        <div className="absolute right-2 top-2 z-10">
          <Button size="sm" variant="secondary" onClick={toggleTerminal}>
            Terminal
          </Button>
        </div>
      )}

      {/* Right: terminal — attaches to the focused session's pane. */}
      {terminalOpen &&
        (isLargeScreen ? (
          <div
            className="relative z-20 shrink-0"
            style={{ width: terminalWidth }}
          >
            <TerminalResizeHandle />
            {terminal}
          </div>
        ) : (
          <div className="absolute inset-y-0 right-0 z-20 w-[min(90vw,28rem)] shadow-xl">
            {terminal}
          </div>
        ))}
    </div>
  );
}
