import { useEffect } from 'react';
import { useThreadsQuery, useSessionQuery } from '@delta/api-client';
import { Button } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useSessionEvents } from '../../data/useSessionEvents';
import { useNavStore } from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';
import { useMediaQuery } from '../../hooks/useMediaQuery';
import { NavigatorPane } from '../navigator/NavigatorPane';
import { TranscriptPane } from '../transcript/TranscriptPane';
import { TerminalPane } from '../terminal/TerminalPane';
import { TerminalResizeHandle } from '../terminal/TerminalResizeHandle';

/**
 * The top-level two-pane workspace with a responsive terminal third pane:
 * navigator | transcript | terminal. On small screens the terminal slides in
 * from the right as an overlay; on large screens it is a persistent collapsible
 * pane.
 */
export function WorkspaceScreen() {
  const client = useApiClient();
  useSessionEvents();

  const sessionQuery = useSessionQuery(client);
  const threadsQuery = useThreadsQuery(client);
  const threads = threadsQuery.data?.threads ?? [];

  const activeThreadId = useNavStore((state) => state.activeThreadId);
  const setActiveThread = useNavStore((state) => state.setActiveThread);
  const terminalOpen = useNavStore((state) => state.terminalOpen);
  const toggleTerminal = useNavStore((state) => state.toggleTerminal);
  const terminalWidth = useNavStore((state) => state.terminalWidth);
  const clearUnread = useLiveStore((state) => state.clearUnread);

  // At `lg`+ the terminal is a static, resizable pane; below it is a fixed-width
  // slide-in overlay with no resizer. Matches Tailwind's `lg` breakpoint.
  const isLargeScreen = useMediaQuery('(min-width: 1024px)');

  // Default the active thread to the main thread once it is known.
  useEffect(() => {
    if (activeThreadId === null && sessionQuery.data) {
      setActiveThread(sessionQuery.data.main_thread_id);
    }
  }, [activeThreadId, sessionQuery.data, setActiveThread]);

  // Clear the unread badge whenever a thread becomes active, regardless of how
  // it was activated (tree click, breadcrumb, branch chip, or the default).
  useEffect(() => {
    if (activeThreadId !== null) {
      clearUnread(activeThreadId);
    }
  }, [activeThreadId, clearUnread]);

  const activeThread =
    threads.find((thread) => thread.id === activeThreadId) ?? null;

  if (sessionQuery.isLoading) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-slate-400">
        Connecting to the session…
      </div>
    );
  }

  if (sessionQuery.isError) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-sm text-slate-500">
        <p>No session yet.</p>
        <p className="text-xs text-slate-400">
          Send your first message to Claude in the terminal
          (<code>tmux attach -t delta</code>) to begin.
        </p>
      </div>
    );
  }

  return (
    <div className="relative flex h-full overflow-hidden">
      {/* Left: navigator */}
      <div className="w-64 shrink-0">
        <NavigatorPane threads={threads} />
      </div>

      {/* Center: transcript */}
      <div className="min-w-0 flex-1">
        {activeThread ? (
          <TranscriptPane threads={threads} activeThread={activeThread} />
        ) : (
          <div className="flex h-full items-center justify-center text-sm text-slate-400">
            Select a thread, or send the first message in main.
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

      {/* Right: terminal — persistent resizable pane on lg, slide-in overlay
          below lg. Only the static lg pane gets the inline pixel width and the
          drag handle; the overlay keeps its fixed responsive width. */}
      {terminalOpen &&
        (isLargeScreen ? (
          <div
            className="relative z-20 shrink-0"
            style={{ width: terminalWidth }}
          >
            <TerminalResizeHandle />
            <TerminalPane />
          </div>
        ) : (
          <div className="absolute inset-y-0 right-0 z-20 w-[min(90vw,28rem)] shadow-xl">
            <TerminalPane />
          </div>
        ))}
    </div>
  );
}
