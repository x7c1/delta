import { describe, expect, it } from 'vitest';
import { parseSessionEvent } from './ws';

describe('parseSessionEvent', () => {
  it('parses a known event kind', () => {
    const event = parseSessionEvent(
      JSON.stringify({
        kind: 'turn_started',
        session_id: 'sess-1',
        pending_send_id: 1,
        matched_uuid: 'uuid-1',
      }),
    );

    expect(event).toEqual({
      kind: 'turn_started',
      session_id: 'sess-1',
      pending_send_id: 1,
      matched_uuid: 'uuid-1',
    });
  });

  it('parses a transcript_updated event', () => {
    const event = parseSessionEvent(
      JSON.stringify({
        kind: 'transcript_updated',
        session_id: 'sess-1',
        thread_ids: [1, 4],
      }),
    );

    expect(event).toEqual({
      kind: 'transcript_updated',
      session_id: 'sess-1',
      thread_ids: [1, 4],
    });
  });

  it('parses the session_opened and session_closed lifecycle events', () => {
    expect(
      parseSessionEvent(
        JSON.stringify({ kind: 'session_opened', session_id: 'sess-1' }),
      ),
    ).toEqual({ kind: 'session_opened', session_id: 'sess-1' });

    expect(
      parseSessionEvent(
        JSON.stringify({ kind: 'session_closed', session_id: 'sess-1' }),
      ),
    ).toEqual({ kind: 'session_closed', session_id: 'sess-1' });
  });

  it('returns null for an unknown kind', () => {
    expect(parseSessionEvent(JSON.stringify({ kind: 'bogus' }))).toBeNull();
  });

  it('returns null for malformed JSON', () => {
    expect(parseSessionEvent('not json')).toBeNull();
  });
});
