/**
 * Turn tracking, the assistant streaming preview, pre-acceptance sends and
 * per-thread latest activity.
 *
 * Shared helpers live in liveStore.testkit.ts.
 */
import { beforeEach, describe, expect, it } from 'vitest';
import { noticeOf, useLiveStore } from './liveStore';
import {
  beginNewSessionSending,
  beginThreadSending,
  localSend,
  notices,
  reset,
} from './liveStore.testkit';

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
      focusHandedOver: false,
      sessionId: 'sess-9',
      threadId: 42,
      text: 'first message',
      firstSendId: 9,
      workdir: '/projects/app',
      launchOptionIds: [],
      provider: 'claude',
      worktree: null,
      pullRequestNumber: null,
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
    // A failure the server could not explain carries no reason, so the chip
    // shows its generic line alone.
    expect(useLiveStore.getState().sending[0].reason).toBeUndefined();

    useLiveStore.getState().removeSending('l1');
    expect(useLiveStore.getState().sending).toHaveLength(0);
  });

  it('keeps the reason a rejection named, for the chip to show verbatim', () => {
    useLiveStore.getState().beginSending({
      id: 'l1',
      target: {
        kind: 'new-session',
        workdir: '/projects/app',
        launchOptionIds: [7],
        provider: 'claude',
        worktree: null,
        pullRequestNumber: null,
      },
      text: 'start a new session',
      status: 'sending',
      createdAt: 0,
    });

    useLiveStore
      .getState()
      .failSending('l1', 'launch option rejected: `cwd` cannot be used');

    expect(useLiveStore.getState().sending[0].reason).toBe(
      'launch option rejected: `cwd` cannot be used',
    );
  });
});

describe('liveStore per-thread latest activity', () => {
  beforeEach(reset);

  it('records a thread\'s activity and only ever moves it forward', () => {
    const note = useLiveStore.getState().noteThreadActivity;

    note(1, '2026-01-01T00:01:00Z');
    expect(useLiveStore.getState().threadActivity).toEqual({
      1: '2026-01-01T00:01:00Z',
    });

    // A later stamp wins.
    note(1, '2026-01-01T00:02:00Z');
    expect(useLiveStore.getState().threadActivity[1]).toBe(
      '2026-01-01T00:02:00Z',
    );

    // An earlier one — two live events for the same thread arriving out of
    // order — must not pull the mark back to an older moment, and leaves the
    // map identity-stable so no subscriber re-renders for nothing.
    const before = useLiveStore.getState().threadActivity;
    note(1, '2026-01-01T00:00:30Z');
    expect(useLiveStore.getState().threadActivity).toBe(before);
  });

  it('keeps each thread separate and defaults to the client clock', () => {
    const note = useLiveStore.getState().noteThreadActivity;
    note(1, '2026-01-01T00:01:00Z');
    note(2);

    const activity = useLiveStore.getState().threadActivity;
    expect(activity[1]).toBe('2026-01-01T00:01:00Z');
    // No timestamp given (the live events carry none), so the store stamps now.
    expect(Number.isNaN(Date.parse(activity[2]))).toBe(false);
  });
});
