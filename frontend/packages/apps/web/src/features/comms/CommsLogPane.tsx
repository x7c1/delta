import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { connectCommsLog } from '@delta/api-client';
import type { CommsFrame } from '@delta/wire-gen';
import type { SessionId } from '@delta/model';
import { Panel } from '@delta/ui-kit';
import { isMockMode, wsUrl } from '../../config';
import { loadMockCommsFrames } from '../../data/mockCommsFrames';
import { useNavStore } from '../../store/navStore';

/**
 * How many frames the pane keeps. The server's own ring buffer is the source of
 * truth for history; this only bounds what one open pane holds in memory during
 * a long session, so it is deliberately generous — the interesting part of a log
 * is always its recent end.
 */
const MAX_FRAMES = 1000;

/**
 * How close to the bottom still counts as "following the tail", in pixels.
 * Matches the transcript's own threshold, since it answers the same question
 * (has the reader scrolled away, or are they just a line off the end).
 */
const STICK_THRESHOLD_PX = 64;

export interface CommsLogPaneProps {
  /**
   * The focused session whose log to show. Null on the not-yet-bound New session
   * screen (there is no session to watch yet).
   */
  sessionId: SessionId | null;
  /**
   * Whether the focused session is open. A closed session has no live wire, so
   * the pane shows its idle state instead of connecting — and the server would
   * have discarded its buffer when it closed anyway.
   */
  attachable: boolean;
}

/**
 * The comms-log inspector: the JSON-RPC frames Delta exchanges with a
 * terminal-less provider's transport, for the focused session.
 *
 * This is the window a headless session has instead of a terminal. It answers
 * "what is the agent doing right now" at the level below the conversation: which
 * request Delta sent, what came back, which notifications the server pushed, and
 * whether it is waiting on an approval. Diagnosing anything at that level
 * otherwise means reading the database by hand.
 *
 * Read-only and disposable by design: frames are never persisted, so the pane
 * shows what the server still has buffered (recent history) plus everything that
 * arrives while it is open. Closing and reopening the pane replays the buffer
 * rather than resuming from where it left off — which is why there is no attempt
 * to keep instances alive across a session switch (unlike the terminal, where
 * re-attaching has a visible cost in the agent's own UI).
 *
 * The view follows the newest frame while the reader is at the end of the log,
 * and stops following the moment they scroll up — so a turn in flight stays in
 * view without stealing the frame someone is studying.
 */
export function CommsLogPane({ sessionId, attachable }: CommsLogPaneProps) {
  const [frames, setFrames] = useState<CommsFrame[]>([]);
  const [connected, setConnected] = useState(false);
  const setCommsOpen = useNavStore((state) => state.setCommsOpen);
  const bodyRef = useRef<HTMLDivElement | null>(null);
  // Whether the pane is following the tail: armed while the reader sits at the
  // end of the log, disarmed as soon as they scroll up to study an earlier frame.
  const stickRef = useRef(true);

  // Whether there is anything to stream at all: mock mode has no backend, and a
  // closed session has no live wire and no server-side buffer left. (Having a
  // session to name is the other half of the condition, checked at the guard
  // below so `sessionId` is narrowed where it is used.)
  const hasLiveWire = !isMockMode() && attachable;

  // Mock mode: no WebSocket exists, so the pane shows a scripted exchange
  // instead (see `loadMockCommsFrames`). Keeping this a separate effect means the
  // live path below is untouched by it — in a real build the import never runs.
  useEffect(() => {
    if (!isMockMode() || sessionId === null) {
      return;
    }
    let disposed = false;
    void loadMockCommsFrames().then((scripted) => {
      if (!disposed) {
        setFrames(scripted);
      }
    });
    return () => {
      disposed = true;
    };
  }, [sessionId]);

  useEffect(() => {
    // Mock mode owns the frame list (the effect above), so leave it alone.
    if (isMockMode()) {
      return;
    }
    // Clear BEFORE the connect guard, not after it: every change of session — or
    // of whether there is a live wire at all — starts the view over. Clearing
    // only on the connecting path would leave the previous session's frames on
    // screen when focus moved to a closed session or to the New session screen,
    // under a note saying nothing is being exchanged.
    setFrames([]);
    stickRef.current = true;
    if (!hasLiveWire || sessionId === null) {
      return;
    }
    const connection = connectCommsLog({
      url: wsUrl('/comms'),
      sessionId,
      onFrame: (frame) =>
        setFrames((current) => {
          const next = [...current, frame];
          return next.length > MAX_FRAMES
            ? next.slice(next.length - MAX_FRAMES)
            : next;
        }),
      onStatus: (status) => setConnected(status === 'open'),
      // A frame the browser cannot read is a contract problem worth seeing in
      // the console, but never a reason to stop showing the rest of the log.
      onError: (error) =>
        console.warn('ignoring an unreadable comms frame', error),
    });
    return () => {
      setConnected(false);
      connection.close();
    };
  }, [hasLiveWire, sessionId]);

  // Arm / disarm tail-following from the reader's own scrolling.
  useEffect(() => {
    const el = bodyRef.current;
    if (el === null) {
      return;
    }
    const onScroll = () => {
      stickRef.current =
        el.scrollHeight - el.scrollTop - el.clientHeight < STICK_THRESHOLD_PX;
    };
    el.addEventListener('scroll', onScroll, { passive: true });
    return () => el.removeEventListener('scroll', onScroll);
  }, []);

  // Keep the newest frame in view while following. Without this the pane opens
  // on the OLDEST frame of the server's replay (up to a few hundred) and every
  // frame that arrives afterwards lands below the fold — so a log whose whole
  // point is "what is the wire doing right now" would show its least
  // interesting end and never move.
  useLayoutEffect(() => {
    if (!stickRef.current) {
      return;
    }
    const el = bodyRef.current;
    if (el !== null) {
      el.scrollTop = el.scrollHeight;
    }
  }, [frames.length]);

  const note = useMemo(() => {
    if (isMockMode()) {
      return frames.length === 0
        ? 'Loading the scripted mock exchange…'
        : null;
    }
    if (sessionId === null) {
      return 'No session is attached yet. Start a session to see the frames it exchanges.';
    }
    if (!attachable) {
      return 'This session is closed, so nothing is being exchanged. Resume it to watch its frames.';
    }
    if (frames.length === 0) {
      return connected
        ? 'Connected. Waiting for the next frame — send a message to see the exchange.'
        : 'Connecting…';
    }
    return null;
  }, [attachable, connected, frames.length, sessionId]);

  return (
    <Panel
      className="border-l border-border-default"
      bodyRef={bodyRef}
      header={
        <div className="flex items-center justify-between gap-2">
          <span className="text-caption font-medium text-fg">
            Communication log
          </span>
          <button
            type="button"
            onClick={() => setCommsOpen(false)}
            aria-label="Close communication log"
            title="Close communication log"
            className="rounded px-1.5 py-0.5 text-secondary leading-none text-fg-subtle transition hover:bg-surface-elevated hover:text-fg"
          >
            »
          </button>
        </div>
      }
    >
      <div data-testid="comms-pane" className="min-w-0">
        {note !== null && (
          <p data-testid="comms-empty-note" className="p-3 text-caption text-fg-muted">
            {note}
          </p>
        )}
        <ol className="min-w-0">
          {frames.map((frame) => (
            <CommsFrameRow key={frame.seq} frame={frame} />
          ))}
        </ol>
      </div>
    </Panel>
  );
}

/**
 * A frame's two-line summary plus its payload behind a disclosure.
 *
 * Collapsed by default: the value of the log is the *sequence* — which methods
 * flew, in what order, which way — and a wall of expanded JSON destroys it. The
 * payload is one click away for the frame that turns out to matter.
 *
 * Two lines because the method is the row's payload and deserves the full
 * width: the direction arrow and method sit on the first line, the kind and
 * timestamp tuck under them right-aligned, so a narrow pane truncates long
 * method names last instead of first.
 */
function CommsFrameRow({ frame }: { frame: CommsFrame }) {
  const pretty = useMemo(
    () => prettyPayload(frame.payload_json),
    [frame.payload_json],
  );
  const time = useMemo(() => formatFrameTime(frame.at_ms), [frame.at_ms]);
  // Delta's own answer to a server request names no method; the kind is then the
  // only thing there is to show, and showing it beats an empty cell.
  const label = frame.method ?? `(${frame.kind})`;
  // Which way this frame went. Read once: the arrow, its colour and the
  // screen-reader text below all say the same thing.
  const toAgent = frame.direction === 'to_agent';

  return (
    <li
      data-testid="comms-frame"
      data-direction={frame.direction}
      data-kind={frame.kind}
      className="border-b border-border-default last:border-b-0"
    >
      <details className="group">
        <summary className="cursor-pointer px-2 py-1 font-mono text-caption hover:bg-surface-elevated">
          <span className="flex items-baseline gap-2">
            <span
              aria-hidden="true"
              className={
                toAgent ? 'shrink-0 text-accent' : 'shrink-0 text-fg-subtle'
              }
            >
              {toAgent ? '→' : '←'}
            </span>
            {/* The direction in words, for anyone who cannot rely on the glyph. */}
            <span className="sr-only">
              {toAgent ? 'sent to agent' : 'received from agent'}
            </span>
            <span
              data-testid="comms-frame-method"
              className="min-w-0 flex-1 truncate text-fg"
            >
              {label}
            </span>
          </span>
          <span className="flex items-baseline justify-end gap-2 text-fg-subtle">
            <span>{frame.kind}</span>
            <span>{time}</span>
          </span>
        </summary>
        <pre className="overflow-x-auto whitespace-pre-wrap break-all bg-surface-elevated px-2 py-1 font-mono text-caption text-fg-muted">
          {pretty}
        </pre>
      </details>
    </li>
  );
}

/**
 * Pretty-print a frame's payload. A payload that does not parse is shown
 * verbatim: the point of an inspector is to show what actually arrived, so
 * unparseable text is information, not an error to swallow.
 */
export function prettyPayload(payloadJson: string): string {
  try {
    return JSON.stringify(JSON.parse(payloadJson), null, 2);
  } catch {
    return payloadJson;
  }
}

/**
 * A frame's wall-clock time, to the millisecond — the resolution that makes two
 * frames in the same turn tellable apart. Fixed 24-hour form (`14:47:51.246`):
 * a 12-hour locale rendering would wedge its AM/PM marker between the seconds
 * and the appended milliseconds (`11:47:51 AM.246`), and a log column wants a
 * fixed width anyway.
 */
export function formatFrameTime(atMs: number): string {
  const date = new Date(atMs);
  if (Number.isNaN(date.getTime())) {
    return '';
  }
  const time = date.toLocaleTimeString([], {
    hourCycle: 'h23',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
  return `${time}.${String(date.getMilliseconds()).padStart(3, '0')}`;
}
