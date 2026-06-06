import type { SessionEvent } from '@delta/model';
import { SESSION_ID } from './fixtures';

/**
 * A dev/test fake event source. MSW cannot mock WebSockets, so in mock mode the
 * app uses this instead of the real `/ws` client. It is structurally compatible
 * with the `SessionEventSource` interface exported by `@delta/api-client` (this
 * package may not depend on api-client, so the shape is mirrored, not imported).
 */

export type FakeEventListener = (event: SessionEvent) => void;
export type FakeStatus = 'connecting' | 'open' | 'closed';
export type FakeStatusListener = (status: FakeStatus) => void;

export interface FakeEventSourceOptions {
  /** Delay between scripted events, in ms. Defaults to 1500. */
  intervalMs?: number;
  /** Override the scripted event sequence. */
  script?: SessionEvent[];
  /** Injectable timer, primarily for tests. Defaults to `setTimeout`. */
  scheduler?: (callback: () => void, ms: number) => void;
}

/** The default scripted sequence demonstrating each event variant. */
export function defaultScript(): SessionEvent[] {
  return [
    { kind: 'session_registered', session_id: SESSION_ID },
    {
      kind: 'turn_started',
      session_id: SESSION_ID,
      pending_send_id: 1,
      matched_uuid: 'uuid-u2',
    },
    {
      kind: 'permission_requested',
      session_id: SESSION_ID,
      request_id: 1,
      tool_name: 'Bash',
    },
    { kind: 'turn_completed', session_id: SESSION_ID, stop_reason: null },
    {
      kind: 'external_input',
      session_id: SESSION_ID,
      prompt: 'typed directly into the pane',
    },
  ];
}

export class FakeEventSource {
  private readonly eventListeners = new Set<FakeEventListener>();
  private readonly statusListeners = new Set<FakeStatusListener>();
  private readonly script: SessionEvent[];
  private readonly intervalMs: number;
  private readonly scheduler: (callback: () => void, ms: number) => void;
  private index = 0;
  private closed = false;

  constructor(options: FakeEventSourceOptions = {}) {
    this.script = options.script ?? defaultScript();
    this.intervalMs = options.intervalMs ?? 1500;
    this.scheduler =
      options.scheduler ??
      ((callback, ms) => {
        setTimeout(callback, ms);
      });
    // Mimic a connection handshake, then start replaying events.
    queueMicrotask(() => {
      if (this.closed) {
        return;
      }
      this.emitStatus('open');
      this.scheduleNext();
    });
  }

  private scheduleNext(): void {
    if (this.closed || this.index >= this.script.length) {
      return;
    }
    this.scheduler(() => {
      if (this.closed) {
        return;
      }
      const event = this.script[this.index++];
      this.emitEvent(event);
      this.scheduleNext();
    }, this.intervalMs);
  }

  private emitEvent(event: SessionEvent): void {
    for (const listener of this.eventListeners) {
      listener(event);
    }
  }

  private emitStatus(status: FakeStatus): void {
    for (const listener of this.statusListeners) {
      listener(status);
    }
  }

  onEvent(listener: FakeEventListener): () => void {
    this.eventListeners.add(listener);
    return () => this.eventListeners.delete(listener);
  }

  onStatus(listener: FakeStatusListener): () => void {
    this.statusListeners.add(listener);
    return () => this.statusListeners.delete(listener);
  }

  close(): void {
    this.closed = true;
    this.emitStatus('closed');
  }
}
