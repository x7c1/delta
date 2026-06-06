import { beforeEach, describe, expect, it, vi } from 'vitest';
import { QueryClient } from '@tanstack/react-query';
import { applySessionEvent } from './applySessionEvent';
import { useLiveStore } from '../store/liveStore';

describe('applySessionEvent', () => {
  beforeEach(() => {
    useLiveStore.setState({
      connection: 'connecting',
      pending: [],
      permission: null,
      unread: {},
      externalInput: null,
    });
  });

  it('invalidates the active thread messages on turn_started', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    applySessionEvent(
      {
        kind: 'turn_started',
        session_id: 'sess-1',
        pending_send_id: 1,
        matched_uuid: 'uuid-1',
      },
      queryClient,
      5,
    );

    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['messages', 5] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['threads'] });
  });

  it('badges the active thread on external_input', () => {
    const queryClient = new QueryClient();
    applySessionEvent(
      { kind: 'external_input', session_id: 'sess-1', prompt: 'typed' },
      queryClient,
      9,
    );

    expect(useLiveStore.getState().unread[9]).toBe(1);
    expect(useLiveStore.getState().externalInput?.prompt).toBe('typed');
  });

  it('routes a permission request to the store as a notice', () => {
    const queryClient = new QueryClient();
    applySessionEvent(
      {
        kind: 'permission_requested',
        session_id: 'sess-1',
        request_id: 2,
        tool_name: 'Edit',
      },
      queryClient,
      1,
    );

    expect(useLiveStore.getState().permission).toEqual({
      requestId: 2,
      toolName: 'Edit',
    });
  });
});
