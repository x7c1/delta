import type { SessionEvent } from '@delta/wire-gen';
import { MAIN_THREAD_ID, SESSION_ID, SESSION_ID_2 } from './fixtures';

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
  // Compute reset deadlines relative to the script's construction time (app
  // boot in dev), so the navigator footer's 5h / 7d countdowns read as plausible
  // remaining values (`02h13m`, `05d04h`) rather than a hard-coded epoch that
  // would drift to "<1m" or a multi-year delta as time passed. Epoch seconds —
  // the wire form expected by RateLimitWindow.resets_at.
  const nowSeconds = Math.floor(Date.now() / 1000);
  const fiveHourResetsAt = nowSeconds + 2 * 3600 + 13 * 60;
  const sevenDayResetsAt = nowSeconds + 5 * 86400 + 4 * 3600;
  return [
    { kind: 'session_registered', session_id: SESSION_ID },
    // Seed the account-wide rate-limit meters so the navigator's 5h/7d footer
    // rows render in mock mode (they are hidden when no `status_updated` has
    // arrived). Both windows populated with non-trivial values exercises the
    // meter fill, the zero-padded percentage column, and the `↻ HHhMMm` /
    // `↻ DDdHHh` countdown formats. `context_used_percentage` is set so the
    // transcript composer's context-usage bar (top border of the composer card)
    // also renders — it is hidden until the first non-null percentage arrives.
    {
      kind: 'status_updated',
      session_id: SESSION_ID,
      snapshot: {
        model_id: 'claude-opus-4-8',
        model_display_name: 'Opus 4.8',
        context_used_percentage: 38,
        context_window_size: null,
        context_current_usage: null,
        total_input_tokens: null,
        five_hour: { used_percentage: 67, resets_at: fiveHourResetsAt },
        seven_day: { used_percentage: 42, resets_at: sevenDayResetsAt },
        total_cost_usd: null,
        current_dir: null,
      },
    },
    {
      kind: 'turn_started',
      session_id: SESSION_ID,
      thread_id: MAIN_THREAD_ID,
      send_id: 1,
      matched_uuid: 'uuid-u2',
    },
    {
      kind: 'permission_requested',
      session_id: SESSION_ID,
      request_id: 1,
      tool_name: 'Bash',
      tool_input: '{"command":"npm install"}',
    },
    {
      kind: 'turn_completed',
      session_id: SESSION_ID,
      thread_id: MAIN_THREAD_ID,
      stop_reason: null,
    },
    {
      kind: 'external_input',
      session_id: SESSION_ID,
      prompt: 'typed directly into the pane',
    },
    // Resume the second (closed) session, then close it again, demonstrating the
    // open/close lifecycle the navigator's indicator reflects.
    { kind: 'session_opened', session_id: SESSION_ID_2 },
    { kind: 'session_closed', session_id: SESSION_ID_2 },
    // A final permission request with no turn_completed after it, so the
    // per-session permission UI stays visible in mock mode for development: the
    // notice pinned above the focused session's composer and the "permission"
    // badge on its navigator row. (The earlier request above is resolved by its
    // turn_completed, demonstrating the request → resolve flow.)
    {
      kind: 'permission_requested',
      session_id: SESSION_ID,
      request_id: 2,
      tool_name: 'Bash',
      tool_input: '{"command":"rm -rf node_modules"}',
    },
    // Two subagents left running at the tail of the script — with no
    // turn_completed after them, which would sweep them — so the running
    // indicator stays visible in mock mode for development: the navigator row
    // badge and the conversation-pane indicator. One foreground, and one
    // background (`run_in_background: true`), the case whose indicator must
    // persist past its launching turn.
    {
      kind: 'subagent_started',
      session_id: SESSION_ID,
      thread_id: MAIN_THREAD_ID,
      tool_use_id: 'toolu-mock-subagent-fg',
      subagent_type: 'general-purpose',
      description: 'Explore the codebase',
      background: false,
    },
    {
      kind: 'subagent_started',
      session_id: SESSION_ID,
      thread_id: MAIN_THREAD_ID,
      tool_use_id: 'toolu-mock-subagent-bg',
      subagent_type: 'general-purpose',
      description: 'Background build & test',
      background: true,
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
  private status: FakeStatus = 'connecting';

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
    this.status = status;
    for (const listener of this.statusListeners) {
      listener(status);
    }
  }

  onEvent(listener: FakeEventListener): () => void {
    this.eventListeners.add(listener);
    return () => this.eventListeners.delete(listener);
  }

  /**
   * Subscribe to connection-status changes. A subscriber that arrives after
   * the handshake already settled is told the current status immediately —
   * the app may construct this source behind a dynamic import and subscribe a
   * microtask later than the constructor's handshake, and it must not be left
   * believing the source is still `connecting`.
   */
  onStatus(listener: FakeStatusListener): () => void {
    this.statusListeners.add(listener);
    if (this.status !== 'connecting') {
      listener(this.status);
    }
    return () => this.statusListeners.delete(listener);
  }

  close(): void {
    this.closed = true;
    this.emitStatus('closed');
  }
}
