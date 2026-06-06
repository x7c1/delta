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
  /**
   * Whether to auto-replay the script on the interval. Defaults to `true`.
   * When `false`, the source still performs the connection handshake (emits
   * `open`) but never schedules the script; events are then driven manually via
   * {@link FakeEventSource.emit}. This lets an external driver (e.g. an
   * end-to-end test) interleave events with user actions deterministically.
   */
  autoPlay?: boolean;
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
  private readonly autoPlay: boolean;
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
    this.autoPlay = options.autoPlay ?? true;
    // Mimic a connection handshake, then start replaying events (unless an
    // external driver is in control).
    queueMicrotask(() => {
      if (this.closed) {
        return;
      }
      this.emitStatus('open');
      if (this.autoPlay) {
        this.scheduleNext();
      }
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

  /**
   * Emit a single event to all subscribers. Intended for `autoPlay: false`
   * mode, where an external driver feeds events one at a time. No-op once the
   * source is closed.
   */
  emit(event: SessionEvent): void {
    if (this.closed) {
      return;
    }
    this.emitEvent(event);
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
