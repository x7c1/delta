/**
 * Turn, send, notice and transcript events: which queries they invalidate,
 * the unread badges, and the per-thread latest activity.
 *
 * Shared fixtures live in applySessionEvent.testkit.ts.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { QueryClient } from '@tanstack/react-query';
import { queryKeys } from '@delta/api-client';
import { applySessionEvent } from './applySessionEvent';
import { noticeOf, useLiveStore } from '../store/liveStore';
import {
  FOCUSED,
  resetApplySessionEventState,
} from './applySessionEvent.testkit';

describe('applySessionEvent', () => {
  beforeEach(resetApplySessionEventState);

  it('invalidates the focused active thread, its session threads, and its open sends on turn_started', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    applySessionEvent(
      {
        kind: 'turn_started',
        session_id: FOCUSED,
        thread_id: 5,
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
    // A matched send left the open list; the send-activity strip refetches.
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['session-sends', FOCUSED],
    });
  });

  it('invalidates the named thread on turn_started even before the freshly-spawned session has bound focus + active thread', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    // The WS frame arrives while the freshly-spawned session's focus +
    // active-thread binding has not settled: `focusedSessionId` is still the
    // new-session sentinel (mapped to null at the router boundary) and
    // `activeThreadId` is still null. Every `turn_started` carries its
    // `thread_id`, so the router routes the refetch by that — not by the
    // (still-null) focused client state — and a TranscriptPane that mounts
    // shortly after for this thread reuses the now-stale-marked cache entry
    // instead of relying on its first fetch to race the backend's writes.
    applySessionEvent(
      {
        kind: 'turn_started',
        session_id: 'sess-spawned',
        thread_id: 7,
        send_id: 1,
        matched_uuid: 'uuid-1',
      },
      queryClient,
      null,
      null,
    );

    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['messages', 7] });
    // The send-activity strip refetches regardless of focus (already covered by
    // earlier tests) — assert it here too so a future refactor that drops
    // either invalidate is caught.
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['session-sends', 'sess-spawned'],
    });
  });

  it('invalidates both the named thread and the focused active thread when a sibling thread completes', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    // The focused session's branch thread (6) completes while the user is
    // viewing the session's main thread (5). The branch is what grew, so its
    // messages refetch; the active thread also refetches so its tree
    // (`session-threads`) and any session-wide signal stay current.
    applySessionEvent(
      {
        kind: 'turn_completed',
        session_id: FOCUSED,
        thread_id: 6,
        stop_reason: null,
      },
      queryClient,
      5,
      FOCUSED,
    );

    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['messages', 6] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['messages', 5] });
  });

  it('falls back to the focused active thread when a turn end carries no thread_id', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    // A session-wide turn end (e.g. an interrupt that landed without a bound
    // thread). The transcript may still have grown via the session-level
    // signal, so the focused active thread refetches.
    applySessionEvent(
      {
        kind: 'turn_interrupted',
        session_id: FOCUSED,
        thread_id: null,
      },
      queryClient,
      5,
      FOCUSED,
    );

    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['messages', 5] });
  });

  it('refetches the open sends on send_dispatched and leaves the transcript alone', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    applySessionEvent(
      { kind: 'send_dispatched', session_id: FOCUSED, send_id: 3 },
      queryClient,
      5,
      FOCUSED,
    );

    // The queued→dispatched flip lives in the open-send list; the transcript
    // has not changed (the echo has not even fired yet). The store's only
    // reaction is retiring a parked-send notice for this very send (see
    // `reduceSendDispatched`), which no send here has.
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['session-sends', FOCUSED],
    });
    expect(invalidate).not.toHaveBeenCalledWith({ queryKey: ['messages', 5] });
    expect(useLiveStore.getState().runningThreads).toEqual({});
  });

  it('refetches the open sends on a subagent start and finish', () => {
    // Closes the race an in-flight open-sends envelope would otherwise win,
    // wiping the subagent this event just recorded — see the
    // `subagent_started` case in `applySessionEvent` for the mechanism.
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    applySessionEvent(
      {
        kind: 'subagent_started',
        session_id: FOCUSED,
        thread_id: 5,
        tool_use_id: 'toolu_1',
        subagent_type: 'general-purpose',
        description: 'Probe the codebase',
        background: false,
      },
      queryClient,
      5,
      FOCUSED,
    );
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['session-sends', FOCUSED],
    });
    expect(useLiveStore.getState().runningSubagents[FOCUSED]).toHaveLength(1);

    invalidate.mockClear();
    applySessionEvent(
      {
        kind: 'subagent_finished',
        session_id: FOCUSED,
        tool_use_id: 'toolu_1',
      },
      queryClient,
      5,
      FOCUSED,
    );
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['session-sends', FOCUSED],
    });
    expect(useLiveStore.getState().runningSubagents[FOCUSED]).toBeUndefined();
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
        thread_id: 8,
        stop_reason: null,
      },
      queryClient,
      5,
      FOCUSED,
    );

    // No invalidation of the FOCUSED session's active thread — but the event
    // names its own thread (8), so that thread's messages still refetch (a
    // no-op when no observer for it is mounted, which is the common case for
    // a background session). The session's open-send list also refetches so
    // its send-activity strip is right when the user views it.
    expect(invalidate).not.toHaveBeenCalledWith({ queryKey: ['messages', 5] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['messages', 8] });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['session-sends', 'other-session'],
    });
    // And the foreign turn must not drain the focused session's tracked send.
    expect(Object.keys(useLiveStore.getState().localSends)).toHaveLength(1);
  });

  it('bumps the completing thread of a non-focused session as unread', () => {
    const queryClient = new QueryClient();

    applySessionEvent(
      {
        kind: 'turn_completed',
        session_id: 'other-session',
        thread_id: 8,
        stop_reason: null,
      },
      queryClient,
      5,
      FOCUSED,
    );

    // A background completion produced something the user has not seen; THAT
    // thread's unread is bumped (the navigator OR-aggregates it onto the row).
    expect(useLiveStore.getState().unread).toEqual({ 8: 1 });
  });

  it('does not bump unread when the completing thread is the one on screen', () => {
    const queryClient = new QueryClient();

    applySessionEvent(
      { kind: 'turn_completed', session_id: FOCUSED, thread_id: 5, stop_reason: null },
      queryClient,
      5,
      FOCUSED,
    );

    // The focused session's active thread (5) is exactly what the user is
    // viewing, so nothing is unseen.
    expect(useLiveStore.getState().unread).toEqual({});
  });

  it('bumps unread for a non-active thread of the focused session', () => {
    const queryClient = new QueryClient();

    // The focused session is on thread 5, but a DIFFERENT thread (6) of the
    // same session completes — the user is not looking at it, so it is unread.
    applySessionEvent(
      { kind: 'turn_completed', session_id: FOCUSED, thread_id: 6, stop_reason: null },
      queryClient,
      5,
      FOCUSED,
    );

    expect(useLiveStore.getState().unread).toEqual({ 6: 1 });
  });

  it('does not bump unread on a turn interrupt, even for a non-focused session', () => {
    const queryClient = new QueryClient();

    applySessionEvent(
      { kind: 'turn_interrupted', session_id: 'other-session', thread_id: 8 },
      queryClient,
      5,
      FOCUSED,
    );

    // An interrupt is the user's own Escape/Ctrl-C, not a surprise completion
    // that needs flagging; only `turn_completed` bumps unread.
    expect(useLiveStore.getState().unread).toEqual({});
  });

  it('notices external_input on the focused active thread without badging it', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');
    applySessionEvent(
      { kind: 'external_input', session_id: FOCUSED, prompt: 'typed' },
      queryClient,
      9,
      FOCUSED,
    );

    // The notice — the user-visible record of the input — is keyed by the
    // focused session and names the thread it landed on, and that thread's
    // transcript refetches so the typed line appears.
    expect(
      noticeOf(useLiveStore.getState().notices, FOCUSED, 'external_input'),
    ).toMatchObject({
      threadId: 9,
      prompt: 'typed',
    });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['messages', 9] });

    // But NO unread. The event carries no `thread_id`, so the only thread it
    // can be attributed to is the focused active one — the thread on screen,
    // which is read by definition. A count written here was invisible while the
    // thread stayed active (its badge is suppressed) and no activation edge
    // ever came back to clear it, so it surfaced as a phantom "1" the moment
    // the user switched threads.
    expect(useLiveStore.getState().unread).toEqual({});
  });

  it('does not badge any thread when external_input arrives with no active thread', () => {
    const queryClient = new QueryClient();

    // The focus-transition window: the session is focused but its active thread
    // is not bound yet (a session switch nulls it, and the new-session screen
    // has none). Nothing may be attributed to "the next thread to become
    // active" — that would be the same phantom, one thread over.
    applySessionEvent(
      { kind: 'external_input', session_id: FOCUSED, prompt: 'typed' },
      queryClient,
      null,
      FOCUSED,
    );

    expect(useLiveStore.getState().unread).toEqual({});
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

  it('records the parked-send notice and refetches open sends on send_parked', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    applySessionEvent(
      {
        kind: 'send_parked',
        session_id: 'other-session',
        send_id: 42,
        text: 'never delivered',
      },
      queryClient,
      9,
      FOCUSED,
    );

    // Unlike external input, a parked send is recorded for EVERY session: the
    // user's own message was dropped, so it must be waiting for them when they
    // return to that session rather than only if they were watching it.
    expect(
      noticeOf(useLiveStore.getState().notices, 'other-session', 'send_parked'),
    ).toMatchObject({ sendId: 42 });
    // The row went back to the queue held for a release, so the open-send list
    // must be refetched rather than left showing a spinning chip.
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: queryKeys.sessionSends('other-session'),
    });
    // A dropped message is not "new content" — no unread badge.
    expect(useLiveStore.getState().unread).toEqual({});
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

  it('records per-thread latest activity for every thread a message landed on', () => {
    const queryClient = new QueryClient();

    // Every thread named by the event, including the one being viewed (5) —
    // unlike `bumpUnread`, which deliberately skips the active thread, this
    // answers "where did the last message land", and the active thread is an
    // ordinary answer to that.
    applySessionEvent(
      { kind: 'transcript_updated', session_id: FOCUSED, thread_ids: [2, 5] },
      queryClient,
      5,
      FOCUSED,
    );
    expect(Object.keys(useLiveStore.getState().threadActivity).sort()).toEqual([
      '2',
      '5',
    ]);

    // The turn events carry a thread too, and record it the same way — again
    // for the active thread, which raised no unread badge above.
    applySessionEvent(
      {
        kind: 'turn_completed',
        session_id: FOCUSED,
        thread_id: 9,
        stop_reason: null,
      },
      queryClient,
      5,
      FOCUSED,
    );
    expect(useLiveStore.getState().threadActivity[9]).toBeDefined();
    expect(useLiveStore.getState().unread[9]).toBe(1);

    // A session-wide turn end names no thread, so it moves nothing.
    const before = useLiveStore.getState().threadActivity;
    applySessionEvent(
      {
        kind: 'turn_completed',
        session_id: FOCUSED,
        thread_id: null,
        stop_reason: null,
      },
      queryClient,
      5,
      FOCUSED,
    );
    expect(useLiveStore.getState().threadActivity).toBe(before);
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
});
