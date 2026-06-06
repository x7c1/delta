import { useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { WsEventSource, type SessionEventSource } from '@delta/api-client';
import { isMockMode, wsUrl } from '../config';
import { useLiveStore } from '../store/liveStore';
import { useNavStore } from '../store/navStore';
import { applySessionEvent } from './applySessionEvent';
import { createMockEventSource } from './mockEventControl';

/**
 * Open the live event source (the real `/ws` client, or the dev fake in mock
 * mode), and route every event through {@link applySessionEvent}. Connection
 * status is mirrored into the live store.
 */
export function useSessionEvents(): void {
  const queryClient = useQueryClient();
  const setConnection = useLiveStore((state) => state.setConnection);

  useEffect(() => {
    const source: SessionEventSource = isMockMode()
      ? createMockEventSource()
      : new WsEventSource({ url: wsUrl('/ws') });

    const offEvent = source.onEvent((event) => {
      // Read the latest active thread at event time, not at subscribe time.
      const activeThreadId = useNavStore.getState().activeThreadId;
      applySessionEvent(event, queryClient, activeThreadId);
    });
    const offStatus = source.onStatus((status) => setConnection(status));

    return () => {
      offEvent();
      offStatus();
      source.close();
    };
  }, [queryClient, setConnection]);
}
