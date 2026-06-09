import { beforeEach, describe, expect, it } from 'vitest';
import { useLiveStore } from './liveStore';

function reset() {
  useLiveStore.setState({
    connection: 'connecting',
    pending: [],
    permission: {},
    unread: {},
    externalInput: {},
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

  it('retargets a pending send to a new thread, keeping it queued', () => {
    const store = useLiveStore.getState();
    store.enqueueSend({
      localId: 'l1',
      sendId: 1,
      sessionId: 'sess-1',
      threadId: 1, // enqueued under the parent thread
      text: 'branch follow-up',
      semanticParentUuid: 'uuid-origin',
      status: 'queued',
      createdAt: 0,
    });

    // The branch send created child thread 7; move the pending entry onto it.
    useLiveStore.getState().retargetSend('l1', 7);

    const item = useLiveStore.getState().pending[0];
    expect(item.threadId).toBe(7);
    expect(item.status).toBe('queued');
    expect(item.text).toBe('branch follow-up');
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

  it('records a permission notice per session and clears it on dismiss', () => {
    useLiveStore.getState().applyEvent({
      kind: 'permission_requested',
      session_id: 'sess-1',
      request_id: 7,
      tool_name: 'Bash',
    });
    expect(useLiveStore.getState().permission).toEqual({
      'sess-1': { requestId: 7, toolName: 'Bash' },
    });

    useLiveStore.getState().dismissPermission('sess-1');
    expect(useLiveStore.getState().permission).toEqual({});
  });

  it('keeps permission notices for different sessions independent', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'permission_requested',
      session_id: 'sess-1',
      request_id: 1,
      tool_name: 'Bash',
    });
    store.applyEvent({
      kind: 'permission_requested',
      session_id: 'sess-2',
      request_id: 2,
      tool_name: 'Edit',
    });

    // Dismissing one session leaves the other's notice intact.
    useLiveStore.getState().dismissPermission('sess-1');
    expect(useLiveStore.getState().permission).toEqual({
      'sess-2': { requestId: 2, toolName: 'Edit' },
    });
  });

  it('clears a session permission notice when its turn completes', () => {
    useLiveStore.getState().applyEvent({
      kind: 'permission_requested',
      session_id: 'sess-1',
      request_id: 7,
      tool_name: 'Bash',
    });

    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      stop_reason: null,
    });
    // The turn ended, so the prompt that was blocking the session is resolved.
    expect(useLiveStore.getState().permission).toEqual({});
  });

  it('clears a session permission notice when the session closes', () => {
    useLiveStore.getState().applyEvent({
      kind: 'permission_requested',
      session_id: 'sess-1',
      request_id: 7,
      tool_name: 'Bash',
    });

    useLiveStore.getState().applyEvent({
      kind: 'session_closed',
      session_id: 'sess-1',
    });
    expect(useLiveStore.getState().permission).toEqual({});
  });

  it('records an external-input marker keyed by session and clears it on dismiss', () => {
    // The focus guard lives in the router (`applySessionEvent`); the store
    // action just records the marker for whichever session/thread it is given.
    useLiveStore.getState().noteExternalInput('sess-1', 3, 'typed');
    expect(useLiveStore.getState().externalInput['sess-1']).toMatchObject({
      threadId: 3,
      prompt: 'typed',
    });

    useLiveStore.getState().dismissExternalInput('sess-1');
    expect(useLiveStore.getState().externalInput).toEqual({});
  });

  it('keeps external-input markers for different sessions independent', () => {
    const store = useLiveStore.getState();
    store.noteExternalInput('sess-1', 1, 'one');
    store.noteExternalInput('sess-2', 2, 'two');

    // Dismissing one session leaves the other's marker intact.
    useLiveStore.getState().dismissExternalInput('sess-1');
    expect(useLiveStore.getState().externalInput).toMatchObject({
      'sess-2': { threadId: 2, prompt: 'two' },
    });
    expect(useLiveStore.getState().externalInput['sess-1']).toBeUndefined();
  });

  it('clears a session external-input marker when its turn completes', () => {
    useLiveStore.getState().noteExternalInput('sess-1', 3, 'typed');

    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      stop_reason: null,
    });
    // The turn ended, so the external-input notice has served its purpose.
    expect(useLiveStore.getState().externalInput).toEqual({});
  });

  it('leaves a foreign session external-input marker on turn_completed', () => {
    useLiveStore.getState().noteExternalInput('sess-1', 3, 'typed');

    // A turn completing in a different session must not clear sess-1's marker.
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-2',
      stop_reason: null,
    });
    expect(useLiveStore.getState().externalInput['sess-1']).toMatchObject({
      threadId: 3,
      prompt: 'typed',
    });
  });

  it('clears a session external-input marker when the session closes', () => {
    useLiveStore.getState().noteExternalInput('sess-1', 3, 'typed');

    useLiveStore.getState().applyEvent({
      kind: 'session_closed',
      session_id: 'sess-1',
    });
    expect(useLiveStore.getState().externalInput).toEqual({});
  });

  it('does not set an external-input marker straight from applyEvent', () => {
    // applyEvent is session-scoped only; the external-input marker is focus-
    // dependent and must not be set here for an unfocused background session.
    useLiveStore.getState().applyEvent({
      kind: 'external_input',
      session_id: 'sess-1',
      prompt: 'typed',
    });
    expect(useLiveStore.getState().externalInput).toEqual({});
  });

  it('bumps and clears unread counts', () => {
    const store = useLiveStore.getState();
    store.bumpUnread(2);
    store.bumpUnread(2);
    expect(useLiveStore.getState().unread[2]).toBe(2);
    store.clearUnread(2);
    expect(useLiveStore.getState().unread[2]).toBeUndefined();
  });

  it('ignores session lifecycle events for ephemeral state', () => {
    // session_registered / session_opened / session_closed are reflected by the
    // sessions query, not the live store, so applying them is a no-op here.
    const before = useLiveStore.getState().pending;
    useLiveStore.getState().applyEvent({
      kind: 'session_opened',
      session_id: 'sess-1',
    });
    expect(useLiveStore.getState().pending).toBe(before);
  });
});
