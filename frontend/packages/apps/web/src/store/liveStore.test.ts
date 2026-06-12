import { beforeEach, describe, expect, it } from 'vitest';
import { useLiveStore, type LocalSend } from './liveStore';

function reset() {
  useLiveStore.setState({
    connection: 'connecting',
    sending: [],
    localSends: {},
    spawns: [],
    activeTurns: {},
    permission: {},
    unread: {},
    externalInput: {},
    resumeUnavailable: {},
    earlySpawnFailures: {},
  });
}

function localSend(overrides: Partial<LocalSend> = {}): LocalSend {
  return {
    sendId: 1,
    sessionId: 'sess-1',
    threadId: 1,
    text: 'hi',
    createdAt: 0,
    ...overrides,
  };
}

describe('liveStore turn tracking', () => {
  beforeEach(reset);

  it('flags the session running on turn_started and clears it on turn_completed', () => {
    useLiveStore.getState().applyEvent({
      kind: 'turn_started',
      session_id: 'sess-1',
      send_id: 1,
      matched_uuid: 'uuid-1',
    });
    expect(useLiveStore.getState().activeTurns).toEqual({ 'sess-1': true });

    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      stop_reason: null,
    });
    expect(useLiveStore.getState().activeTurns).toEqual({});
  });

  it('clears the running flag on turn_interrupted', () => {
    useLiveStore.getState().applyEvent({
      kind: 'turn_started',
      session_id: 'sess-1',
      send_id: 1,
      matched_uuid: 'uuid-1',
    });

    useLiveStore.getState().applyEvent({
      kind: 'turn_interrupted',
      session_id: 'sess-1',
    });
    expect(useLiveStore.getState().activeTurns).toEqual({});
  });

  it('drains the tracked local send when its turn completes', () => {
    useLiveStore.getState().recordLocalSend(localSend());

    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      stop_reason: null,
    });
    expect(useLiveStore.getState().localSends).toEqual({});
  });

  it('drains the tracked local send on turn_interrupted, even without turn_started', () => {
    // The common timing case: the user line was not ingested in the
    // UserPromptSubmit sync, so turn_started never fired. The turn-end event
    // must still clear the tracked send.
    useLiveStore.getState().recordLocalSend(localSend());

    useLiveStore.getState().applyEvent({
      kind: 'turn_interrupted',
      session_id: 'sess-1',
    });
    expect(useLiveStore.getState().localSends).toEqual({});
  });

  it('does not drain another session’s tracked sends on a foreign turn end', () => {
    useLiveStore.getState().recordLocalSend(localSend());

    // A turn ending in a DIFFERENT session must leave sess-1's send untouched
    // (regression: the FIFO matcher used to drop the first item regardless of
    // session).
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-2',
      stop_reason: null,
    });
    useLiveStore.getState().applyEvent({
      kind: 'turn_interrupted',
      session_id: 'sess-2',
    });
    expect(Object.keys(useLiveStore.getState().localSends)).toHaveLength(1);

    // The turn for the OWNING session still clears it.
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      stop_reason: null,
    });
    expect(useLiveStore.getState().localSends).toEqual({});
  });

  it('drains turn state when the session closes', () => {
    useLiveStore.getState().recordLocalSend(localSend());
    useLiveStore.getState().applyEvent({
      kind: 'turn_started',
      session_id: 'sess-1',
      send_id: 1,
      matched_uuid: 'uuid-1',
    });

    useLiveStore.getState().applyEvent({
      kind: 'session_closed',
      session_id: 'sess-1',
    });
    expect(useLiveStore.getState().localSends).toEqual({});
    expect(useLiveStore.getState().activeTurns).toEqual({});
  });

  it('resetTurnEphemera drops tracked sends and running flags, nothing else', () => {
    useLiveStore.getState().recordLocalSend(localSend());
    useLiveStore.getState().applyEvent({
      kind: 'turn_started',
      session_id: 'sess-1',
      send_id: 1,
      matched_uuid: 'uuid-1',
    });
    useLiveStore.getState().beginSending({
      id: 'l1',
      target: { kind: 'thread', sessionId: 'sess-1', threadId: 1 },
      text: 'in flight',
      status: 'sending',
      createdAt: 0,
    });
    useLiveStore.getState().trackSpawn({
      sessionId: 'sess-9',
      threadId: 42,
      text: 'first message',
      workdir: null,
    });

    // A reconnect cannot reconcile event-reconstructed turn state, but the
    // in-flight POST and the tracked spawn heal on their own (the POST
    // resolves; the spawn registers via the refetched session list).
    useLiveStore.getState().resetTurnEphemera();

    expect(useLiveStore.getState().localSends).toEqual({});
    expect(useLiveStore.getState().activeTurns).toEqual({});
    expect(useLiveStore.getState().sending).toHaveLength(1);
    expect(useLiveStore.getState().spawns).toHaveLength(1);
  });

  it('seedActiveTurn sets the running flag from a non-idle turn, and only sets', () => {
    // After a reconnect the refetched sends envelope reports the turn state;
    // a non-idle phase re-seeds the flag the dropped events would have set.
    useLiveStore
      .getState()
      .seedActiveTurn('sess-1', { state: 'in_flight', send_id: 1 });
    expect(useLiveStore.getState().activeTurns).toEqual({ 'sess-1': true });

    // An idle report never clears: turn-end events own clearing, so a
    // momentarily-stale refetch cannot wipe a flag an event just set.
    useLiveStore
      .getState()
      .seedActiveTurn('sess-1', { state: 'idle', send_id: null });
    expect(useLiveStore.getState().activeTurns).toEqual({ 'sess-1': true });
  });

  it('seedActiveTurn ignores awaiting_echo: the turn has not started yet', () => {
    // A dispatched-but-unechoed send is what `send_dispatched` reports live —
    // it never set the running flag, so the refetch must not either.
    useLiveStore
      .getState()
      .seedActiveTurn('sess-1', { state: 'awaiting_echo', send_id: 2 });
    expect(useLiveStore.getState().activeTurns).toEqual({});
  });
});

describe('liveStore sending (pre-acceptance submits)', () => {
  beforeEach(reset);

  it('walks a submit through sending → failed → dismissed', () => {
    useLiveStore.getState().beginSending({
      id: 'l1',
      target: { kind: 'thread', sessionId: 'sess-1', threadId: 1 },
      text: 'hello',
      status: 'sending',
      createdAt: 0,
    });
    expect(useLiveStore.getState().sending[0].status).toBe('sending');

    useLiveStore.getState().failSending('l1');
    expect(useLiveStore.getState().sending[0].status).toBe('failed');

    useLiveStore.getState().removeSending('l1');
    expect(useLiveStore.getState().sending).toHaveLength(0);
  });
});

describe('liveStore spawn tracking', () => {
  beforeEach(reset);

  function trackOne(sessionId = 'sess-spawn-1') {
    useLiveStore.getState().trackSpawn({
      sessionId,
      threadId: 42,
      text: 'start a new session',
      workdir: '/work/dir',
    });
  }

  it('flips the tracked spawn to failed on its spawn_failed event', () => {
    trackOne();
    useLiveStore.getState().recordLocalSend(
      localSend({ sendId: 7, sessionId: 'sess-spawn-1', threadId: 42 }),
    );

    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
    });

    const spawn = useLiveStore.getState().spawns[0];
    expect(spawn.status).toBe('failed');
    expect(spawn.text).toBe('start a new session');
    expect(spawn.workdir).toBe('/work/dir');
    // The first send's turn will never end; its tracked twin goes with it.
    expect(useLiveStore.getState().localSends).toEqual({});
  });

  it('leaves tracked spawns alone on spawn_failed for an unknown session', () => {
    trackOne();

    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      session_id: 'sess-someone-elses',
      pane_token: 'pane-x',
    });
    expect(useLiveStore.getState().spawns[0].status).toBe('spawning');
  });

  it('fails a spawn whose spawn_failed event outran its POST response', () => {
    // The live channel and the POST response are independent: the watchdog's
    // broadcast can land before this client processes the response that
    // carries the spawn's ids. The failure must be buffered, not dropped, or
    // the chip spins forever (regression: it was dropped).
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
    });
    expect(useLiveStore.getState().spawns).toHaveLength(0);

    // The POST resolves: the send is tracked, then the spawn — which lands
    // already failed, with its Retry payload intact and the doomed local send
    // dropped.
    useLiveStore.getState().recordLocalSend(
      localSend({ sendId: 7, sessionId: 'sess-spawn-1', threadId: 42 }),
    );
    trackOne();

    const spawn = useLiveStore.getState().spawns[0];
    expect(spawn.status).toBe('failed');
    expect(spawn.text).toBe('start a new session');
    expect(spawn.workdir).toBe('/work/dir');
    expect(useLiveStore.getState().localSends).toEqual({});
    expect(useLiveStore.getState().earlySpawnFailures).toEqual({});
  });

  it('drops a buffered early failure once that session registers', () => {
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      session_id: 'sess-foreign',
      pane_token: 'pane-x',
    });
    expect(useLiveStore.getState().earlySpawnFailures).toEqual({
      'sess-foreign': true,
    });

    useLiveStore.getState().applyEvent({
      kind: 'session_registered',
      session_id: 'sess-foreign',
    });
    expect(useLiveStore.getState().earlySpawnFailures).toEqual({});

    // A later spawn tracked for a clean id starts `spawning` as usual.
    trackOne('sess-foreign');
    expect(useLiveStore.getState().spawns[0].status).toBe('spawning');
  });

  it('does not buffer a duplicate spawn_failed for an already-failed spawn', () => {
    trackOne();
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
    });
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
    });
    expect(useLiveStore.getState().spawns[0].status).toBe('failed');
    expect(useLiveStore.getState().earlySpawnFailures).toEqual({});
  });

  it('keeps a failed spawn through unrelated turn events until dismissed', () => {
    trackOne();
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
    });

    // A failed chip is terminal: an unrelated turn ending must not silently
    // remove it. It survives until the user retries or dismisses it.
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      stop_reason: null,
    });
    expect(useLiveStore.getState().spawns[0].status).toBe('failed');

    useLiveStore.getState().clearSpawn('sess-spawn-1');
    expect(useLiveStore.getState().spawns).toHaveLength(0);
  });

  it('fails each spawn by its own id, leaving the others alone', () => {
    trackOne('sess-spawn-1');
    trackOne('sess-spawn-2');

    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      session_id: 'sess-spawn-2',
      pane_token: 'pane-2',
    });

    const byId = Object.fromEntries(
      useLiveStore.getState().spawns.map((spawn) => [spawn.sessionId, spawn]),
    );
    expect(byId['sess-spawn-1'].status).toBe('spawning');
    expect(byId['sess-spawn-2'].status).toBe('failed');
  });
});

describe('liveStore.applyEvent notices', () => {
  beforeEach(reset);

  it('records a permission notice per session and clears it on dismiss', () => {
    useLiveStore.getState().applyEvent({
      kind: 'permission_requested',
      session_id: 'sess-1',
      request_id: 7,
      tool_name: 'Bash',
      tool_input: '{}',
    });
    expect(useLiveStore.getState().permission).toEqual({
      'sess-1': { requestId: 7, toolName: 'Bash', toolInput: '{}' },
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
      tool_input: '{}',
    });
    store.applyEvent({
      kind: 'permission_requested',
      session_id: 'sess-2',
      request_id: 2,
      tool_name: 'Edit',
      tool_input: '{}',
    });

    // Dismissing one session leaves the other's notice intact.
    useLiveStore.getState().dismissPermission('sess-1');
    expect(useLiveStore.getState().permission).toEqual({
      'sess-2': { requestId: 2, toolName: 'Edit', toolInput: '{}' },
    });
  });

  it('clears a session permission notice when its turn completes', () => {
    useLiveStore.getState().applyEvent({
      kind: 'permission_requested',
      session_id: 'sess-1',
      request_id: 7,
      tool_name: 'Bash',
      tool_input: '{}',
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
      tool_input: '{}',
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
      tool_input: '{}',
    });

    // A stale resolution for an older request must not wipe the live notice.
    useLiveStore.getState().applyEvent({
      kind: 'permission_resolved',
      session_id: 'sess-1',
      request_id: 7,
    });
    expect(useLiveStore.getState().permission).toEqual({
      'sess-1': { requestId: 8, toolName: 'Bash', toolInput: '{}' },
    });
  });

  it('clears a session permission notice when the session closes', () => {
    useLiveStore.getState().applyEvent({
      kind: 'permission_requested',
      session_id: 'sess-1',
      request_id: 7,
      tool_name: 'Bash',
      tool_input: '{}',
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
