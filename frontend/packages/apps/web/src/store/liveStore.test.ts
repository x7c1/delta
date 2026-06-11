import { beforeEach, describe, expect, it } from 'vitest';
import { useLiveStore } from './liveStore';
import { NEW_SESSION_DRAFT_KEY } from './composerStore';

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

  it('clears the in-flight pending send on turn_interrupted', () => {
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

    // The turn starts, then the user interrupts it. The `Stop` hook never fires
    // on interrupt, so `turn_interrupted` must drain the stuck pending chip.
    useLiveStore.getState().applyEvent({
      kind: 'turn_started',
      session_id: 'sess-1',
      pending_send_id: 1,
      matched_uuid: 'uuid-1',
    });
    expect(useLiveStore.getState().pending[0].status).toBe('in_progress');

    useLiveStore.getState().applyEvent({
      kind: 'turn_interrupted',
      session_id: 'sess-1',
    });
    expect(useLiveStore.getState().pending).toHaveLength(0);
  });

  it('drops a still-queued send on turn_interrupted when turn_started never fired', () => {
    const store = useLiveStore.getState();
    store.enqueueSend({
      localId: 'l1',
      sendId: 1,
      sessionId: 'sess-1',
      threadId: 1,
      text: 'interrupted before start',
      semanticParentUuid: null,
      status: 'queued',
      createdAt: 0,
    });

    useLiveStore.getState().applyEvent({
      kind: 'turn_interrupted',
      session_id: 'sess-1',
    });
    expect(useLiveStore.getState().pending).toHaveLength(0);
  });

  it('does not drain another session queue on a foreign turn_interrupted', () => {
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

    useLiveStore.getState().applyEvent({
      kind: 'turn_interrupted',
      session_id: 'sess-2',
    });
    expect(useLiveStore.getState().pending).toHaveLength(1);
    expect(useLiveStore.getState().pending[0].localId).toBe('a1');
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

  it('binds the unbound new-session send to its spawned session and main thread', () => {
    const store = useLiveStore.getState();
    store.enqueueSend({
      localId: 'l1',
      sendId: 0, // placeholder id from the new-session POST response
      sessionId: null, // unbound: the spawn has no real session id yet
      threadId: NEW_SESSION_DRAFT_KEY, // enqueued under the new-session sentinel
      text: 'first message',
      semanticParentUuid: null,
      status: 'queued',
      createdAt: 0,
    });

    // The spawn registered as sess-9 with main thread 42; bind the pending to it
    // so the optimistic strip survives the focus jump to the real thread.
    useLiveStore.getState().bindNewSessionPending('sess-9', 42);

    const item = useLiveStore.getState().pending[0];
    expect(item.sessionId).toBe('sess-9');
    expect(item.threadId).toBe(42);
    expect(item.status).toBe('queued');
    expect(item.text).toBe('first message');
  });

  it('keeps the bound new-session send drainable on its first turn_completed', () => {
    const store = useLiveStore.getState();
    store.enqueueSend({
      localId: 'l1',
      sendId: 0,
      sessionId: null,
      threadId: NEW_SESSION_DRAFT_KEY,
      text: 'first message',
      semanticParentUuid: null,
      status: 'in_progress',
      createdAt: 0,
    });
    useLiveStore.getState().bindNewSessionPending('sess-9', 42);

    // The first turn finishes: the now-bound send drains by exact session match.
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-9',
      stop_reason: null,
    });
    expect(useLiveStore.getState().pending).toHaveLength(0);
  });

  it('leaves the queue untouched when no unbound new-session send exists', () => {
    const store = useLiveStore.getState();
    store.enqueueSend({
      localId: 'l1',
      sendId: 1,
      sessionId: 'sess-1', // already bound
      threadId: 1,
      text: 'bound send',
      semanticParentUuid: null,
      status: 'queued',
      createdAt: 0,
    });

    useLiveStore.getState().bindNewSessionPending('sess-9', 42);

    const item = useLiveStore.getState().pending[0];
    expect(item.sessionId).toBe('sess-1');
    expect(item.threadId).toBe(1);
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

  it('clears a session permission notice when its request resolves', () => {
    useLiveStore.getState().applyEvent({
      kind: 'permission_requested',
      session_id: 'sess-1',
      request_id: 7,
      tool_name: 'Bash',
    });

    useLiveStore.getState().applyEvent({
      kind: 'permission_resolved',
      session_id: 'sess-1',
      request_id: 7,
    });
    // The correlated tool_result was ingested, so the notice is cleared.
    expect(useLiveStore.getState().permission).toEqual({});
  });

  it('ignores a resolution for a different request than the current notice', () => {
    useLiveStore.getState().applyEvent({
      kind: 'permission_requested',
      session_id: 'sess-1',
      request_id: 8,
      tool_name: 'Bash',
    });

    // A stale resolution for an older request must not wipe the live notice.
    useLiveStore.getState().applyEvent({
      kind: 'permission_resolved',
      session_id: 'sess-1',
      request_id: 7,
    });
    expect(useLiveStore.getState().permission).toEqual({
      'sess-1': { requestId: 8, toolName: 'Bash' },
    });
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

  it('marks the oldest unbound new-session pending as failed on failSpawn', () => {
    const store = useLiveStore.getState();
    // A bound (real-session) pending must be untouched; only the unbound
    // new-session pending correlates to the failed spawn.
    store.enqueueSend({
      localId: 'bound',
      sendId: 1,
      sessionId: 'sess-1',
      threadId: 1,
      text: 'in a real session',
      semanticParentUuid: null,
      status: 'queued',
      createdAt: 0,
    });
    store.enqueueSend({
      localId: 'spawn',
      sendId: 2,
      sessionId: null,
      threadId: NEW_SESSION_DRAFT_KEY,
      text: 'start a new session',
      semanticParentUuid: null,
      workdir: '/work/dir',
      status: 'queued',
      createdAt: 1,
    });

    useLiveStore.getState().failSpawn();

    const pending = useLiveStore.getState().pending;
    expect(pending.find((item) => item.localId === 'spawn')?.status).toBe(
      'failed',
    );
    expect(pending.find((item) => item.localId === 'bound')?.status).toBe(
      'queued',
    );
  });

  it('marks the oldest unbound new-session pending as failed on a spawn_failed event', () => {
    const store = useLiveStore.getState();
    store.enqueueSend({
      localId: 'spawn',
      sendId: 0,
      sessionId: null,
      threadId: NEW_SESSION_DRAFT_KEY,
      text: 'start a new session',
      semanticParentUuid: null,
      workdir: null,
      status: 'queued',
      createdAt: 0,
    });

    // The router (`applySessionEvent`) calls `failSpawn` for a spawn_failed
    // event; assert the action it routes to produces the failed chip.
    useLiveStore.getState().failSpawn();

    expect(useLiveStore.getState().pending[0].status).toBe('failed');
  });

  it('does not drain a failed spawn pending on turn_completed or turn_interrupted', () => {
    const store = useLiveStore.getState();
    store.enqueueSend({
      localId: 'spawn',
      sendId: 0,
      sessionId: null,
      threadId: NEW_SESSION_DRAFT_KEY,
      text: 'start a new session',
      semanticParentUuid: null,
      workdir: null,
      status: 'queued',
      createdAt: 0,
    });
    useLiveStore.getState().failSpawn();

    // A failed chip is terminal: an unrelated turn ending must not silently
    // remove it. It survives until the user retries or dismisses it.
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      stop_reason: null,
    });
    useLiveStore.getState().applyEvent({
      kind: 'turn_interrupted',
      session_id: 'sess-1',
    });

    const pending = useLiveStore.getState().pending;
    expect(pending).toHaveLength(1);
    expect(pending[0].status).toBe('failed');
  });

  it('does not bind a failed spawn pending to a later registered session', () => {
    const store = useLiveStore.getState();
    store.enqueueSend({
      localId: 'spawn',
      sendId: 0,
      sessionId: null,
      threadId: NEW_SESSION_DRAFT_KEY,
      text: 'start a new session',
      semanticParentUuid: null,
      workdir: null,
      status: 'queued',
      createdAt: 0,
    });
    useLiveStore.getState().failSpawn();

    // A subsequent successful spawn must not resurrect the failed chip onto a
    // real session; with no live unbound pending, binding is a no-op.
    useLiveStore.getState().bindNewSessionPending('sess-9', 99);

    const item = useLiveStore.getState().pending[0];
    expect(item.status).toBe('failed');
    expect(item.sessionId).toBeNull();
  });

  it('marks a second unbound new-session pending failed, skipping the already-failed one', () => {
    const store = useLiveStore.getState();
    store.enqueueSend({
      localId: 'spawn-a',
      sendId: 0,
      sessionId: null,
      threadId: NEW_SESSION_DRAFT_KEY,
      text: 'first',
      semanticParentUuid: null,
      workdir: null,
      status: 'queued',
      createdAt: 0,
    });
    store.enqueueSend({
      localId: 'spawn-b',
      sendId: 0,
      sessionId: null,
      threadId: NEW_SESSION_DRAFT_KEY,
      text: 'second',
      semanticParentUuid: null,
      workdir: null,
      status: 'queued',
      createdAt: 1,
    });

    useLiveStore.getState().failSpawn();
    useLiveStore.getState().failSpawn();

    const pending = useLiveStore.getState().pending;
    expect(pending.find((item) => item.localId === 'spawn-a')?.status).toBe(
      'failed',
    );
    expect(pending.find((item) => item.localId === 'spawn-b')?.status).toBe(
      'failed',
    );
  });
});
