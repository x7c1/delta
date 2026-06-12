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
    const source: SessionEventSource = isMockMode()
      ? createMockEventSource()
      : new WsEventSource({ url: wsUrl('/ws') });

    const offEvent = source.onEvent((event) => {
      // Read the latest focus at event time, not at subscribe time. The
      // new-session sentinel has no real id yet, so map it to null for routing.
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
    });

    // Track connections so a *re*-open (not the first) triggers a resync.
    let hasConnected = false;
    const offStatus = source.onStatus((status) => {
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
    });

    return () => {
      offEvent();
      offStatus();
      source.close();
    };
  }, [queryClient, setConnection]);
}
