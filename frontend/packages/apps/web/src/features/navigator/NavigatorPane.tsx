import type { SessionListItem, Thread } from '@delta/model';
import { Button, Panel, Spinner, StatusDot, type DotTone } from '@delta/ui-kit';
import {
  useCloseSessionMutation,
  type ConnectionStatus,
} from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { useLiveStore } from '../../store/liveStore';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { SessionNode } from './SessionNode';

export interface NavigatorPaneProps {
  /** Every session, ordered by creation. */
  sessions: SessionListItem[];
  /** The focused session's thread tree (empty when none is focused). */
  threads: Thread[];
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
 * open-session count, the permission notice, a running indicator, and the live
 * connection status. Top-level nodes are sessions; expanding the focused session
 * reveals its thread tree.
 */
export function NavigatorPane({ sessions, threads }: NavigatorPaneProps) {
  const client = useApiClient();
  const closeSession = useCloseSessionMutation(client);

  const connection = useLiveStore((state) => state.connection);
  const permission = useLiveStore((state) => state.permission);
  const dismissPermission = useLiveStore((state) => state.dismissPermission);
  const hasInProgress = useLiveStore((state) =>
    state.pending.some((item) => item.status === 'in_progress'),
  );

  const focusedSessionId = useNavStore((state) => state.focusedSessionId);
  const setFocusedSession = useNavStore((state) => state.setFocusedSession);
  const setTerminalOpen = useNavStore((state) => state.setTerminalOpen);

  const openCount = sessions.filter((item) => item.open).length;

  return (
    <Panel
      className="border-r border-slate-200"
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
          <div className="flex items-center gap-2">
            <span
              className="text-xs text-slate-500"
              data-testid="open-session-count"
            >
              open: {openCount}
            </span>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => setFocusedSession(NEW_SESSION_FOCUS)}
            >
              New
            </Button>
          </div>
        </div>
      }
      footer={hasInProgress ? <Spinner label="running" /> : undefined}
    >
      {permission && (
        <div className="space-y-1 border-b border-amber-200 bg-amber-50 px-3 py-2 text-xs">
          <p className="font-medium text-amber-800">
            Permission requested: {permission.toolName}
          </p>
          <p className="text-slate-600">Answer the prompt in the terminal.</p>
          <div className="flex gap-2">
            <Button size="sm" onClick={() => setTerminalOpen(true)}>
              Open terminal
            </Button>
            <Button size="sm" variant="ghost" onClick={dismissPermission}>
              Dismiss
            </Button>
          </div>
        </div>
      )}

      {focusedSessionId === NEW_SESSION_FOCUS && (
        <div
          className="mx-2 mb-1.5 mt-1.5 rounded-lg border border-indigo-300 bg-indigo-50/70 px-2 py-2 text-xs text-indigo-700 shadow-sm ring-1 ring-indigo-200"
          data-testid="new-session-node"
        >
          New session — send the first message to start it.
        </div>
      )}

      <ul className="pb-2 pt-1.5">
        {sessions.map((item) => (
          <SessionNode
            key={item.session.id}
            item={item}
            isFocused={focusedSessionId === item.session.id}
            threads={
              focusedSessionId === item.session.id ? threads : undefined
            }
            onFocus={() => setFocusedSession(item.session.id)}
            onClose={() => closeSession.mutate(item.session.id)}
          />
        ))}
      </ul>
    </Panel>
  );
}
