import { useEffect, useMemo } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import {
  invalidateThreadMessages,
  useProvidersQuery,
  useSessionsQuery,
  useSessionThreadsQuery,
} from '@delta/api-client';
import type {
  AgentProvider,
  ProviderCapabilities,
  SessionListItem,
} from '@delta/wire-gen';
import { Button, ErrorBoundary } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useSessionEvents } from '../../data/useSessionEvents';
import {
  NEW_SESSION_FOCUS,
  useNavStore,
  type FocusedSession,
} from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';
import { useGarbageCollectSessionScopedStorage } from '../../store/sessionScopedStorage';
import { useMediaQuery } from '../../hooks/useMediaQuery';
import { NavigatorPane } from '../navigator/NavigatorPane';
import { SettingsView } from '../settings/SettingsView';
import { TranscriptPane } from '../transcript/TranscriptPane';
import { TerminalPane } from '../terminal/TerminalPane';
import { TerminalFallback } from '../terminal/TerminalFallback';
import { TerminalResizeHandle } from '../terminal/TerminalResizeHandle';

/**
 * A terminal-screen glyph for the "Terminal" reopen button so it reads as a
 * terminal at a glance. Decorative — always `aria-hidden`, so the button's
 * accessible name stays its "Terminal" label.
 */
function TerminalIcon({ className }: { className?: string }) {
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
      <rect width="18" height="18" x="3" y="3" rx="2" />
      <path d="m7 11 2-2-2-2" />
      <path d="M11 13h4" />
    </svg>
  );
}

/**
 * Tailwind class string for the "Terminal" reopen button. Lives next to the
 * matching collapsed-state class in `ThreadTimelineOverlay`
 * ({@link TIMELINE_TOGGLE_BUTTON_CLASS}) — the two buttons sit side-by-side
 * in the transcript pane's top region, so they must read as the same
 * control shape: same border, radius, padding, font, shadow, and hover.
 * Keeping the literal class chain spelled out here (rather than imported)
 * keeps each side reviewable on its own.
 */
const TERMINAL_TOGGLE_BUTTON_CLASS =
  'inline-flex items-center gap-1.5 rounded-md border border-border-default bg-surface px-3 py-1.5 text-caption font-medium text-fg shadow-md transition-colors hover:bg-surface-elevated';

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

  // Drop localStorage entries for sessions that no longer exist — preferences
  // keyed by session id (e.g. the timeline footer's expand/collapse) leak one
  // key per session as sessions are deleted (here, on another device, or by
  // direct DB edits). The GC runs once after every page of sessions arrives,
  // gated until the full list is loaded so a still-paginating cold start does
  // not falsely flag late-page sessions as orphans.
  //
  // This is the app-shell registration point: feature components must not
  // invoke the GC themselves; the session list lives here.
  const gcSessionIds = useMemo<readonly string[] | null>(
    () =>
      sessionsQuery.isSuccess && !sessionsQuery.hasNextPage
        ? sessions.map((item) => item.session.id)
        : null,
    [sessionsQuery.isSuccess, sessionsQuery.hasNextPage, sessions],
  );
  useGarbageCollectSessionScopedStorage(gcSessionIds);

  // Provider capability profiles (`GET /api/providers`), indexed by provider id
  // so a focused session's terminal surface can be resolved from its provider.
  // This is the same query the new-session selector uses for launch
  // availability; consuming it here reads the capability side of the response.
  const providersQuery = useProvidersQuery(client);
  const capabilitiesByProvider = useMemo(() => {
    const map = new Map<AgentProvider, ProviderCapabilities>();
    for (const entry of providersQuery.data?.providers ?? []) {
      map.set(entry.provider, entry.capabilities);
    }
    return map;
  }, [providersQuery.data]);

  const focusedSessionId = useNavStore((state) => state.focusedSessionId);
  const activeThreadId = useNavStore((state) => state.activeThreadId);
  // Both focus writes below are the workspace resolving focus against the
  // loaded session list, never a user navigation, so they use the reconciling
  // setter — it leaves any overlay the user opened in the meantime standing.
  const reconcileFocusedSession = useNavStore(
    (state) => state.reconcileFocusedSession,
  );
  const setActiveThread = useNavStore((state) => state.setActiveThread);
  const terminalOpen = useNavStore((state) => state.terminalOpen);
  const toggleTerminal = useNavStore((state) => state.toggleTerminal);
  const terminalWidth = useNavStore((state) => state.terminalWidth);
  const clearUnread = useLiveStore((state) => state.clearUnread);
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
      reconcileFocusedSession(listed[listed.length - 1].sessionId);
    }
    for (const spawn of listed) {
      clearSpawn(spawn.sessionId);
    }
  }, [
    sessionsQuery.isSuccess,
    sessions,
    spawns,
    isNewSessionFocus,
    reconcileFocusedSession,
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
      reconcileFocusedSession(pickInitialFocus(sessions));
    }
  }, [
    sessionsQuery.isSuccess,
    sessionsQuery.hasNextPage,
    sessions,
    focusedSessionId,
    isNewSessionFocus,
    reconcileFocusedSession,
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

  // Clear the unread badge whenever a thread becomes active. Unread is
  // thread-keyed, so activating a thread clears exactly its badge; the
  // collapsed session row's OR-aggregated dot clears once its last unread
  // thread is viewed. Focusing a session activates its main thread (see
  // NavigatorPane), which clears main's unread through this same path.
  useEffect(() => {
    if (activeThreadId !== null) {
      clearUnread(activeThreadId);
    }
  }, [activeThreadId, clearUnread]);

  // Refetch the bound thread's messages on every active-thread transition.
  // The session-event router (`applySessionEvent`) invalidates the messages
  // cache only for queries that have already been observed; turn lifecycle
  // and transcript-update events that arrive BEFORE the TranscriptPane mounts
  // for a freshly-spawned session are therefore no-ops on the not-yet-existing
  // cache entry. Without a binding-side flush the first mount's fetch is the
  // ONLY chance to capture pre-bind growth, which under slow scheduling can
  // race the backend's user-line write: the fetch returns empty, no
  // subsequent event re-invalidates that key (the user-line `transcript_updated`
  // already fired), and the messages stay at 0 until the next persisted line
  // 3 s later — at which point everything lands at once. Invalidating on bind
  // forces an extra refetch right after the mount's initial fetch resolves, so
  // any DB state that caught up in the interim is picked up immediately.
  const queryClient = useQueryClient();
  useEffect(() => {
    if (activeThreadId !== null) {
      invalidateThreadMessages(queryClient, activeThreadId);
    }
  }, [activeThreadId, queryClient]);

  const activeThread =
    threads.find((thread) => thread.id === activeThreadId) ?? null;

  if (sessionsQuery.isPending) {
    return (
      <div className="flex h-full items-center justify-center text-secondary text-fg-subtle">
        Loading sessions…
      </div>
    );
  }

  if (sessionsQuery.isError) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-secondary text-fg-muted">
        <p>Could not load sessions.</p>
        <p className="text-caption text-fg-subtle">
          Make sure the Delta server is running, then reload.
        </p>
        <Button size="sm" variant="secondary" onClick={() => sessionsQuery.refetch()}>
          Retry
        </Button>
      </div>
    );
  }

  const focusedOpen = focusedItem?.open ?? false;

  // Whether the focused session's provider offers an attachable terminal, read
  // from its capability profile — never from `provider === 'claude'`. A provider
  // with no terminal (Codex's headless app-server) hides the terminal toggle and
  // pane entirely, and — because the pane is what opens the `/pty` bridge — must
  // never mount it in the first place.
  //
  // The providers-loading window is the subtle case. Failing OPEN to `true` while
  // the query is still in flight would briefly mount the pane for the focused
  // session and open its `/pty` bridge before the capability is known: with
  // `terminalOpen` persisted `true` (from a previous Claude session) and a Codex
  // session focused on reload, that fires a PTY websocket the backend rejects
  // with a "session is not open" warning. So while the profile is unresolved we
  // WITHHOLD the terminal rather than fail open; a real terminal provider
  // (Claude) attaches the instant the query resolves, and the `/pty` behaviour it
  // then drives is byte-identical to before. Fail open only in the two cases
  // where there is genuinely nothing to wait for: the new-session screen (no
  // focused session), and a query that has SUCCEEDED but does not list the
  // focused provider (an unrecognised provider — keep the historical default).
  const focusedProvider = focusedItem?.session.provider ?? null;
  const focusedCapabilities =
    focusedProvider === null
      ? undefined
      : capabilitiesByProvider.get(focusedProvider);
  const focusedHasTerminal =
    focusedProvider === null
      ? true
      : focusedCapabilities !== undefined
        ? focusedCapabilities.has_terminal
        : providersQuery.isSuccess;

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
      <TerminalPane
        sessionId={focusedRealSessionId}
        attachable={focusedOpen}
        hasTerminal={focusedHasTerminal}
      />
    </ErrorBoundary>
  );

  // The Terminal reopen button rides at the right end of the transcript
  // pane's top region (next to the collapsed timeline toggle) so the two
  // controls share one row and the timeline card can grow downward without
  // overlapping anything. `null` while the terminal is open: the transcript
  // pane drops the slot entirely and the top region centers on whatever is
  // left (timeline toggle alone, or nothing at all on the new-session
  // screen).
  const terminalToggleButton =
    focusedHasTerminal && !terminalOpen ? (
      <button
        type="button"
        onClick={toggleTerminal}
        data-testid="terminal-toggle"
        className={TERMINAL_TOGGLE_BUTTON_CLASS}
      >
        <TerminalIcon className="h-3.5 w-3.5" />
        Terminal
      </button>
    ) : null;

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
            terminalButton={terminalToggleButton}
          />
        ) : activeThread ? (
          <TranscriptPane
            threads={threads}
            activeThread={activeThread}
            readOnly={!focusedOpen}
            terminalButton={terminalToggleButton}
          />
        ) : (
          <div className="flex h-full items-center justify-center text-secondary text-fg-subtle">
            Select a session to view its conversation.
          </div>
        )}
      </div>

      {/* Right: terminal — attaches to the focused session's pane. Gated on the
          focused provider's terminal capability as well as `terminalOpen`, so a
          provider with no terminal (Codex) never shows a pane even if
          `terminalOpen` was persisted true from a previous Claude session. */}
      {terminalOpen &&
        focusedHasTerminal &&
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

      {/* Settings is a modal overlay layered over the workspace rather than a
          full-pane mode: it self-gates on `settingsOpen` (renders nothing when
          closed) and leaves the center conversation pane in place beneath it. */}
      <SettingsView />
    </div>
  );
}
