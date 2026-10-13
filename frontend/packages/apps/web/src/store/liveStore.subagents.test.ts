/**
 * Running-subagent tracking.
 *
 * Shared helpers live in liveStore.testkit.ts.
 */
import { beforeEach, describe, expect, it } from 'vitest';
import { threadIsRunning, useLiveStore } from './liveStore';
import { reset } from './liveStore.testkit';

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
