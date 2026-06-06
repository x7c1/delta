import { beforeEach, describe, expect, it } from 'vitest';
import { useLiveStore } from './liveStore';

function reset() {
  useLiveStore.setState({
    connection: 'connecting',
    pending: [],
    permission: null,
    unread: {},
    externalInput: null,
  });
}

describe('liveStore.applyEvent', () => {
  beforeEach(reset);

  it('walks a queued send through in-progress to done over the FIFO', () => {
    const store = useLiveStore.getState();
    store.enqueueSend({
      localId: 'l1',
      sendId: 1,
      threadId: 1,
      text: 'hi',
      semanticParentUuid: null,
      status: 'queued',
      createdAt: 0,
    });

    useLiveStore.getState().applyEvent(
      {
        kind: 'turn_started',
        session_id: 'sess-1',
        pending_send_id: 1,
        matched_uuid: 'uuid-1',
      },
      1,
    );
    expect(useLiveStore.getState().pending[0].status).toBe('in_progress');

    useLiveStore.getState().applyEvent(
      { kind: 'turn_completed', session_id: 'sess-1', stop_reason: null },
      1,
    );
    expect(useLiveStore.getState().pending).toHaveLength(0);
  });

  it('drops a still-queued send on turn_completed when turn_started never fired', () => {
    const store = useLiveStore.getState();
    store.enqueueSend({
      localId: 'l1',
      sendId: 1,
      threadId: 1,
      text: 'これはテスト送信です',
      semanticParentUuid: null,
      status: 'queued',
      createdAt: 0,
    });

    // No turn_started (the common timing case: the user line was not ingested
    // in the UserPromptSubmit sync). The completed turn must still clear it.
    useLiveStore.getState().applyEvent(
      { kind: 'turn_completed', session_id: 'sess-1', stop_reason: null },
      1,
    );
    expect(useLiveStore.getState().pending).toHaveLength(0);
  });

  it('records a permission notice and clears it on dismiss', () => {
    useLiveStore.getState().applyEvent(
      {
        kind: 'permission_requested',
        session_id: 'sess-1',
        request_id: 7,
        tool_name: 'Bash',
      },
      1,
    );
    expect(useLiveStore.getState().permission).toEqual({
      requestId: 7,
      toolName: 'Bash',
    });

    useLiveStore.getState().dismissPermission();
    expect(useLiveStore.getState().permission).toBeNull();
  });

  it('marks external input on the active thread', () => {
    useLiveStore.getState().applyEvent(
      { kind: 'external_input', session_id: 'sess-1', prompt: 'typed' },
      3,
    );
    expect(useLiveStore.getState().externalInput).toMatchObject({
      threadId: 3,
      prompt: 'typed',
    });
  });

  it('bumps and clears unread counts', () => {
    const store = useLiveStore.getState();
    store.bumpUnread(2);
    store.bumpUnread(2);
    expect(useLiveStore.getState().unread[2]).toBe(2);
    store.clearUnread(2);
    expect(useLiveStore.getState().unread[2]).toBeUndefined();
  });
});
