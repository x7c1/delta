import { beforeEach, describe, expect, it, vi } from 'vitest';
import { QueryClient } from '@tanstack/react-query';
import { applySessionEvent } from './applySessionEvent';
import { useLiveStore } from '../store/liveStore';

const FOCUSED = 'sess-1';

describe('applySessionEvent', () => {
  beforeEach(() => {
    useLiveStore.setState({
      connection: 'connecting',
      pending: [],
      permission: null,
      unread: {},
      externalInput: null,
      resuming: {},
    });
  });

  it('invalidates the focused active thread and its session threads on turn_started', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    applySessionEvent(
      {
        kind: 'turn_started',
        session_id: FOCUSED,
        pending_send_id: 1,
        matched_uuid: 'uuid-1',
      },
      queryClient,
      5,
      FOCUSED,
    );

    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['messages', 5] });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['session-threads', FOCUSED],
    });
  });

  it('ignores a turn event for a non-focused session', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    // The focused session has a queued send in the FIFO.
    useLiveStore.getState().enqueueSend({
      localId: 'focused1',
      sendId: 1,
      sessionId: FOCUSED,
      threadId: 5,
      text: 'mine',
      semanticParentUuid: null,
      status: 'queued',
      createdAt: 0,
    });

    applySessionEvent(
      {
        kind: 'turn_completed',
        session_id: 'other-session',
        stop_reason: null,
      },
      queryClient,
      5,
      FOCUSED,
    );

    // No transcript/thread invalidation for a session the user is not viewing.
    expect(invalidate).not.toHaveBeenCalledWith({ queryKey: ['messages', 5] });
    // And the foreign turn must not drain the focused session's queue.
    expect(useLiveStore.getState().pending).toHaveLength(1);
    expect(useLiveStore.getState().pending[0].localId).toBe('focused1');
  });

  it('badges the focused active thread on external_input', () => {
    const queryClient = new QueryClient();
    applySessionEvent(
      { kind: 'external_input', session_id: FOCUSED, prompt: 'typed' },
      queryClient,
      9,
      FOCUSED,
    );

    expect(useLiveStore.getState().unread[9]).toBe(1);
    expect(useLiveStore.getState().externalInput?.prompt).toBe('typed');
  });

  it('ignores external_input for a non-focused session', () => {
    const queryClient = new QueryClient();
    applySessionEvent(
      { kind: 'external_input', session_id: 'other-session', prompt: 'typed' },
      queryClient,
      9,
      FOCUSED,
    );

    // A background session's typing must not badge or surface on the focused
    // transcript (regression: the marker used to be set unconditionally).
    expect(useLiveStore.getState().unread[9]).toBeUndefined();
    expect(useLiveStore.getState().externalInput).toBeNull();
  });

  it('invalidates the affected threads on transcript_updated without touching the FIFO', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    useLiveStore.getState().enqueueSend({
      localId: 'l1',
      sendId: 1,
      sessionId: FOCUSED,
      threadId: 2,
      text: 'hi',
      semanticParentUuid: null,
      status: 'queued',
      createdAt: 0,
    });
    useLiveStore.getState().bumpUnread(2);

    applySessionEvent(
      { kind: 'transcript_updated', session_id: FOCUSED, thread_ids: [2, 7] },
      queryClient,
      5,
      FOCUSED,
    );

    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['messages', 2] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['messages', 7] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['messages', 5] });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['session-threads', FOCUSED],
    });

    // No FIFO / unread mutation for this event.
    expect(useLiveStore.getState().pending).toHaveLength(1);
    expect(useLiveStore.getState().pending[0].status).toBe('queued');
    expect(useLiveStore.getState().unread[2]).toBe(1);
  });

  it('does not double-invalidate the active thread when it is already reported', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    applySessionEvent(
      { kind: 'transcript_updated', session_id: FOCUSED, thread_ids: [5] },
      queryClient,
      5,
      FOCUSED,
    );

    const messageInvalidations = invalidate.mock.calls.filter(
      ([arg]) =>
        Array.isArray(arg?.queryKey) &&
        arg.queryKey[0] === 'messages' &&
        arg.queryKey[1] === 5,
    );
    expect(messageInvalidations).toHaveLength(1);
  });

  it('invalidates the session list on the lifecycle events', () => {
    for (const kind of [
      'session_registered',
      'session_opened',
      'session_closed',
    ] as const) {
      const queryClient = new QueryClient();
      const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

      applySessionEvent(
        { kind, session_id: FOCUSED },
        queryClient,
        null,
        FOCUSED,
      );

      expect(invalidate).toHaveBeenCalledWith({ queryKey: ['sessions'] });
    }
  });

  it('clears a resuming marker when the session opens', () => {
    useLiveStore.getState().markResuming(FOCUSED);
    expect(useLiveStore.getState().resuming[FOCUSED]).toBe(true);

    applySessionEvent(
      { kind: 'session_opened', session_id: FOCUSED },
      new QueryClient(),
      null,
      FOCUSED,
    );

    expect(useLiveStore.getState().resuming[FOCUSED]).toBeUndefined();
  });

  it('routes a permission request to the store as a notice', () => {
    const queryClient = new QueryClient();
    applySessionEvent(
      {
        kind: 'permission_requested',
        session_id: FOCUSED,
        request_id: 2,
        tool_name: 'Edit',
      },
      queryClient,
      1,
      FOCUSED,
    );

    expect(useLiveStore.getState().permission).toEqual({
      requestId: 2,
      toolName: 'Edit',
    });
  });
});
