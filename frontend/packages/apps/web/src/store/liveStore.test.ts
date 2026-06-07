import { beforeEach, describe, expect, it } from 'vitest';
import { useLiveStore } from './liveStore';

function reset() {
  useLiveStore.setState({
    connection: 'connecting',
    pending: [],
    permission: null,
    unread: {},
    externalInput: null,
    resuming: {},
  });
}

describe('liveStore.applyEvent', () => {
  beforeEach(reset);

  it('walks a queued send through in-progress to done over the FIFO', () => {
    const store = useLiveStore.getState();
    store.enqueueSend({
      localId: 'l1',
      sendId: 1,
      sessionId: 'sess-1',
      threadId: 1,
      text: 'hi',
      semanticParentUuid: null,
      status: 'queued',
      createdAt: 0,
    });

    useLiveStore.getState().applyEvent({
      kind: 'turn_started',
      session_id: 'sess-1',
      pending_send_id: 1,
      matched_uuid: 'uuid-1',
    });
    expect(useLiveStore.getState().pending[0].status).toBe('in_progress');

    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      stop_reason: null,
    });
    expect(useLiveStore.getState().pending).toHaveLength(0);
  });

  it('drops a still-queued send on turn_completed when turn_started never fired', () => {
    const store = useLiveStore.getState();
    store.enqueueSend({
      localId: 'l1',
      sendId: 1,
      sessionId: 'sess-1',
      threadId: 1,
      text: 'これはテスト送信です',
      semanticParentUuid: null,
      status: 'queued',
      createdAt: 0,
    });

    // No turn_started (the common timing case: the user line was not ingested
    // in the UserPromptSubmit sync). The completed turn must still clear it.
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      stop_reason: null,
    });
    expect(useLiveStore.getState().pending).toHaveLength(0);
  });

  it('does not drain another session queue on a foreign turn_completed', () => {
    const store = useLiveStore.getState();
    store.enqueueSend({
      localId: 'a1',
      sendId: 1,
      sessionId: 'sess-1',
      threadId: 1,
      text: 'session one send',
      semanticParentUuid: null,
      status: 'queued',
      createdAt: 0,
    });

    // A turn completes in a DIFFERENT session that has no pending send here.
    // It must leave sess-1's queued item untouched (regression: the matcher
    // used to drop the first queued item regardless of session).
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-2',
      stop_reason: null,
    });
    expect(useLiveStore.getState().pending).toHaveLength(1);
    expect(useLiveStore.getState().pending[0].localId).toBe('a1');

    // The turn for the OWNING session still clears it.
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      stop_reason: null,
    });
    expect(useLiveStore.getState().pending).toHaveLength(0);
  });

  it('reconciles an unbound new-session send via its turn, not a foreign one', () => {
    const store = useLiveStore.getState();
    // An optimistic new-session send: no bound session id yet.
    store.enqueueSend({
      localId: 'new1',
      sendId: 0,
      sessionId: null,
      threadId: -1,
      text: 'start fresh',
      semanticParentUuid: null,
      status: 'queued',
      createdAt: 0,
    });
    // A bound send for an existing session sits in front of it.
    store.enqueueSend({
      localId: 'bound1',
      sendId: 5,
      sessionId: 'sess-1',
      threadId: 1,
      text: 'existing',
      semanticParentUuid: null,
      status: 'queued',
      createdAt: 1,
    });

    // sess-1's turn must clear ITS item, not the unbound new-session item that
    // happens to be older in FIFO order.
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      stop_reason: null,
    });
    const pending = useLiveStore.getState().pending;
    expect(pending).toHaveLength(1);
    expect(pending[0].localId).toBe('new1');

    // The newly-registered session's turn then clears the unbound item.
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-new',
      stop_reason: null,
    });
    expect(useLiveStore.getState().pending).toHaveLength(0);
  });

  it('records a permission notice and clears it on dismiss', () => {
    useLiveStore.getState().applyEvent({
      kind: 'permission_requested',
      session_id: 'sess-1',
      request_id: 7,
      tool_name: 'Bash',
    });
    expect(useLiveStore.getState().permission).toEqual({
      requestId: 7,
      toolName: 'Bash',
    });

    useLiveStore.getState().dismissPermission();
    expect(useLiveStore.getState().permission).toBeNull();
  });

  it('records an external-input marker on a thread', () => {
    // The focus guard lives in the router (`applySessionEvent`); the store
    // action just records the marker for whichever thread it is given.
    useLiveStore.getState().noteExternalInput(3, 'typed');
    expect(useLiveStore.getState().externalInput).toMatchObject({
      threadId: 3,
      prompt: 'typed',
    });
  });

  it('does not set an external-input marker straight from applyEvent', () => {
    // applyEvent is session-scoped only; the external-input marker is focus-
    // dependent and must not be set here for an unfocused background session.
    useLiveStore.getState().applyEvent({
      kind: 'external_input',
      session_id: 'sess-1',
      prompt: 'typed',
    });
    expect(useLiveStore.getState().externalInput).toBeNull();
  });

  it('bumps and clears unread counts', () => {
    const store = useLiveStore.getState();
    store.bumpUnread(2);
    store.bumpUnread(2);
    expect(useLiveStore.getState().unread[2]).toBe(2);
    store.clearUnread(2);
    expect(useLiveStore.getState().unread[2]).toBeUndefined();
  });

  it('marks a session resuming and clears it when the session opens', () => {
    useLiveStore.getState().markResuming('sess-1');
    expect(useLiveStore.getState().resuming['sess-1']).toBe(true);

    useLiveStore.getState().applyEvent({
      kind: 'session_opened',
      session_id: 'sess-1',
    });
    expect(useLiveStore.getState().resuming['sess-1']).toBeUndefined();
  });

  it('clears a resuming marker directly (failed-resume path)', () => {
    useLiveStore.getState().markResuming('sess-3');
    expect(useLiveStore.getState().resuming['sess-3']).toBe(true);
    useLiveStore.getState().clearResuming('sess-3');
    expect(useLiveStore.getState().resuming['sess-3']).toBeUndefined();
    // Clearing an unmarked session is a no-op, not a crash.
    useLiveStore.getState().clearResuming('never-marked');
    expect(useLiveStore.getState().resuming['never-marked']).toBeUndefined();
  });

  it('clears a resuming marker on session_registered too', () => {
    useLiveStore.getState().markResuming('sess-2');
    useLiveStore.getState().applyEvent({
      kind: 'session_registered',
      session_id: 'sess-2',
    });
    expect(useLiveStore.getState().resuming['sess-2']).toBeUndefined();
  });
});
