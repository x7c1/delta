import { act } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import type { CommsFrame } from '@delta/wire-gen';
import type { SessionId } from '@delta/model';
import { useNavStore } from '../../store/navStore';
import {
  CommsLogPane,
  formatFrameTime,
  prettyPayload,
} from './CommsLogPane';

/**
 * The comms-log pane in isolation.
 *
 * Two things matter here and are tested directly:
 *
 * - the **states a session can be in** — live, closed, and not-yet-bound — each
 *   resolve to a pane that says what is going on, never a spinner that never
 *   ends and never a blank box;
 * - a delivered frame renders with its **direction and method visible at a
 *   glance** and its payload inspectable, which is the whole point of the pane.
 *
 * The WebSocket is replaced at the client seam (`connectCommsLog`), so frames are
 * delivered synchronously and no real socket is opened in jsdom.
 */

/** The options the component passed to `connectCommsLog`, for driving frames. */
let connected: {
  sessionId: string;
  onFrame: (frame: CommsFrame) => void;
  onStatus?: (status: 'open' | 'closed') => void;
  onError?: (error: unknown) => void;
} | null = null;
let closeCalls = 0;

vi.mock('@delta/api-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@delta/api-client')>();
  return {
    ...actual,
    connectCommsLog: (options: NonNullable<typeof connected>) => {
      connected = options;
      return {
        close() {
          closeCalls += 1;
        },
      };
    },
  };
});

function frame(overrides: Partial<CommsFrame> = {}): CommsFrame {
  return {
    seq: 0,
    at_ms: Date.UTC(2026, 0, 1, 9, 30, 0),
    direction: 'to_agent',
    kind: 'request',
    method: 'turn/start',
    payload_json: '{"id":1,"method":"turn/start"}',
    ...overrides,
  };
}

/** Deliver frames through the captured client seam. */
function deliver(...frames: CommsFrame[]): void {
  act(() => {
    for (const one of frames) {
      connected?.onFrame(one);
    }
  });
}

beforeEach(() => {
  connected = null;
  closeCalls = 0;
  useNavStore.setState({ commsOpen: true });
});

describe('CommsLogPane', () => {
  it('subscribes to the focused session and renders its frames in order', () => {
    render(<CommsLogPane sessionId={'sess-1' as SessionId} attachable />);
    expect(connected?.sessionId).toBe('sess-1');

    deliver(
      frame({ seq: 0, method: 'thread/start' }),
      frame({
        seq: 1,
        direction: 'from_agent',
        kind: 'response',
        method: 'thread/start',
      }),
      frame({
        seq: 2,
        direction: 'from_agent',
        kind: 'notification',
        method: 'turn/completed',
      }),
    );

    const rows = screen.getAllByTestId('comms-frame');
    expect(rows).toHaveLength(3);
    // Server order is display order: the sequence is what the log is for.
    expect(
      rows.map((row) =>
        row.querySelector('[data-testid="comms-frame-method"]')?.textContent,
      ),
    ).toEqual(['thread/start', 'thread/start', 'turn/completed']);
    // Direction and kind are on the row itself, so both are readable at a
    // glance (and assertable) without expanding anything.
    expect(rows.map((row) => row.dataset.direction)).toEqual([
      'to_agent',
      'from_agent',
      'from_agent',
    ]);
    expect(rows.map((row) => row.dataset.kind)).toEqual([
      'request',
      'response',
      'notification',
    ]);
  });

  it('renders the payload pretty-printed and collapsed', () => {
    render(<CommsLogPane sessionId={'sess-1' as SessionId} attachable />);
    deliver(frame({ payload_json: '{"a":{"b":1}}' }));

    const row = screen.getByTestId('comms-frame');
    const details = row.querySelector('details');
    expect(details?.open).toBe(false);
    // Collapsed by default — the sequence is the signal, and expanded JSON
    // would bury it — but the payload is present and formatted for reading.
    expect(details?.querySelector('pre')?.textContent).toBe(
      '{\n  "a": {\n    "b": 1\n  }\n}',
    );
  });

  it('labels a methodless frame by its kind rather than leaving it blank', () => {
    // Delta's own answer to a server request names no method; the row must still
    // say what it is.
    render(<CommsLogPane sessionId={'sess-1' as SessionId} attachable />);
    deliver(frame({ method: null, kind: 'response' }));

    expect(screen.getByTestId('comms-frame-method')).toHaveTextContent(
      '(response)',
    );
  });

  it('shows a waiting state once connected with no frames yet', () => {
    render(<CommsLogPane sessionId={'sess-1' as SessionId} attachable />);
    act(() => connected?.onStatus?.('open'));

    expect(screen.getByTestId('comms-empty-note')).toHaveTextContent(
      /Waiting for the next frame/,
    );
    expect(screen.queryAllByTestId('comms-frame')).toHaveLength(0);
  });

  it('shows an idle state for a closed session and opens no socket', () => {
    // A closed session has no live wire; the server discarded its buffer when it
    // closed. The pane must say so — not spin forever.
    render(
      <CommsLogPane sessionId={'sess-1' as SessionId} attachable={false} />,
    );

    expect(connected).toBeNull();
    expect(screen.getByTestId('comms-empty-note')).toHaveTextContent(
      /This session is closed/,
    );
  });

  it('shows an idle state with no session bound yet and opens no socket', () => {
    render(<CommsLogPane sessionId={null} attachable={false} />);

    expect(connected).toBeNull();
    expect(screen.getByTestId('comms-empty-note')).toHaveTextContent(
      /No session is attached yet/,
    );
  });

  it('closes the stream when the pane unmounts', () => {
    const view = render(
      <CommsLogPane sessionId={'sess-1' as SessionId} attachable />,
    );
    expect(closeCalls).toBe(0);
    view.unmount();
    expect(closeCalls).toBe(1);
  });

  it('starts a fresh log when the focused session changes', () => {
    // Two sessions' frames must never mix: the pane resets and re-subscribes, so
    // the new session starts from ITS server-side replay.
    const view = render(
      <CommsLogPane sessionId={'sess-1' as SessionId} attachable />,
    );
    deliver(frame({ seq: 0, method: 'turn/start' }));
    expect(screen.getAllByTestId('comms-frame')).toHaveLength(1);

    view.rerender(
      <CommsLogPane sessionId={'sess-2' as SessionId} attachable />,
    );

    expect(connected?.sessionId).toBe('sess-2');
    expect(screen.queryAllByTestId('comms-frame')).toHaveLength(0);
  });

  it('drops the previous session’s frames when focus moves to a closed session', () => {
    // The other half of the reset above, and the one a user actually hits by
    // clicking a dormant session in the navigator: no socket is opened for it, so
    // if the reset only happened on the connecting path the closed session would
    // display ANOTHER session's frames under a note saying nothing is being
    // exchanged.
    const view = render(
      <CommsLogPane sessionId={'sess-1' as SessionId} attachable />,
    );
    deliver(frame({ seq: 0, method: 'turn/start' }));
    expect(screen.getAllByTestId('comms-frame')).toHaveLength(1);

    view.rerender(
      <CommsLogPane sessionId={'sess-2' as SessionId} attachable={false} />,
    );

    expect(screen.queryAllByTestId('comms-frame')).toHaveLength(0);
    expect(screen.getByTestId('comms-empty-note')).toHaveTextContent(
      /This session is closed/,
    );
  });

  it('follows the tail as frames arrive, and stops once the reader scrolls up', () => {
    render(<CommsLogPane sessionId={'sess-1' as SessionId} attachable />);
    // The Panel's scrollable body is the pane's own parent element. jsdom lays
    // nothing out (and hardcodes `scrollTop` to 0), so the three metrics the
    // follow logic reads and writes are stubbed here.
    const body = screen.getByTestId('comms-pane').parentElement;
    if (body === null) {
      throw new Error('the pane is not inside a Panel body');
    }
    let scrollTop = 0;
    Object.defineProperty(body, 'scrollTop', {
      configurable: true,
      get: () => scrollTop,
      set: (value: number) => {
        scrollTop = value;
      },
    });
    Object.defineProperty(body, 'scrollHeight', {
      configurable: true,
      value: 1000,
    });
    Object.defineProperty(body, 'clientHeight', { configurable: true, value: 100 });

    // A pane opened mid-session replays hundreds of frames; the newest end is the
    // one worth looking at, so the view lands there rather than on the oldest.
    deliver(frame({ seq: 0 }));
    expect(body.scrollTop).toBe(1000);

    // The reader scrolls up to study an earlier frame — following stops, so the
    // next frame no longer yanks the view away from what they are reading.
    body.scrollTop = 200;
    fireEvent.scroll(body);
    deliver(frame({ seq: 1 }));
    expect(body.scrollTop).toBe(200);
  });

  it('closes the pane from its close button', () => {
    render(<CommsLogPane sessionId={'sess-1' as SessionId} attachable />);
    screen.getByLabelText('Close communication log').click();
    expect(useNavStore.getState().commsOpen).toBe(false);
  });
});

describe('prettyPayload', () => {
  it('pretty-prints valid JSON', () => {
    expect(prettyPayload('{"a":1}')).toBe('{\n  "a": 1\n}');
  });

  it('shows unparseable text verbatim', () => {
    // An inspector's job is to show what arrived; hiding it would remove the one
    // clue a wire-level bug leaves behind.
    expect(prettyPayload('<not json>')).toBe('<not json>');
  });
});

describe('formatFrameTime', () => {
  it('includes milliseconds, so two frames in one turn stay tellable apart', () => {
    const at = new Date(2026, 0, 1, 9, 30, 12, 45).getTime();
    expect(formatFrameTime(at)).toMatch(/\.045$/);
  });

  it('uses a fixed 24-hour clock — a 12-hour locale would wedge AM/PM between the seconds and the milliseconds', () => {
    const afternoon = new Date(2026, 0, 1, 14, 47, 51, 246).getTime();
    expect(formatFrameTime(afternoon)).toBe('14:47:51.246');
  });

  it('renders an unusable timestamp as empty rather than "Invalid Date"', () => {
    expect(formatFrameTime(Number.NaN)).toBe('');
  });
});
