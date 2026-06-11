import { describe, expect, it } from 'vitest';
import type { SessionEvent } from '@delta/wire-gen';
import { FakeEventSource } from './ws-fake';

describe('FakeEventSource', () => {
  it('replays the scripted events to subscribers via an injected scheduler', async () => {
    const queue: Array<() => void> = [];
    const script: SessionEvent[] = [
      { kind: 'session_registered', session_id: 'sess-1' },
      { kind: 'turn_completed', session_id: 'sess-1', stop_reason: null },
    ];
    const source = new FakeEventSource({
      script,
      scheduler: (cb) => queue.push(cb),
    });

    const events: SessionEvent[] = [];
    source.onEvent((event) => events.push(event));

    // Let the queued microtask (handshake + first schedule) run.
    await Promise.resolve();
    // Drain the scheduler queue.
    while (queue.length > 0) {
      queue.shift()!();
    }

    expect(events).toEqual(script);
  });

  it('stops emitting once closed', async () => {
    const queue: Array<() => void> = [];
    const source = new FakeEventSource({
      script: [{ kind: 'turn_completed', session_id: 'sess-1', stop_reason: null }],
      scheduler: (cb) => queue.push(cb),
    });
    const events: SessionEvent[] = [];
    source.onEvent((event) => events.push(event));

    await Promise.resolve();
    source.close();
    while (queue.length > 0) {
      queue.shift()!();
    }

    expect(events).toEqual([]);
  });
});
