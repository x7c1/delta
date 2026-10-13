/**
 * The per-session notices `applyEvent` raises and clears.
 *
 * Shared helpers live in liveStore.testkit.ts.
 */
import { beforeEach, describe, expect, it } from 'vitest';
import { noticeOf, RESOLVED_REQUESTS_KEPT, useLiveStore } from './liveStore';
import { localSend, notices, reset } from './liveStore.testkit';

describe('liveStore.applyEvent notices', () => {
  beforeEach(reset);

  const PERMISSION_NOTICE = {
    kind: 'permission',
    requestId: 7,
    toolName: 'Bash',
    toolInput: '{}',
    dismissed: false,
    queued: [],
    pendingCount: 1,
  } as const;

  function requestPermission(
    sessionId = 'sess-1',
    requestId = 7,
    toolName = 'Bash',
  ) {
    useLiveStore.getState().applyEvent({
      kind: 'permission_requested',
      session_id: sessionId,
      request_id: requestId,
      tool_name: toolName,
      tool_input: '{}',
    });
  }

  function resolvePermission(sessionId: string, requestId: number) {
    useLiveStore.getState().applyEvent({
      kind: 'permission_resolved',
      session_id: sessionId,
      request_id: requestId,
    });
  }

  it('records a permission notice per session and flags it dismissed on dismiss', () => {
    requestPermission();
    expect(noticeOf(notices(), 'sess-1', 'permission')).toEqual(
      PERMISSION_NOTICE,
    );

    // Dismissing keeps the entry, flagged: removal would let the next sends
    // refetch re-seed the same still-pending request and resurrect the card.
    useLiveStore.getState().dismissPermission('sess-1');
    expect(noticeOf(notices(), 'sess-1', 'permission')).toEqual({
      ...PERMISSION_NOTICE,
      dismissed: true,
    });
  });

  it('keeps permission notices for different sessions independent', () => {
    requestPermission('sess-1', 1);
    requestPermission('sess-2', 2);

    // Dismissing one session leaves the other's notice intact.
    useLiveStore.getState().dismissPermission('sess-1');
    expect(noticeOf(notices(), 'sess-1', 'permission')?.dismissed).toBe(true);
    expect(noticeOf(notices(), 'sess-2', 'permission')).toEqual({
      ...PERMISSION_NOTICE,
      requestId: 2,
    });
  });

  it('clears a session permission notice when its turn completes', () => {
    requestPermission();

    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    // The turn ended, so the prompt that was blocking the session is resolved.
    expect(noticeOf(notices(), 'sess-1', 'permission')).toBeNull();
  });

  it('clears a session permission notice when its request resolves', () => {
    requestPermission();

    useLiveStore.getState().applyEvent({
      kind: 'permission_resolved',
      session_id: 'sess-1',
      request_id: 7,
    });
    // The correlated tool_result was ingested, so the notice is cleared.
    expect(noticeOf(notices(), 'sess-1', 'permission')).toBeNull();
  });

  it('ignores a resolution for a different request than the current notice', () => {
    requestPermission('sess-1', 8);

    // A stale resolution for an older request must not wipe the live notice.
    resolvePermission('sess-1', 7);
    expect(noticeOf(notices(), 'sess-1', 'permission')).toEqual({
      ...PERMISSION_NOTICE,
      requestId: 8,
    });
  });

  it('clears a session permission notice when the session closes', () => {
    requestPermission();

    useLiveStore.getState().applyEvent({
      kind: 'session_closed',
      session_id: 'sess-1',
    });
    expect(noticeOf(notices(), 'sess-1', 'permission')).toBeNull();
  });

  it('seedPermission re-creates the notice the missed event would have set', () => {
    // After a reconnect the refetched sends envelope reports the pending
    // dialog; a non-null report re-seeds the notice the dropped
    // `permission_requested` would have set.
    useLiveStore.getState().seedPermission(
      'sess-1',
      { request_id: 7, tool_name: 'Bash', tool_input: '{}' },
      1,
    );
    expect(noticeOf(notices(), 'sess-1', 'permission')).toEqual(
      PERMISSION_NOTICE,
    );
  });

  it('seedPermission refuses a request permission_resolved already resolved', () => {
    // A sends envelope assembled while the request was still pending can land
    // AFTER the resolution cleared the card. The card is gone, so the
    // same-request guard sees nothing to match; the resolved id must hold.
    requestPermission();
    resolvePermission('sess-1', 7);
    expect(noticeOf(notices(), 'sess-1', 'permission')).toBeNull();

    useLiveStore.getState().seedPermission(
      'sess-1',
      { request_id: 7, tool_name: 'Bash', tool_input: '{}' },
      1,
    );
    expect(noticeOf(notices(), 'sess-1', 'permission')).toBeNull();

    // A genuinely new request has a new id and still seeds.
    useLiveStore.getState().seedPermission(
      'sess-1',
      { request_id: 8, tool_name: 'Bash', tool_input: '{}' },
      1,
    );
    expect(noticeOf(notices(), 'sess-1', 'permission')).toEqual({
      ...PERMISSION_NOTICE,
      requestId: 8,
    });
  });

  it('seedPermission refuses a resolved request no card was showing yet', () => {
    // The resolution can outrun the seed that would have shown the card; the
    // stale envelope naming it must not show it afterwards either.
    resolvePermission('sess-1', 7);
    useLiveStore.getState().seedPermission(
      'sess-1',
      { request_id: 7, tool_name: 'Bash', tool_input: '{}' },
      1,
    );
    expect(noticeOf(notices(), 'sess-1', 'permission')).toBeNull();
  });

  it('remembers resolved requests per session, capped to the most recent', () => {
    // Another session's resolution of the same id does not block this one.
    resolvePermission('sess-2', 7);
    useLiveStore.getState().seedPermission(
      'sess-1',
      { request_id: 7, tool_name: 'Bash', tool_input: '{}' },
      1,
    );
    expect(noticeOf(notices(), 'sess-1', 'permission')).toEqual(
      PERMISSION_NOTICE,
    );

    for (let id = 100; id < 100 + RESOLVED_REQUESTS_KEPT + 1; id++) {
      resolvePermission('sess-3', id);
    }
    const kept = useLiveStore.getState().resolvedRequests['sess-3'];
    expect(kept).toHaveLength(RESOLVED_REQUESTS_KEPT);
    // The oldest id went first.
    expect(kept?.[0]).toBe(101);
  });

  it('seedPermission never clears, and never un-dismisses the shown request', () => {
    requestPermission();
    useLiveStore.getState().dismissPermission('sess-1');

    // A report of the SAME request changes nothing — in particular it must
    // not resurrect the card the user just dismissed.
    useLiveStore.getState().seedPermission(
      'sess-1',
      { request_id: 7, tool_name: 'Bash', tool_input: '{}' },
      1,
    );
    expect(noticeOf(notices(), 'sess-1', 'permission')?.dismissed).toBe(true);

    // A null report clears nothing: clearing is owned by the events and the
    // lifecycle sweeps, so a momentarily-stale refetch cannot wipe a notice
    // an event just set.
    useLiveStore.getState().seedPermission('sess-1', null, 0);
    expect(noticeOf(notices(), 'sess-1', 'permission')).not.toBeNull();

    // A DIFFERENT pending request is a new question: it replaces the entry,
    // un-dismissed.
    useLiveStore.getState().seedPermission(
      'sess-1',
      { request_id: 9, tool_name: 'Edit', tool_input: '{}' },
      1,
    );
    expect(noticeOf(notices(), 'sess-1', 'permission')).toEqual({
      kind: 'permission',
      requestId: 9,
      toolName: 'Edit',
      toolInput: '{}',
      dismissed: false,
      queued: [],
      pendingCount: 1,
    });
  });

  it('shows the FIRST of several rapid permission requests, queueing the rest', () => {
    // A parallel tool-call fan-out lands N requests in the same instant. The
    // last writer must NOT win: the card the user is looking at stays put and
    // the newcomers queue behind it, counted.
    requestPermission('sess-1', 11, 'cat a');
    requestPermission('sess-1', 12, 'cat b');
    requestPermission('sess-1', 13, 'cat c');

    expect(noticeOf(notices(), 'sess-1', 'permission')).toEqual({
      kind: 'permission',
      requestId: 11,
      toolName: 'cat a',
      toolInput: '{}',
      dismissed: false,
      queued: [
        { requestId: 12, toolName: 'cat b', toolInput: '{}' },
        { requestId: 13, toolName: 'cat c', toolInput: '{}' },
      ],
      pendingCount: 3,
    });
  });

  it('promotes the next queued request when the shown one resolves', () => {
    requestPermission('sess-1', 11, 'cat a');
    requestPermission('sess-1', 12, 'cat b');

    resolvePermission('sess-1', 11);
    expect(noticeOf(notices(), 'sess-1', 'permission')).toEqual({
      kind: 'permission',
      requestId: 12,
      toolName: 'cat b',
      toolInput: '{}',
      dismissed: false,
      queued: [],
      pendingCount: 1,
    });

    // The server also re-broadcasts the promoted head so an event-only client is
    // never left dialog-less; a client that promoted it itself must treat that as
    // a no-op rather than queueing the shown request behind itself.
    requestPermission('sess-1', 12, 'cat b');
    expect(noticeOf(notices(), 'sess-1', 'permission')).toMatchObject({
      requestId: 12,
      queued: [],
      pendingCount: 1,
    });

    resolvePermission('sess-1', 12);
    expect(noticeOf(notices(), 'sess-1', 'permission')).toBeNull();
  });

  it('drops a resolved QUEUED request without disturbing the shown one', () => {
    // A decision (or a provider withdrawal) can settle a request that is not the
    // head: the endpoint is keyed by row id, not by queue position.
    requestPermission('sess-1', 11, 'cat a');
    requestPermission('sess-1', 12, 'cat b');
    requestPermission('sess-1', 13, 'cat c');

    resolvePermission('sess-1', 12);
    expect(noticeOf(notices(), 'sess-1', 'permission')).toEqual({
      kind: 'permission',
      requestId: 11,
      toolName: 'cat a',
      toolInput: '{}',
      dismissed: false,
      queued: [{ requestId: 13, toolName: 'cat c', toolInput: '{}' }],
      pendingCount: 2,
    });
  });

  it('seedPermission restores the head AND the depth after a reconnect', () => {
    // The reconnect drops the event-built notices; the refetched envelope reports
    // the head plus how many are pending, so the card comes back with its
    // remaining-count indication even though this client never saw the other
    // requests' events.
    useLiveStore.getState().seedPermission(
      'sess-1',
      { request_id: 11, tool_name: 'cat a', tool_input: '{}' },
      3,
    );
    expect(noticeOf(notices(), 'sess-1', 'permission')).toEqual({
      kind: 'permission',
      requestId: 11,
      toolName: 'cat a',
      toolInput: '{}',
      dismissed: false,
      queued: [],
      pendingCount: 3,
    });

    // A later refetch of the same head only corrects the depth, leaving the card
    // (and a dismissal) alone.
    useLiveStore.getState().seedPermission(
      'sess-1',
      { request_id: 11, tool_name: 'cat a', tool_input: '{}' },
      2,
    );
    expect(noticeOf(notices(), 'sess-1', 'permission')?.pendingCount).toBe(2);

    // A report whose head is one this client has queued means its own head
    // resolved during the gap: everything ahead of the reported head is dropped
    // and the rest of the queue survives.
    requestPermission('sess-1', 12, 'cat b');
    requestPermission('sess-1', 13, 'cat c');
    useLiveStore.getState().seedPermission(
      'sess-1',
      { request_id: 12, tool_name: 'cat b', tool_input: '{}' },
      2,
    );
    expect(noticeOf(notices(), 'sess-1', 'permission')).toMatchObject({
      requestId: 12,
      queued: [{ requestId: 13, toolName: 'cat c', toolInput: '{}' }],
      pendingCount: 2,
    });
  });

  it('records an external-input notice keyed by session and clears it on dismiss', () => {
    // The focus guard lives in the router (`applySessionEvent`); the store
    // action just records the notice for whichever session/thread it is given.
    useLiveStore.getState().noteExternalInput('sess-1', 3, 'typed');
    expect(noticeOf(notices(), 'sess-1', 'external_input')).toMatchObject({
      threadId: 3,
      prompt: 'typed',
    });

    useLiveStore.getState().dismissExternalInput('sess-1');
    expect(noticeOf(notices(), 'sess-1', 'external_input')).toBeNull();
  });

  it('keeps external-input notices for different sessions independent', () => {
    const store = useLiveStore.getState();
    store.noteExternalInput('sess-1', 1, 'one');
    store.noteExternalInput('sess-2', 2, 'two');

    // Dismissing one session leaves the other's notice intact.
    useLiveStore.getState().dismissExternalInput('sess-1');
    expect(noticeOf(notices(), 'sess-2', 'external_input')).toMatchObject({
      threadId: 2,
      prompt: 'two',
    });
    expect(noticeOf(notices(), 'sess-1', 'external_input')).toBeNull();
  });

  it('clears a session external-input notice when its turn completes', () => {
    useLiveStore.getState().noteExternalInput('sess-1', 3, 'typed');

    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    // The turn ended, so the external-input notice has served its purpose.
    expect(noticeOf(notices(), 'sess-1', 'external_input')).toBeNull();
  });

  it('leaves a foreign session external-input notice on turn_completed', () => {
    useLiveStore.getState().noteExternalInput('sess-1', 3, 'typed');

    // A turn completing in a different session must not clear sess-1's notice.
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-2',
      thread_id: 1,
      stop_reason: null,
    });
    expect(noticeOf(notices(), 'sess-1', 'external_input')).toMatchObject({
      threadId: 3,
      prompt: 'typed',
    });
  });

  it('clears a session external-input notice when the session closes', () => {
    useLiveStore.getState().noteExternalInput('sess-1', 3, 'typed');

    useLiveStore.getState().applyEvent({
      kind: 'session_closed',
      session_id: 'sess-1',
    });
    expect(noticeOf(notices(), 'sess-1', 'external_input')).toBeNull();
  });

  it('does not set an external-input notice straight from applyEvent', () => {
    // applyEvent is session-scoped only; the external-input notice is focus-
    // dependent and must not be set here for an unfocused background session.
    useLiveStore.getState().applyEvent({
      kind: 'external_input',
      session_id: 'sess-1',
      prompt: 'typed',
    });
    expect(notices()).toEqual({});
  });

  it('surfaces a parked send and keeps the notice past the turn that parked it', () => {
    // The park happens mid-turn, and that turn ends moments later — so a
    // `turn_end` sweep would erase the notice before anyone could read it.
    useLiveStore.getState().applyEvent({
      kind: 'send_parked',
      session_id: 'sess-1',
      send_id: 42,
      text: 'never delivered',
    });
    // The notice records which send was parked; the message itself lives on
    // the held server row, not in this notice.
    expect(noticeOf(notices(), 'sess-1', 'send_parked')).toMatchObject({
      sendId: 42,
    });

    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    expect(noticeOf(notices(), 'sess-1', 'send_parked')).not.toBeNull();

    // It goes away on an explicit dismiss…
    useLiveStore.getState().dismissSendParked('sess-1');
    expect(noticeOf(notices(), 'sess-1', 'send_parked')).toBeNull();
  });

  it('drops the parked send’s tracked twin, which no turn end would ever drain', () => {
    // The echo-deadline watchdog parks a send whose turn NEVER started: no
    // `turn_completed` / `turn_interrupted` can follow, and the server row is
    // already out of the open list. If the park did not drop the local twin
    // here, the chip would spin forever with nothing left to drain it.
    useLiveStore.getState().recordLocalSend(localSend({ sendId: 42 }));

    useLiveStore.getState().applyEvent({
      kind: 'send_parked',
      session_id: 'sess-1',
      send_id: 42,
      text: 'never delivered',
    });

    expect(useLiveStore.getState().localSends).toEqual({});
    expect(noticeOf(notices(), 'sess-1', 'send_parked')).not.toBeNull();
  });

  it('leaves another send’s tracked twin alone when one is parked', () => {
    useLiveStore.getState().recordLocalSend(localSend({ sendId: 7 }));

    useLiveStore.getState().applyEvent({
      kind: 'send_parked',
      session_id: 'sess-1',
      send_id: 42,
      text: 'never delivered',
    });

    expect(Object.keys(useLiveStore.getState().localSends)).toEqual(['7']);
  });

  it('clears the parked-send notice once the released send dispatches', () => {
    // The notice tells the user their message is waiting in the queue for
    // them to send or cancel. A held row has no automatic trigger, so a
    // `send_dispatched` for it can only be that release — the question has
    // been answered and the card must not outlive the state it describes.
    // Driven by the event rather than the button so it also lands for a
    // release done in another tab.
    useLiveStore.getState().applyEvent({
      kind: 'send_parked',
      session_id: 'sess-1',
      send_id: 42,
      text: 'never delivered',
    });

    useLiveStore.getState().applyEvent({
      kind: 'send_dispatched',
      session_id: 'sess-1',
      send_id: 42,
    });
    expect(noticeOf(notices(), 'sess-1', 'send_parked')).toBeNull();
  });

  it('keeps the parked-send notice when a different send dispatches', () => {
    // Ordinary traffic: the queue behind a held row dispatches past it, and
    // the held row is still sitting there waiting for the user.
    useLiveStore.getState().applyEvent({
      kind: 'send_parked',
      session_id: 'sess-1',
      send_id: 42,
      text: 'never delivered',
    });

    useLiveStore.getState().applyEvent({
      kind: 'send_dispatched',
      session_id: 'sess-1',
      send_id: 43,
    });
    expect(noticeOf(notices(), 'sess-1', 'send_parked')).toMatchObject({
      sendId: 42,
    });
  });

  it('forgets the parked-send notice for a cancelled row, and only that row', () => {
    // A cancel is broadcast as nothing at all — the server flips the row to
    // `cancelled` and drops it from the open list — so the strip's Cancel
    // reports it here, the way it already drops the tracked local twin.
    useLiveStore.getState().applyEvent({
      kind: 'send_parked',
      session_id: 'sess-1',
      send_id: 42,
      text: 'never delivered',
    });

    useLiveStore.getState().forgetParkedSend('sess-1', 43);
    expect(noticeOf(notices(), 'sess-1', 'send_parked')).not.toBeNull();

    useLiveStore.getState().forgetParkedSend('sess-1', 42);
    expect(noticeOf(notices(), 'sess-1', 'send_parked')).toBeNull();
  });

  it('clears a parked-send notice when the session closes', () => {
    useLiveStore.getState().applyEvent({
      kind: 'send_parked',
      session_id: 'sess-1',
      send_id: 42,
      text: 'never delivered',
    });

    useLiveStore.getState().applyEvent({
      kind: 'session_closed',
      session_id: 'sess-1',
    });
    expect(noticeOf(notices(), 'sess-1', 'send_parked')).toBeNull();
  });

  it('keeps the resume-unavailable notice across turn ends and clears it on open', () => {
    useLiveStore.getState().markResumeUnavailable('sess-1');
    expect(noticeOf(notices(), 'sess-1', 'resume_unavailable')).not.toBeNull();

    // A resume-impossible session is closed and turn-less; neither sweep that
    // drains the turn-scoped notices may touch it.
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    useLiveStore.getState().applyEvent({
      kind: 'session_closed',
      session_id: 'sess-1',
    });
    expect(noticeOf(notices(), 'sess-1', 'resume_unavailable')).not.toBeNull();

    // Only a successful open proves the flag stale.
    useLiveStore.getState().applyEvent({
      kind: 'session_opened',
      session_id: 'sess-1',
    });
    expect(noticeOf(notices(), 'sess-1', 'resume_unavailable')).toBeNull();
  });

  it('bumps and clears unread counts', () => {
    const store = useLiveStore.getState();
    store.bumpUnread(2);
    store.bumpUnread(2);
    expect(useLiveStore.getState().unread[2]).toBe(2);
    store.clearUnread(2);
    expect(useLiveStore.getState().unread[2]).toBeUndefined();
  });

  it('tracks running per thread and clears only the completed thread', () => {
    // Two sub-threads of one session run concurrently.
    useLiveStore.getState().applyEvent({
      kind: 'turn_started',
      session_id: 'sess-1',
      thread_id: 2,
      send_id: 1,
      matched_uuid: 'u-2',
    });
    useLiveStore.getState().applyEvent({
      kind: 'turn_started',
      session_id: 'sess-1',
      thread_id: 3,
      send_id: 2,
      matched_uuid: 'u-3',
    });
    expect(useLiveStore.getState().runningThreads).toEqual({
      'sess-1': { 2: true, 3: true },
    });

    // One thread completes: only its flag clears; the other keeps running and
    // the session entry survives (so the row still OR-aggregates to running).
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 2,
      stop_reason: null,
    });
    expect(useLiveStore.getState().runningThreads).toEqual({
      'sess-1': { 3: true },
    });

    // The last running thread ends: the now-empty session entry is dropped.
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 3,
      stop_reason: null,
    });
    expect(useLiveStore.getState().runningThreads).toEqual({});
  });

  it('session_closed clears every running thread of the session', () => {
    useLiveStore.getState().applyEvent({
      kind: 'turn_started',
      session_id: 'sess-1',
      thread_id: 2,
      send_id: 1,
      matched_uuid: 'u-2',
    });
    useLiveStore.getState().applyEvent({
      kind: 'turn_started',
      session_id: 'sess-1',
      thread_id: 3,
      send_id: 2,
      matched_uuid: 'u-3',
    });
    useLiveStore.getState().applyEvent({
      kind: 'session_closed',
      session_id: 'sess-1',
    });
    expect(useLiveStore.getState().runningThreads).toEqual({});
  });

  it('ignores session lifecycle events for ephemeral state', () => {
    // session_registered / session_opened are reflected by the sessions query,
    // not the live store, so applying them is a no-op here.
    const before = useLiveStore.getState().spawns;
    useLiveStore.getState().applyEvent({
      kind: 'session_opened',
      session_id: 'sess-1',
    });
    useLiveStore.getState().applyEvent({
      kind: 'session_registered',
      session_id: 'sess-1',
    });
    expect(useLiveStore.getState().spawns).toBe(before);
  });
});
