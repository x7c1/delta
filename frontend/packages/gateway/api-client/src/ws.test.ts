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

  it('returns null for an unknown kind', () => {
    expect(parseSessionEvent(JSON.stringify({ kind: 'bogus' }))).toBeNull();
  });

  it('returns null for malformed JSON', () => {
    expect(parseSessionEvent('not json')).toBeNull();
  });
});
