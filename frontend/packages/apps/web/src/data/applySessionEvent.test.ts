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

  it('invalidates the affected threads on transcript_updated without touching the FIFO', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    // Seed a queued send and an unread badge to prove neither is mutated.
    useLiveStore.getState().enqueueSend({
      localId: 'l1',
      sendId: 1,
      threadId: 2,
      text: 'hi',
      semanticParentUuid: null,
      status: 'queued',
      createdAt: 0,
    });
    useLiveStore.getState().bumpUnread(2);

    applySessionEvent(
      { kind: 'transcript_updated', session_id: 'sess-1', thread_ids: [2, 7] },
      queryClient,
      5,
    );

    // Every reported thread, plus the active thread, plus the tree are refetched.
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['messages', 2] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['messages', 7] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['messages', 5] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['threads'] });

    // No FIFO / unread mutation for this event.
    expect(useLiveStore.getState().pending).toHaveLength(1);
    expect(useLiveStore.getState().pending[0].status).toBe('queued');
    expect(useLiveStore.getState().unread[2]).toBe(1);
  });

  it('does not double-invalidate the active thread when it is already reported', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    applySessionEvent(
      { kind: 'transcript_updated', session_id: 'sess-1', thread_ids: [5] },
      queryClient,
      5,
    );

    const messageInvalidations = invalidate.mock.calls.filter(
      ([arg]) =>
        Array.isArray(arg?.queryKey) &&
        arg.queryKey[0] === 'messages' &&
        arg.queryKey[1] === 5,
    );
    expect(messageInvalidations).toHaveLength(1);
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
