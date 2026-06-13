import { beforeEach, describe, expect, it } from 'vitest';
import { noticeOf, useLiveStore, type LocalSend } from './liveStore';

function reset() {
  useLiveStore.setState({
    connection: 'connecting',
    sending: [],
    localSends: {},
    spawns: [],
    activeTurns: {},
    notices: {},
    unread: {},
  });
}

/** The notices map, for `noticeOf` lookups in assertions. */
function notices() {
  return useLiveStore.getState().notices;
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

  it('resetTurnEphemera drops tracked sends, running flags, and permission notices, nothing else', () => {
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
    useLiveStore.getState().applyEvent({
      kind: 'permission_requested',
      session_id: 'sess-1',
      request_id: 7,
      tool_name: 'Bash',
      tool_input: '{}',
    });
    useLiveStore.getState().noteExternalInput('sess-1', 3, 'typed');
    useLiveStore.getState().markResumeUnavailable('sess-2');

    // A reconnect cannot reconcile event-reconstructed turn state (including
    // the permission notice, whose resolution may have been missed — it is
    // re-seeded from the refetched sends envelope). The in-flight POST and
    // the tracked spawn heal on their own (the POST resolves; the spawn
    // registers via the refetched session list), and the notices with no
    // server counterpart stay: each has a non-event escape hatch.
    useLiveStore.getState().resetTurnEphemera();

    expect(useLiveStore.getState().localSends).toEqual({});
    expect(useLiveStore.getState().activeTurns).toEqual({});
    expect(useLiveStore.getState().sending).toHaveLength(1);
    expect(useLiveStore.getState().spawns).toHaveLength(1);
    expect(noticeOf(notices(), 'sess-1', 'permission')).toBeNull();
    expect(noticeOf(notices(), 'sess-1', 'external_input')).not.toBeNull();
    expect(noticeOf(notices(), 'sess-2', 'resume_unavailable')).not.toBeNull();
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
    expect(
      noticeOf(notices(), 'sess-spawn-1', 'spawn_failure_buffered'),
    ).toBeNull();
  });

  it('drops a buffered early failure once that session registers', () => {
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      session_id: 'sess-foreign',
      pane_token: 'pane-x',
    });
    expect(
      noticeOf(notices(), 'sess-foreign', 'spawn_failure_buffered'),
    ).not.toBeNull();

    useLiveStore.getState().applyEvent({
      kind: 'session_registered',
      session_id: 'sess-foreign',
    });
    expect(
      noticeOf(notices(), 'sess-foreign', 'spawn_failure_buffered'),
    ).toBeNull();

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
    expect(
      noticeOf(notices(), 'sess-spawn-1', 'spawn_failure_buffered'),
    ).toBeNull();
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

  const PERMISSION_NOTICE = {
    kind: 'permission',
    requestId: 7,
    toolName: 'Bash',
    toolInput: '{}',
    dismissed: false,
  } as const;

  function requestPermission(sessionId = 'sess-1', requestId = 7) {
    useLiveStore.getState().applyEvent({
      kind: 'permission_requested',
      session_id: sessionId,
      request_id: requestId,
      tool_name: 'Bash',
      tool_input: '{}',
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
    useLiveStore.getState().applyEvent({
      kind: 'permission_resolved',
      session_id: 'sess-1',
      request_id: 7,
    });
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
    useLiveStore.getState().seedPermission('sess-1', {
      request_id: 7,
      tool_name: 'Bash',
      tool_input: '{}',
    });
    expect(noticeOf(notices(), 'sess-1', 'permission')).toEqual(
      PERMISSION_NOTICE,
    );
  });

  it('seedPermission never clears, and never un-dismisses the shown request', () => {
    requestPermission();
    useLiveStore.getState().dismissPermission('sess-1');

    // A report of the SAME request changes nothing — in particular it must
    // not resurrect the card the user just dismissed.
    useLiveStore.getState().seedPermission('sess-1', {
      request_id: 7,
      tool_name: 'Bash',
      tool_input: '{}',
    });
    expect(noticeOf(notices(), 'sess-1', 'permission')?.dismissed).toBe(true);

    // A null report clears nothing: clearing is owned by the events and the
    // lifecycle sweeps, so a momentarily-stale refetch cannot wipe a notice
    // an event just set.
    useLiveStore.getState().seedPermission('sess-1', null);
    expect(noticeOf(notices(), 'sess-1', 'permission')).not.toBeNull();

    // A DIFFERENT pending request is a new question: it replaces the entry,
    // un-dismissed.
    useLiveStore.getState().seedPermission('sess-1', {
      request_id: 9,
      tool_name: 'Edit',
      tool_input: '{}',
    });
    expect(noticeOf(notices(), 'sess-1', 'permission')).toEqual({
      kind: 'permission',
      requestId: 9,
      toolName: 'Edit',
      toolInput: '{}',
      dismissed: false,
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

  it('keeps the resume-unavailable notice across turn ends and clears it on open', () => {
    useLiveStore.getState().markResumeUnavailable('sess-1');
    expect(noticeOf(notices(), 'sess-1', 'resume_unavailable')).not.toBeNull();

    // A resume-impossible session is closed and turn-less; neither sweep that
    // drains the turn-scoped notices may touch it.
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
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

describe('liveStore.applyEvent question notices', () => {
  beforeEach(reset);

  const QUESTION_INPUT = '{"questions":[{"header":"Pick"}]}';
  const QUESTION_NOTICE = {
    kind: 'question',
    requestId: 5,
    toolInput: QUESTION_INPUT,
    dismissed: false,
  } as const;

  function askQuestion(sessionId = 'sess-1', requestId = 5) {
    useLiveStore.getState().applyEvent({
      kind: 'question_asked',
      session_id: sessionId,
      request_id: requestId,
      tool_input: QUESTION_INPUT,
    });
  }

  it('records a question notice on question_asked and flags it dismissed on dismiss', () => {
    askQuestion();
    expect(noticeOf(notices(), 'sess-1', 'question')).toEqual(QUESTION_NOTICE);

    useLiveStore.getState().dismissQuestion('sess-1');
    expect(noticeOf(notices(), 'sess-1', 'question')).toEqual({
      ...QUESTION_NOTICE,
      dismissed: true,
    });
  });

  it('clears a question notice when its request resolves (the user answered)', () => {
    askQuestion();

    // The correlated tool_result was ingested, so the same `permission_resolved`
    // event that clears a permission dialog clears the matching question.
    useLiveStore.getState().applyEvent({
      kind: 'permission_resolved',
      session_id: 'sess-1',
      request_id: 5,
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toBeNull();
  });

  it('ignores a resolution for a different request than the current question', () => {
    askQuestion('sess-1', 6);

    useLiveStore.getState().applyEvent({
      kind: 'permission_resolved',
      session_id: 'sess-1',
      request_id: 5,
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toEqual({
      ...QUESTION_NOTICE,
      requestId: 6,
    });
  });

  it('clears a question notice when its turn completes', () => {
    askQuestion();

    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      stop_reason: null,
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toBeNull();
  });

  it('clears a question notice when the session closes', () => {
    askQuestion();

    useLiveStore.getState().applyEvent({
      kind: 'session_closed',
      session_id: 'sess-1',
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toBeNull();
  });

  it('seedQuestion re-creates the notice the missed event would have set', () => {
    useLiveStore.getState().seedQuestion('sess-1', {
      request_id: 5,
      tool_input: QUESTION_INPUT,
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toEqual(QUESTION_NOTICE);
  });

  it('seedQuestion never clears, and never un-dismisses the shown question', () => {
    askQuestion();
    useLiveStore.getState().dismissQuestion('sess-1');

    // A report of the SAME request must not resurrect the dismissed card.
    useLiveStore.getState().seedQuestion('sess-1', {
      request_id: 5,
      tool_input: QUESTION_INPUT,
    });
    expect(noticeOf(notices(), 'sess-1', 'question')?.dismissed).toBe(true);

    // A null report clears nothing (clearing is owned by the events/sweeps).
    useLiveStore.getState().seedQuestion('sess-1', null);
    expect(noticeOf(notices(), 'sess-1', 'question')).not.toBeNull();

    // A DIFFERENT pending question replaces the entry, un-dismissed.
    useLiveStore.getState().seedQuestion('sess-1', {
      request_id: 9,
      tool_input: '{"questions":[]}',
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toEqual({
      kind: 'question',
      requestId: 9,
      toolInput: '{"questions":[]}',
      dismissed: false,
    });
  });
});
