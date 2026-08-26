import { beforeEach, describe, expect, it } from 'vitest';
import type { StatusSnapshot } from '@delta/wire-gen';
import {
  noticeOf,
  threadIsRunning,
  useLiveStore,
  type LocalSend,
} from './liveStore';
import { loadPersistedStatus } from './statusPersistence';

function reset() {
  useLiveStore.setState({
    connection: 'connecting',
    sending: [],
    localSends: {},
    spawns: [],
    runningThreads: {},
    notices: {},
    unread: {},
    streamingMessages: {},
    runningSubagents: {},
    contextUsage: {},
    rateLimits: {},
  });
}

/**
 * A usage snapshot, with sensible defaults overridable per field. `rate_limits`
 * defaults to `null` — "this snapshot says nothing about rate limits" — which
 * is what a provider's token-usage-only frame looks like.
 */
function statusSnapshot(
  overrides: Partial<StatusSnapshot> = {},
): StatusSnapshot {
  return {
    provider: 'claude',
    model_id: null,
    model_display_name: null,
    context_used_percentage: null,
    context_window_size: null,
    context_current_usage: null,
    total_input_tokens: null,
    rate_limits: null,
    total_cost_usd: null,
    current_dir: null,
    ...overrides,
  };
}

/** A rate-limit window of `durationSeconds`, as the wire delivers it. */
function window(
  durationSeconds: number | null,
  usedPercentage: number | null,
  resetsAt: number | null,
) {
  return {
    duration_seconds: durationSeconds,
    used_percentage: usedPercentage,
    resets_at: resetsAt,
  };
}

const FIVE_HOURS = 5 * 60 * 60;
const SEVEN_DAYS = 7 * 24 * 60 * 60;

/** A thread-targeted submit chip, as `beginSending` records before its POST. */
function beginThreadSending(overrides: { sessionId?: string; threadId?: number } = {}) {
  useLiveStore.getState().beginSending({
    id: `local-${overrides.sessionId ?? 'sess-1'}-${overrides.threadId ?? 1}`,
    target: {
      kind: 'thread',
      sessionId: overrides.sessionId ?? 'sess-1',
      threadId: overrides.threadId ?? 1,
    },
    text: 'hi',
    status: 'sending',
    createdAt: 0,
  });
}

/**
 * A new-session submit chip, as `beginSending` records before its POST. The
 * target carries no session id — the response mints it — so this stands in for
 * the first-turn launch the new-session composer fires.
 */
function beginNewSessionSending(overrides: { id?: string } = {}) {
  useLiveStore.getState().beginSending({
    id: overrides.id ?? 'local-new-1',
    target: {
      kind: 'new-session',
      workdir: null,
      launchOptionIds: [],
    },
    text: 'hi',
    status: 'sending',
    createdAt: 0,
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
      thread_id: 1,
      send_id: 1,
      matched_uuid: 'uuid-1',
    });
    expect(useLiveStore.getState().runningThreads).toEqual({ 'sess-1': { 1: true } });

    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    expect(useLiveStore.getState().runningThreads).toEqual({});
  });

  it('clears the running flag on turn_interrupted', () => {
    useLiveStore.getState().applyEvent({
      kind: 'turn_started',
      session_id: 'sess-1',
      thread_id: 1,
      send_id: 1,
      matched_uuid: 'uuid-1',
    });

    useLiveStore.getState().applyEvent({
      kind: 'turn_interrupted',
      session_id: 'sess-1',
      thread_id: 1,
    });
    expect(useLiveStore.getState().runningThreads).toEqual({});
  });

  it('drains the tracked local send when its turn completes', () => {
    useLiveStore.getState().recordLocalSend(localSend());

    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
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
      thread_id: 1,
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
      thread_id: 1,
      stop_reason: null,
    });
    useLiveStore.getState().applyEvent({
      kind: 'turn_interrupted',
      session_id: 'sess-2',
      thread_id: 1,
    });
    expect(Object.keys(useLiveStore.getState().localSends)).toHaveLength(1);

    // The turn for the OWNING session still clears it.
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    expect(useLiveStore.getState().localSends).toEqual({});
  });

  it('records and drains normally when the POST returns before its turn ends (the race-won path)', () => {
    // The common ordering: the POST `onSuccess` runs before the turn ends, so
    // `recordLocalSend` stages the send and the turn-end drains it. No race
    // flag is involved.
    beginThreadSending();
    useLiveStore.getState().removeSending('local-sess-1-1');
    useLiveStore.getState().recordLocalSend(localSend());
    expect(Object.keys(useLiveStore.getState().localSends)).toEqual(['1']);

    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    expect(useLiveStore.getState().localSends).toEqual({});
    // No leaked state on the in-flight queue: the submit was already removed.
    expect(useLiveStore.getState().sending).toEqual([]);
  });

  it('flags a racing thread submit on turn-end so the late POST drops the send instead of staging it', () => {
    // The race-lost ordering: a fast echo turn completes before the POST
    // resolves. The submit chip is still in flight (`beginSending`), the
    // turn-end lands first and finds nothing to drain. Without intervention
    // the late `recordLocalSend` would stage a send whose turn already ended,
    // leaving a chip with no future drain trigger — the chip the user keeps
    // staring at. Instead the turn-end flags the racing submit `dropOnResolve`,
    // and the submit hook reads that flag to skip `recordLocalSend` entirely.
    beginThreadSending();

    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    // The submit is flagged in place; nothing else changed about it.
    expect(useLiveStore.getState().sending).toEqual([
      expect.objectContaining({
        id: 'local-sess-1-1',
        status: 'sending',
        dropOnResolve: true,
      }),
    ]);
    // The store does not stash any separate per-session race counter.
    expect(useLiveStore.getState()).not.toHaveProperty('endedBeforeRecorded');

    // The POST resolves. The submit hook (modeled here by checking the flag
    // before recording) drops the send without staging.
    const sendingItem = useLiveStore
      .getState()
      .sending.find((item) => item.id === 'local-sess-1-1');
    expect(sendingItem?.dropOnResolve).toBe(true);
    useLiveStore.getState().removeSending('local-sess-1-1');
    // recordLocalSend is intentionally NOT called when the flag was set.
    expect(useLiveStore.getState().localSends).toEqual({});
    expect(useLiveStore.getState().sending).toEqual([]);
  });

  it('flags a racing new-session POST on turn-end before its session id is known', () => {
    // The same race for the FIRST send of a freshly-spawned session: the
    // launch POST is still in flight (its target is `new-session`, so the
    // session id is unknown until the response), the echo turn completes
    // first and drains nothing. A new-session submit cannot match by session
    // id, so any in-flight new-session POST is treated as a possible racer
    // for the ending session and flagged in submit order — without this gate
    // the first-turn chip lingered forever (the reported flake).
    beginNewSessionSending();

    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-spawn-1',
      thread_id: 42,
      stop_reason: null,
    });
    expect(useLiveStore.getState().sending).toEqual([
      expect.objectContaining({
        id: 'local-new-1',
        status: 'sending',
        dropOnResolve: true,
      }),
    ]);

    // POST resolves and the submit hook drops the send under the minted ids.
    useLiveStore.getState().removeSending('local-new-1');
    expect(useLiveStore.getState().localSends).toEqual({});
  });

  it('flags each racing submit independently when two turns end before recording', () => {
    // Two POSTs are in flight at once and both their turns end before either
    // POST returns. Each turn-end must flag a DIFFERENT submit (FIFO in submit
    // order), so each late `onSuccess` drops the right send. Re-flagging the
    // same submit would leave the second send staged with no drain trigger.
    beginThreadSending({ threadId: 1 });
    beginThreadSending({ threadId: 2 });
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    const flagged = useLiveStore
      .getState()
      .sending.filter((item) => item.dropOnResolve === true)
      .map((item) => item.id);
    expect(flagged).toEqual(['local-sess-1-1', 'local-sess-1-2']);
  });

  it('does not flag a turn-end that already drained a tracked send (the common path)', () => {
    // A turn that drains a tracked send (the common case), or an external
    // direct-pane turn with no browser submit, must not flag anything — a
    // stray flag would wrongly drop the NEXT genuinely-pending submit.
    beginThreadSending({ threadId: 2 });
    useLiveStore.getState().recordLocalSend(localSend());
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    expect(useLiveStore.getState().localSends).toEqual({});
    expect(
      useLiveStore.getState().sending.find((item) => item.dropOnResolve === true),
    ).toBeUndefined();

    // The unrelated still-in-flight submit is unaffected and a later send
    // tracks normally.
    useLiveStore.getState().recordLocalSend(localSend({ sendId: 2, threadId: 2 }));
    expect(Object.keys(useLiveStore.getState().localSends)).toEqual(['2']);
  });

  it('does not cross-flag: a turn ending in another session leaves this submit unflagged', () => {
    beginThreadSending({ sessionId: 'sess-1' });
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-2',
      thread_id: 1,
      stop_reason: null,
    });
    // A thread-targeted submit only matches a turn-end on its OWN session id.
    expect(
      useLiveStore.getState().sending[0]?.dropOnResolve,
    ).toBeUndefined();

    useLiveStore.getState().recordLocalSend(localSend({ sessionId: 'sess-1' }));
    expect(Object.keys(useLiveStore.getState().localSends)).toEqual(['1']);
  });

  it('does not re-flag a submit that already carries the dropOnResolve flag', () => {
    // A second turn-end on the same session with only one in-flight submit
    // must not double-flag it: there is no second drop to consume, and a
    // duplicate flag would just be a no-op anyway.
    beginThreadSending();
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    const flagged = useLiveStore
      .getState()
      .sending.filter((item) => item.dropOnResolve === true);
    expect(flagged).toHaveLength(1);
  });

  it('does not flag a submit on session_closed (no further POST can follow)', () => {
    // A closed session accepts no further sends, so the flag would never get
    // consumed — better to leave the submit alone and let its retry (if any)
    // run normally.
    beginThreadSending();
    useLiveStore.getState().applyEvent({
      kind: 'session_closed',
      session_id: 'sess-1',
    });
    expect(
      useLiveStore.getState().sending[0]?.dropOnResolve,
    ).toBeUndefined();
  });

  it('drains turn state when the session closes', () => {
    useLiveStore.getState().recordLocalSend(localSend());
    useLiveStore.getState().applyEvent({
      kind: 'turn_started',
      session_id: 'sess-1',
      thread_id: 1,
      send_id: 1,
      matched_uuid: 'uuid-1',
    });

    useLiveStore.getState().applyEvent({
      kind: 'session_closed',
      session_id: 'sess-1',
    });
    expect(useLiveStore.getState().localSends).toEqual({});
    expect(useLiveStore.getState().runningThreads).toEqual({});
  });

  it('resetTurnEphemera drops tracked sends, running flags, permission notices, and the race flag, nothing else', () => {
    useLiveStore.getState().recordLocalSend(localSend());
    useLiveStore.getState().applyEvent({
      kind: 'turn_started',
      session_id: 'sess-1',
      thread_id: 1,
      send_id: 1,
      matched_uuid: 'uuid-1',
    });
    // An in-flight submit ALREADY race-flagged by a missed turn-end (modeled
    // directly so the assertion is about reset behavior, not how the flag
    // got there): without clearing on reset, the flag would survive the outage
    // and wrongly drop the legitimately-pending send when the POST eventually
    // resolves.
    useLiveStore.getState().beginSending({
      id: 'l1',
      target: { kind: 'thread', sessionId: 'sess-1', threadId: 1 },
      text: 'in flight',
      status: 'sending',
      createdAt: 0,
    });
    useLiveStore.setState((state) => ({
      sending: state.sending.map((item) =>
        item.id === 'l1' ? { ...item, dropOnResolve: true as const } : item,
      ),
    }));
    expect(useLiveStore.getState().sending[0]?.dropOnResolve).toBe(true);
    useLiveStore.getState().trackSpawn({
      sessionId: 'sess-9',
      threadId: 42,
      text: 'first message',
      workdir: null,
      launchOptionIds: [],
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
    useLiveStore.getState().applyEvent({
      kind: 'assistant_streaming',
      session_id: 'sess-1',
      thread_id: 1,
      message_id: 'm1',
      index: 0,
      final: false,
      delta: 'partial',
    });

    // A reconnect cannot reconcile event-reconstructed turn state (including
    // the permission notice, whose resolution may have been missed — it is
    // re-seeded from the refetched sends envelope). The in-flight POST and
    // the tracked spawn heal on their own (the POST resolves; the spawn
    // registers via the refetched session list), and the notices with no
    // server counterpart stay: each has a non-event escape hatch.
    useLiveStore.getState().resetTurnEphemera();

    expect(useLiveStore.getState().localSends).toEqual({});
    expect(useLiveStore.getState().runningThreads).toEqual({});
    expect(useLiveStore.getState().sending).toHaveLength(1);
    // The race flag is cleared so the POST `onSuccess` stages the send
    // normally — we can no longer prove its turn is over.
    expect(useLiveStore.getState().sending[0]?.dropOnResolve).toBeUndefined();
    expect(useLiveStore.getState().spawns).toHaveLength(1);
    expect(noticeOf(notices(), 'sess-1', 'permission')).toBeNull();
    expect(noticeOf(notices(), 'sess-1', 'external_input')).not.toBeNull();
    expect(noticeOf(notices(), 'sess-2', 'resume_unavailable')).not.toBeNull();
    // The live preview cannot be re-seeded (no partial-stream replay), so a
    // reconnect drops it; the flushed message renders from the refetch.
    expect(useLiveStore.getState().streamingMessages).toEqual({});
  });

  it('seedActiveTurn (stale read) sets from in_flight and only sets', () => {
    // After a reconnect the refetched sends envelope reports the turn state;
    // a stale read of a non-idle phase re-seeds the flag the dropped events
    // would have set.
    useLiveStore
      .getState()
      .seedActiveTurn('sess-1', { state: 'in_flight', send_id: 1, thread_id: 1 }, false);
    expect(useLiveStore.getState().runningThreads).toEqual({ 'sess-1': { 1: true } });

    // A stale idle report never clears: turn-end events own clearing, so a
    // momentarily-stale refetch cannot wipe a flag an event just set.
    useLiveStore
      .getState()
      .seedActiveTurn('sess-1', { state: 'idle', send_id: null, thread_id: null }, false);
    expect(useLiveStore.getState().runningThreads).toEqual({ 'sess-1': { 1: true } });
  });

  it('seedActiveTurn (stale read) ignores awaiting_echo: turn not started yet', () => {
    // A dispatched-but-unechoed send is what `send_dispatched` reports live —
    // it never set the running flag, so the refetch must not either.
    useLiveStore
      .getState()
      .seedActiveTurn('sess-1', { state: 'awaiting_echo', send_id: 2, thread_id: 1 }, false);
    expect(useLiveStore.getState().runningThreads).toEqual({});
  });

  it('seedActiveTurn (fresh read) reconciles the flag to the server truth', () => {
    // A genuinely fresh fetch is authoritative: a fresh in_flight sets the
    // flag (reconnect healing), and a fresh idle CLEARS it — the server says
    // there is no running turn. This is what stops a re-focus from leaving the
    // running spinner stuck on (a stale cached in_flight already set the flag,
    // and only the fresh idle that follows can clear it).
    useLiveStore
      .getState()
      .seedActiveTurn('sess-1', { state: 'in_flight', send_id: 1, thread_id: 1 }, true);
    expect(useLiveStore.getState().runningThreads).toEqual({ 'sess-1': { 1: true } });

    useLiveStore
      .getState()
      .seedActiveTurn('sess-1', { state: 'idle', send_id: null, thread_id: null }, true);
    expect(useLiveStore.getState().runningThreads).toEqual({});
  });

  it('seedActiveTurn (fresh read) reconciles awaiting_echo to not-running', () => {
    // awaiting_echo is not in_flight, so a fresh read clears any leftover flag,
    // consistent with the stale-read mode ignoring it.
    useLiveStore.setState({ runningThreads: { 'sess-1': { 1: true } } });
    useLiveStore
      .getState()
      .seedActiveTurn('sess-1', { state: 'awaiting_echo', send_id: 2, thread_id: 1 }, true);
    expect(useLiveStore.getState().runningThreads).toEqual({});
  });
});

describe('liveStore assistant streaming preview', () => {
  beforeEach(reset);

  function stream(
    overrides: Partial<{
      session_id: string;
      thread_id: number;
      message_id: string;
      index: number;
      final: boolean;
      delta: string;
    }> = {},
  ) {
    useLiveStore.getState().applyEvent({
      kind: 'assistant_streaming',
      session_id: 'sess-1',
      thread_id: 1,
      message_id: 'm1',
      index: 0,
      final: false,
      delta: '',
      ...overrides,
    });
  }

  it('accumulates deltas in index order into one per-session preview', () => {
    stream({ index: 0, delta: 'Hel' });
    stream({ index: 1, delta: 'lo ' });
    stream({ index: 2, delta: 'world', final: true });

    const preview = useLiveStore.getState().streamingMessages['sess-1'];
    expect(preview.text).toBe('Hello world');
    expect(preview.threadId).toBe(1);
    expect(preview.messageId).toBe('m1');
    expect(preview.done).toBe(true);
  });

  it('reconciles out-of-order and duplicate chunks deterministically', () => {
    stream({ index: 1, delta: 'B' });
    stream({ index: 0, delta: 'A' });
    // A re-delivered index overwrites rather than appends.
    stream({ index: 1, delta: 'B' });
    stream({ index: 2, delta: 'C', final: true });

    expect(useLiveStore.getState().streamingMessages['sess-1'].text).toBe('ABC');
  });

  it('starts a fresh preview when the message_id changes', () => {
    stream({ message_id: 'm1', index: 0, delta: 'first' });
    stream({ message_id: 'm2', index: 0, delta: 'second' });
    expect(useLiveStore.getState().streamingMessages['sess-1'].text).toBe(
      'second',
    );
    expect(useLiveStore.getState().streamingMessages['sess-1'].messageId).toBe(
      'm2',
    );
  });

  it('keeps the preview on turn_completed (suppression owns the handoff)', () => {
    // turn_completed (the Stop hook) can outrun the async transcript refetch
    // that persists the assistant message. Clearing the buffer here would
    // remove the bubble before the persisted copy renders, leaving a visible
    // gap. So the buffer is left in place: the content-based suppression guard
    // (persistedHasStreamedText) removes it in the same render that adds the
    // persisted message — a gap-free swap. The next turn's first chunk (a new
    // message_id) overwrites it, so it does not accumulate.
    stream({ index: 0, delta: 'partial' });
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    expect(useLiveStore.getState().streamingMessages['sess-1'].text).toBe(
      'partial',
    );
  });

  it('clears the preview on turn_interrupted', () => {
    stream({ index: 0, delta: 'partial' });
    useLiveStore.getState().applyEvent({
      kind: 'turn_interrupted',
      session_id: 'sess-1',
      thread_id: 1,
    });
    expect(useLiveStore.getState().streamingMessages).toEqual({});
  });

  it('clears the preview on session_closed', () => {
    stream({ index: 0, delta: 'partial' });
    useLiveStore.getState().applyEvent({
      kind: 'session_closed',
      session_id: 'sess-1',
    });
    expect(useLiveStore.getState().streamingMessages).toEqual({});
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
      launchOptionIds: [],
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

  it('releases the tracked spawn when its session registers', () => {
    // The launch bound: the spawn window is over. The workspace focused the
    // session when its send was accepted and its row is listed by now, so the
    // tracked entry has nothing left to do. Only that one entry goes.
    trackOne();
    trackOne('sess-spawn-2');

    useLiveStore.getState().applyEvent({
      kind: 'session_registered',
      session_id: 'sess-spawn-1',
    });

    expect(
      useLiveStore.getState().spawns.map((spawn) => spawn.sessionId),
    ).toEqual(['sess-spawn-2']);
  });

  it('keeps a failed spawn when a session_registered arrives for it', () => {
    // A `failed` entry is the Retry / Dismiss card, and only the user's answer
    // removes it — a registration for the same id must never take it away
    // under them.
    trackOne();
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
    });

    useLiveStore.getState().applyEvent({
      kind: 'session_registered',
      session_id: 'sess-spawn-1',
    });

    expect(useLiveStore.getState().spawns[0].status).toBe('failed');
  });

  it('leaves tracked spawns alone on session_registered for another session', () => {
    trackOne();

    useLiveStore.getState().applyEvent({
      kind: 'session_registered',
      session_id: 'sess-someone-elses',
    });
    expect(useLiveStore.getState().spawns).toHaveLength(1);
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
      thread_id: 1,
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

describe('liveStore.applyEvent question notices', () => {
  beforeEach(reset);

  const QUESTION_INPUT = '{"questions":[{"header":"Pick"}]}';
  const QUESTION_THREAD = 3;
  const QUESTION_NOTICE = {
    kind: 'question',
    requestId: 5,
    threadId: QUESTION_THREAD,
    toolInput: QUESTION_INPUT,
    dismissed: false,
  } as const;

  function askQuestion(sessionId = 'sess-1', requestId = 5) {
    useLiveStore.getState().applyEvent({
      kind: 'question_asked',
      session_id: sessionId,
      request_id: requestId,
      thread_id: QUESTION_THREAD,
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
      thread_id: 1,
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
      thread_id: QUESTION_THREAD,
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
      thread_id: QUESTION_THREAD,
      tool_input: QUESTION_INPUT,
    });
    expect(noticeOf(notices(), 'sess-1', 'question')?.dismissed).toBe(true);

    // A null report clears nothing (clearing is owned by the events/sweeps).
    useLiveStore.getState().seedQuestion('sess-1', null);
    expect(noticeOf(notices(), 'sess-1', 'question')).not.toBeNull();

    // A DIFFERENT pending question replaces the entry, un-dismissed.
    useLiveStore.getState().seedQuestion('sess-1', {
      request_id: 9,
      thread_id: 7,
      tool_input: '{"questions":[]}',
    });
    expect(noticeOf(notices(), 'sess-1', 'question')).toEqual({
      kind: 'question',
      requestId: 9,
      threadId: 7,
      toolInput: '{"questions":[]}',
      dismissed: false,
    });
  });
});

describe('liveStore running-subagent tracking', () => {
  beforeEach(reset);

  function subagents(sessionId = 'sess-1') {
    return useLiveStore.getState().runningSubagents[sessionId];
  }

  it('adds a running subagent on subagent_started with its display fields', () => {
    useLiveStore.getState().applyEvent({
      kind: 'subagent_started',
      session_id: 'sess-1',
      thread_id: 7,
      tool_use_id: 'toolu_a1',
      subagent_type: 'general-purpose',
      description: 'Probe the codebase',
      background: false,
    });
    expect(subagents()).toEqual([
      {
        threadId: 7,
        toolUseId: 'toolu_a1',
        subagentType: 'general-purpose',
        description: 'Probe the codebase',
        background: false,
      },
    ]);
  });

  it('records the launching thread and folds it into thread-running, suppressing unread until the subagent finishes', () => {
    const store = useLiveStore.getState();
    // A turn on thread 7 completed off-focus, bumping its unread, while the
    // background subagent it launched keeps running.
    store.applyEvent({
      kind: 'subagent_started',
      session_id: 'sess-1',
      thread_id: 7,
      tool_use_id: 'toolu_bg',
      subagent_type: null,
      description: null,
      background: true,
    });
    store.applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 7,
      stop_reason: null,
    });
    store.bumpUnread(7);

    // The thread reads as running purely from its still-running subagent, even
    // though its turn already ended (so the navigator shows the spinner, not the
    // "done while you were away" dot).
    const running = useLiveStore.getState().runningSubagents['sess-1'];
    expect(running?.[0]?.threadId).toBe(7);
    expect(threadIsRunning(undefined, running, 7 as number)).toBe(true);
    expect(threadIsRunning(undefined, running, 8 as number)).toBe(false);

    // Once the subagent finishes, the thread is idle again and its unread
    // surfaces (the dot appears).
    store.applyEvent({
      kind: 'subagent_finished',
      session_id: 'sess-1',
      tool_use_id: 'toolu_bg',
    });
    expect(
      threadIsRunning(undefined, useLiveStore.getState().runningSubagents['sess-1'], 7 as number),
    ).toBe(false);
    expect(useLiveStore.getState().unread[7]).toBe(1);
  });

  it('removes the subagent on subagent_finished and drops the empty entry', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'subagent_started',
      session_id: 'sess-1',
      thread_id: 7,
      tool_use_id: 'toolu_a1',
      subagent_type: null,
      description: null,
      background: false,
    });
    store.applyEvent({
      kind: 'subagent_finished',
      session_id: 'sess-1',
      tool_use_id: 'toolu_a1',
    });
    expect(subagents()).toBeUndefined();
  });

  it('tracks multiple concurrent subagents in start order and clears one at a time', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'subagent_started',
      session_id: 'sess-1',
      thread_id: 7,
      tool_use_id: 'toolu_a1',
      subagent_type: null,
      description: null,
      background: false,
    });
    store.applyEvent({
      kind: 'subagent_started',
      session_id: 'sess-1',
      thread_id: 7,
      tool_use_id: 'toolu_a2',
      subagent_type: null,
      description: null,
      background: false,
    });
    expect(subagents()?.map((s) => s.toolUseId)).toEqual([
      'toolu_a1',
      'toolu_a2',
    ]);

    store.applyEvent({
      kind: 'subagent_finished',
      session_id: 'sess-1',
      tool_use_id: 'toolu_a1',
    });
    expect(subagents()?.map((s) => s.toolUseId)).toEqual(['toolu_a2']);
  });

  it('ignores a duplicate subagent_started for the same tool_use_id', () => {
    const store = useLiveStore.getState();
    const started = {
      kind: 'subagent_started' as const,
      session_id: 'sess-1',
      thread_id: 7,
      tool_use_id: 'toolu_a1',
      subagent_type: null,
      description: null,
      background: false,
    };
    store.applyEvent(started);
    store.applyEvent(started);
    expect(subagents()).toHaveLength(1);
  });

  it('ignores a subagent_finished for an untracked tool_use_id', () => {
    useLiveStore.getState().applyEvent({
      kind: 'subagent_finished',
      session_id: 'sess-1',
      tool_use_id: 'toolu_never',
    });
    expect(subagents()).toBeUndefined();
  });

  it('clears running subagents when the turn completes', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'subagent_started',
      session_id: 'sess-1',
      thread_id: 7,
      tool_use_id: 'toolu_a1',
      subagent_type: null,
      description: null,
      background: false,
    });
    store.applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    expect(subagents()).toBeUndefined();
  });

  it('clears running subagents when the session closes', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'subagent_started',
      session_id: 'sess-1',
      thread_id: 7,
      tool_use_id: 'toolu_a1',
      subagent_type: null,
      description: null,
      background: false,
    });
    store.applyEvent({ kind: 'session_closed', session_id: 'sess-1' });
    expect(subagents()).toBeUndefined();
  });

  it('keeps a background subagent while sweeping a foreground one at turn end', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'subagent_started',
      session_id: 'sess-1',
      thread_id: 7,
      tool_use_id: 'toolu_fg',
      subagent_type: null,
      description: null,
      background: false,
    });
    store.applyEvent({
      kind: 'subagent_started',
      session_id: 'sess-1',
      thread_id: 7,
      tool_use_id: 'toolu_bg',
      subagent_type: null,
      description: null,
      background: true,
    });
    // The launching turn ends: the foreground entry is swept, the background
    // one survives (it outlives the turn that launched it).
    store.applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    expect(subagents()?.map((s) => s.toolUseId)).toEqual(['toolu_bg']);
  });

  it('clears a surviving background subagent on its completion subagent_finished', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'subagent_started',
      session_id: 'sess-1',
      thread_id: 7,
      tool_use_id: 'toolu_bg',
      subagent_type: null,
      description: null,
      background: true,
    });
    // The launching turn ends; the background subagent keeps running.
    store.applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    expect(subagents()?.map((s) => s.toolUseId)).toEqual(['toolu_bg']);
    // Its completion notification (folded server-side) arrives as a
    // subagent_finished, clearing it.
    store.applyEvent({
      kind: 'subagent_finished',
      session_id: 'sess-1',
      tool_use_id: 'toolu_bg',
    });
    expect(subagents()).toBeUndefined();
  });

  it('re-seeds the running set authoritatively from the sends envelope', () => {
    const store = useLiveStore.getState();
    // A stale local entry the reconnect could not reconcile from events.
    store.applyEvent({
      kind: 'subagent_started',
      session_id: 'sess-1',
      thread_id: 7,
      tool_use_id: 'toolu_stale',
      subagent_type: null,
      description: null,
      background: false,
    });
    // The server reports a different running set: it replaces the local copy.
    // A surviving background subagent is restored with its flag, so the
    // reconnecting client's later turn-end sweep keeps it.
    store.seedRunningSubagents('sess-1', [
      {
        thread_id: 4,
        tool_use_id: 'toolu_fresh',
        subagent_type: 'general-purpose',
        description: 'Still running',
        background: true,
      },
    ]);
    expect(subagents()).toEqual([
      {
        threadId: 4,
        toolUseId: 'toolu_fresh',
        subagentType: 'general-purpose',
        description: 'Still running',
        background: true,
      },
    ]);
  });

  it('seeding an empty list clears the session entry', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'subagent_started',
      session_id: 'sess-1',
      thread_id: 7,
      tool_use_id: 'toolu_a1',
      subagent_type: null,
      description: null,
      background: false,
    });
    store.seedRunningSubagents('sess-1', []);
    expect(subagents()).toBeUndefined();
  });

  it('resetTurnEphemera drops the event-reconstructed running set', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'subagent_started',
      session_id: 'sess-1',
      thread_id: 7,
      tool_use_id: 'toolu_a1',
      subagent_type: null,
      description: null,
      background: false,
    });
    store.resetTurnEphemera();
    expect(useLiveStore.getState().runningSubagents).toEqual({});
  });
});

describe('liveStore status_updated', () => {
  beforeEach(reset);

  it('keeps only the latest per-session context usage (replace, not append)', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({ context_used_percentage: 40 }),
    });
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({ context_used_percentage: 55 }),
    });
    // Only the most recent snapshot is retained for the session.
    expect(useLiveStore.getState().contextUsage).toEqual({ 'sess-1': 55 });
  });

  it('keeps context usage keyed per session (one session does not clobber another)', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({ context_used_percentage: 40 }),
    });
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-2',
      snapshot: statusSnapshot({ context_used_percentage: 70 }),
    });
    // Each session retains its own latest value; the second event does not
    // overwrite the first (context is per session, unlike the global rate limit).
    expect(useLiveStore.getState().contextUsage).toEqual({
      'sess-1': 40,
      'sess-2': 70,
    });
  });

  it('keeps one rate-limit snapshot per provider, replaced by the latest', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({
        rate_limits: [window(FIVE_HOURS, 10, 100)],
      }),
    });
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-2',
      snapshot: statusSnapshot({
        rate_limits: [window(FIVE_HOURS, 30, 300)],
      }),
    });
    // Both sessions are Claude sessions reporting the same account, so the
    // later event simply replaces that provider's windows.
    expect(useLiveStore.getState().rateLimits).toEqual({
      claude: [window(FIVE_HOURS, 30, 300)],
    });
  });

  it('keeps each provider\'s rate limits apart', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-claude',
      snapshot: statusSnapshot({
        rate_limits: [window(FIVE_HOURS, 10, 100)],
      }),
    });
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-codex',
      snapshot: statusSnapshot({
        provider: 'codex',
        rate_limits: [window(SEVEN_DAYS, 55, 500)],
      }),
    });
    // A Codex account update must not overwrite the Claude account's windows:
    // they are different accounts that happen to share a footer.
    expect(useLiveStore.getState().rateLimits).toEqual({
      claude: [window(FIVE_HOURS, 10, 100)],
      codex: [window(SEVEN_DAYS, 55, 500)],
    });
  });

  it('leaves rate limits alone when a snapshot says nothing about them', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-codex',
      snapshot: statusSnapshot({
        provider: 'codex',
        rate_limits: [window(SEVEN_DAYS, 55, 500)],
      }),
    });
    // A token-usage frame carries `rate_limits: null` — no statement — and the
    // account's windows must survive it. (A provider that reports usage and
    // limits on separate frames would otherwise wipe its own footer every turn.)
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-codex',
      snapshot: statusSnapshot({
        provider: 'codex',
        context_used_percentage: 25,
      }),
    });
    expect(useLiveStore.getState().rateLimits).toEqual({
      codex: [window(SEVEN_DAYS, 55, 500)],
    });
    expect(useLiveStore.getState().contextUsage).toEqual({ 'sess-codex': 25 });
  });

  it('clears a provider\'s windows when it reports it has none', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({ rate_limits: [window(FIVE_HOURS, 10, 100)] }),
    });
    // An empty list IS a statement — "this account has no windows" — so the
    // rows clear (e.g. a subscription that lapsed), unlike a `null`.
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({ rate_limits: [] }),
    });
    expect(useLiveStore.getState().rateLimits).toEqual({ claude: [] });
  });

  it('drops a session context entry when the percentage goes null (e.g. /compact)', () => {
    const store = useLiveStore.getState();
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({ context_used_percentage: 40 }),
    });
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({ context_used_percentage: null }),
    });
    expect(useLiveStore.getState().contextUsage).toEqual({});
  });
});

describe('liveStore status persistence', () => {
  beforeEach(() => {
    reset();
    localStorage.clear();
  });

  it('persists the latest status snapshot on status_updated so a reload can restore it', () => {
    useLiveStore.getState().applyEvent({
      kind: 'status_updated',
      session_id: 'sess-1',
      snapshot: statusSnapshot({
        context_used_percentage: 62,
        // Reset times in the far future so the freshness pruning keeps them
        // (resets_at is epoch seconds; load compares against now/1000).
        rate_limits: [
          window(FIVE_HOURS, 30, Date.now() / 1000 + 3600),
          window(SEVEN_DAYS, 9, Date.now() / 1000 + 5 * 86400),
        ],
      }),
    });

    // What a fresh page load would read back at init (the store seeds
    // contextUsage / rateLimits from this on startup).
    const restored = loadPersistedStatus(Date.now());
    expect(restored.contextUsage).toEqual({ 'sess-1': 62 });
    expect(restored.rateLimits).toEqual({
      claude: [
        { ...window(FIVE_HOURS, 30, expect.any(Number)) },
        { ...window(SEVEN_DAYS, 9, expect.any(Number)) },
      ],
    });
  });

  it('round-trips a multi-provider snapshot through localStorage', () => {
    const store = useLiveStore.getState();
    const resetsAt = Date.now() / 1000 + 3600;
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-claude',
      snapshot: statusSnapshot({ rate_limits: [window(FIVE_HOURS, 30, resetsAt)] }),
    });
    store.applyEvent({
      kind: 'status_updated',
      session_id: 'sess-codex',
      snapshot: statusSnapshot({
        provider: 'codex',
        context_used_percentage: 12,
        rate_limits: [window(SEVEN_DAYS, 44, resetsAt)],
      }),
    });

    // Both accounts survive the reload, still apart: a restore that collapsed
    // them would resurrect exactly the cross-provider leak the keying prevents.
    const restored = loadPersistedStatus(Date.now());
    expect(restored.rateLimits).toEqual({
      claude: [window(FIVE_HOURS, 30, resetsAt)],
      codex: [window(SEVEN_DAYS, 44, resetsAt)],
    });
    expect(restored.contextUsage).toEqual({ 'sess-codex': 12 });
  });
});
