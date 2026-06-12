import { useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { WsEventSource, type SessionEventSource } from '@delta/api-client';
import { isMockMode, wsUrl } from '../config';
import { useLiveStore } from '../store/liveStore';
import { NEW_SESSION_FOCUS, useNavStore } from '../store/navStore';
import { applySessionEvent } from './applySessionEvent';
import { createMockEventSource } from './mockEventControl';

/**
 * Open the live event source (the real `/ws` client, or the dev fake in mock
 * mode), and route every event through {@link applySessionEvent}. Connection
 * status is mirrored into the live store.
 *
 * The source reconnects on its own after a dropped socket. Events broadcast
 * during the gap are lost (the server does not replay), so on every *re*-open
 * we resync: refetch all REST resources — sessions, threads, messages, and the
 * open-send lists behind the pending strip all catch up — and drop the
 * event-reconstructed turn ephemera (tracked local sends, active-turn flags),
 * whose turn-end drains may have been missed and cannot be recovered by any
 * refetch; otherwise a stuck "in progress" chip would linger until a reload.
 */
export function useSessionEvents(): void {
  const queryClient = useQueryClient();
  const setConnection = useLiveStore((state) => state.setConnection);

  useEffect(() => {
    let disposed = false;
    let source: SessionEventSource | null = null;
    const teardowns: Array<() => void> = [];

    const attach = (s: SessionEventSource) => {
      source = s;
      teardowns.push(
        s.onEvent((event) => {
          // Read the latest focus at event time, not at subscribe time. The
          // new-session sentinel has no real id yet, so map it to null for
          // routing.
          const { activeThreadId, focusedSessionId } = useNavStore.getState();
          const focusedRealSessionId =
            focusedSessionId === null || focusedSessionId === NEW_SESSION_FOCUS
              ? null
              : focusedSessionId;
          applySessionEvent(
            event,
            queryClient,
            activeThreadId,
            focusedRealSessionId,
          );
        }),
      );

      // Track connections so a *re*-open (not the first) triggers a resync.
      let hasConnected = false;
      teardowns.push(
        s.onStatus((status) => {
          setConnection(status);
          if (status !== 'open') {
            return;
          }
          if (hasConnected) {
            // Reconnected after a gap: heal the missed window.
            void queryClient.invalidateQueries();
            useLiveStore.getState().resetTurnEphemera();
          }
          hasConnected = true;
        }),
      );
    };

    if (isMockMode()) {
      // The dev fake lives behind a dynamic import (`@delta/api-mocks` stays
      // out of the production bundle), so attaching is deferred until it
      // loads — and skipped entirely if this effect was cleaned up meanwhile.
      void createMockEventSource().then((s) => {
        if (disposed) {
          s.close();
          return;
        }
        attach(s);
      });
    } else {
      attach(new WsEventSource({ url: wsUrl('/ws') }));
    }

    return () => {
      disposed = true;
      for (const teardown of teardowns) {
        teardown();
      }
      source?.close();
    };
  }, [queryClient, setConnection]);
}
