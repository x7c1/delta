import { beforeEach, describe, expect, it, vi } from 'vitest';
import { QueryClient } from '@tanstack/react-query';
import { queryKeys } from '@delta/api-client';
import type { Send } from '@delta/wire-gen';
import { applySessionEvent } from './applySessionEvent';
import { noticeOf, useLiveStore } from '../store/liveStore';

const FOCUSED = 'sess-1';

function serverSend(overrides: Partial<Send> = {}): Send {
  return {
    id: 1,
    session_id: FOCUSED,
    thread_id: 5,
    semantic_parent_uuid: null,
    text: 'hi',
    locator_quote: null,
    status: 'dispatched',
    matched_uuid: null,
    created_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

describe('applySessionEvent', () => {
  beforeEach(() => {
    useLiveStore.setState({
      connection: 'connecting',
      sending: [],
      localSends: {},
      spawns: [],
      activeTurns: {},
      notices: {},
      unread: {},
      streamingMessages: {},
    });
  });

  it('invalidates the focused active thread, its session threads, and its open sends on turn_started', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    applySessionEvent(
      {
        kind: 'turn_started',
        session_id: FOCUSED,
        send_id: 1,
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
    // A matched send left the open list; the pending strip refetches.
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['session-sends', FOCUSED],
    });
  });

  it('refetches the open sends on send_dispatched and touches nothing else', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    applySessionEvent(
      { kind: 'send_dispatched', session_id: FOCUSED, send_id: 3 },
      queryClient,
      5,
      FOCUSED,
    );

    // The queued→dispatched flip lives in the open-send list; the transcript
    // has not changed (the echo has not even fired yet).
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['session-sends', FOCUSED],
    });
    expect(invalidate).not.toHaveBeenCalledWith({ queryKey: ['messages', 5] });
    expect(useLiveStore.getState().activeTurns).toEqual({});
  });

  it('refetches the open sends of a non-focused session on its turn events', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');
    // The focused session has a tracked local send.
    useLiveStore.getState().recordLocalSend({
      sendId: 9,
      sessionId: FOCUSED,
      threadId: 5,
      text: 'mine',
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

    // No transcript invalidation for a session the user is not viewing — but
    // its open-send list still refetches so its strip is right when viewed.
    expect(invalidate).not.toHaveBeenCalledWith({ queryKey: ['messages', 5] });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['session-sends', 'other-session'],
    });
    // And the foreign turn must not drain the focused session's tracked send.
    expect(Object.keys(useLiveStore.getState().localSends)).toHaveLength(1);
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
    // The notice is keyed by the focused session.
    expect(
      noticeOf(useLiveStore.getState().notices, FOCUSED, 'external_input'),
    ).toMatchObject({
      threadId: 9,
      prompt: 'typed',
    });
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
    expect(useLiveStore.getState().notices).toEqual({});
  });

  it('invalidates the affected threads and the open sends on transcript_updated', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');
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
    // An ingested user line is what matches a dispatched send, so the open
    // list refetches here too.
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['session-sends', FOCUSED],
    });

    // No unread mutation for this event.
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

  it('fails the tracked spawn and drops its cached sends on spawn_failed', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');
    // The spawn was accepted with real ids; its first send is in the cache.
    useLiveStore.getState().trackSpawn({
      sessionId: 'sess-spawned',
      threadId: 42,
      text: 'new session',
      workdir: null,
    });
    queryClient.setQueryData(queryKeys.sessionSends('sess-spawned'), {
      sends: [serverSend({ session_id: 'sess-spawned', thread_id: 42 })],
    });

    applySessionEvent(
      { kind: 'spawn_failed', session_id: 'sess-spawned', pane_token: 'pane-1' },
      queryClient,
      null,
      FOCUSED,
    );

    expect(useLiveStore.getState().spawns[0].status).toBe('failed');
    // The server deleted the row; the cached open sends go with it (a refetch
    // would only 404).
    expect(
      queryClient.getQueryData(queryKeys.sessionSends('sess-spawned')),
    ).toBeUndefined();
    // The spawn never registered, so there is no session row to refetch.
    expect(invalidate).not.toHaveBeenCalledWith({ queryKey: ['sessions'] });
  });

  it('routes a permission request to the store as a notice', () => {
    const queryClient = new QueryClient();
    applySessionEvent(
      {
        kind: 'permission_requested',
        session_id: FOCUSED,
        request_id: 2,
        tool_name: 'Edit',
        tool_input: '{}',
      },
      queryClient,
      1,
      FOCUSED,
    );

    expect(
      noticeOf(useLiveStore.getState().notices, FOCUSED, 'permission'),
    ).toEqual({
      kind: 'permission',
      requestId: 2,
      toolName: 'Edit',
      toolInput: '{}',
      dismissed: false,
    });
  });
});
