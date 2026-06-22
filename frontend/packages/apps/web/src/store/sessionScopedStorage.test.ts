import { renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';
import {
  SESSION_SCOPED_PREFIX,
  garbageCollectSessionScopedStorage,
  parseSessionScopedKey,
  readSessionScoped,
  sessionScopedKey,
  useGarbageCollectSessionScopedStorage,
  writeSessionScoped,
} from './sessionScopedStorage';

// Realistic UUID v7 session ids — what the backend issues — so the round-trip
// coverage matches production-shape keys. Each id contains hyphens but no
// `.`, which is what the helper's `.` delimiter relies on.
const SESSION_A = '019edfea-bede-75f1-8825-72333d787342';
const SESSION_B = '019edfea-c0de-7000-9999-aaaaaaaaaaaa';
const SESSION_C = '019edfea-cafe-7000-1111-bbbbbbbbbbbb';

describe('sessionScopedKey / parseSessionScopedKey', () => {
  it('composes a key under the shared prefix and round-trips through the parser', () => {
    const key = sessionScopedKey(SESSION_A, 'thread-timeline-overlay.expanded');
    expect(key.startsWith(SESSION_SCOPED_PREFIX)).toBe(true);
    expect(parseSessionScopedKey(key)).toEqual({
      sessionId: SESSION_A,
      subKey: 'thread-timeline-overlay.expanded',
    });
  });

  it('extracts the session id correctly even when the sub-key itself contains dots', () => {
    // Real callers use dotted sub-keys (e.g. `thread-timeline-overlay.expanded`);
    // the parser splits on the FIRST delimiter so the id stays a single
    // hex/hyphen blob and the rest is the sub-key.
    const key = sessionScopedKey(SESSION_A, 'a.b.c');
    expect(parseSessionScopedKey(key)).toEqual({
      sessionId: SESSION_A,
      subKey: 'a.b.c',
    });
  });

  it('returns null for keys that do not match the prefix', () => {
    expect(parseSessionScopedKey('delta-nav')).toBeNull();
    expect(parseSessionScopedKey('delta:status-snapshot')).toBeNull();
    expect(parseSessionScopedKey('completely-unrelated')).toBeNull();
  });

  it('returns null for a malformed entry (bare prefix, missing sub-key, etc.)', () => {
    expect(parseSessionScopedKey(SESSION_SCOPED_PREFIX)).toBeNull();
    expect(parseSessionScopedKey(`${SESSION_SCOPED_PREFIX}.`)).toBeNull();
    expect(parseSessionScopedKey(`${SESSION_SCOPED_PREFIX}${SESSION_A}`)).toBeNull();
    expect(parseSessionScopedKey(`${SESSION_SCOPED_PREFIX}${SESSION_A}.`)).toBeNull();
  });
});

describe('readSessionScoped / writeSessionScoped', () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it('round-trips a boolean preference per session', () => {
    writeSessionScoped(SESSION_A, 'expanded', true);
    writeSessionScoped(SESSION_B, 'expanded', false);
    expect(readSessionScoped(SESSION_A, 'expanded', (raw) => raw === 'true')).toBe(
      true,
    );
    expect(readSessionScoped(SESSION_B, 'expanded', (raw) => raw === 'true')).toBe(
      false,
    );
  });

  it('returns null when no value has been written for that (session, sub-key) pair', () => {
    writeSessionScoped(SESSION_A, 'expanded', true);
    expect(
      readSessionScoped(SESSION_A, 'something-else', (raw) => raw === 'true'),
    ).toBeNull();
    expect(
      readSessionScoped(SESSION_B, 'expanded', (raw) => raw === 'true'),
    ).toBeNull();
  });

  it('returns null when the stored value cannot be parsed', () => {
    // A future code change might change the encoding of a sub-key. Until it
    // is migrated, the unparseable value should fall back to null rather
    // than crash a render that wraps the call.
    window.localStorage.setItem(sessionScopedKey(SESSION_A, 'json'), 'not-json');
    expect(readSessionScoped(SESSION_A, 'json', JSON.parse)).toBeNull();
  });

  it('uses the supplied serializer for the write path', () => {
    writeSessionScoped(
      SESSION_A,
      'json',
      { theme: 'dark', density: 'compact' },
      JSON.stringify,
    );
    expect(
      readSessionScoped(SESSION_A, 'json', JSON.parse),
    ).toEqual({ theme: 'dark', density: 'compact' });
  });
});

describe('garbageCollectSessionScopedStorage', () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it('drops keys whose session id is not in the known set', () => {
    writeSessionScoped(SESSION_A, 'expanded', true);
    writeSessionScoped(SESSION_B, 'expanded', true);
    writeSessionScoped(SESSION_C, 'expanded', true);

    const removed = garbageCollectSessionScopedStorage(new Set([SESSION_A, SESSION_C]));
    expect(removed).toBe(1);
    expect(window.localStorage.getItem(sessionScopedKey(SESSION_A, 'expanded'))).toBe(
      'true',
    );
    expect(window.localStorage.getItem(sessionScopedKey(SESSION_B, 'expanded'))).toBeNull();
    expect(window.localStorage.getItem(sessionScopedKey(SESSION_C, 'expanded'))).toBe(
      'true',
    );
  });

  it('preserves every key when every session id is still known', () => {
    writeSessionScoped(SESSION_A, 'expanded', true);
    writeSessionScoped(SESSION_B, 'expanded', false);
    const removed = garbageCollectSessionScopedStorage(
      new Set([SESSION_A, SESSION_B]),
    );
    expect(removed).toBe(0);
    expect(window.localStorage.getItem(sessionScopedKey(SESSION_A, 'expanded'))).toBe(
      'true',
    );
    expect(window.localStorage.getItem(sessionScopedKey(SESSION_B, 'expanded'))).toBe(
      'false',
    );
  });

  it('does not touch unrelated keys', () => {
    // The other localStorage citizens (navStore, statusPersistence) carry
    // their own prefixes and must be left strictly alone by the GC.
    window.localStorage.setItem('delta-nav', '{"focusedSessionId":null}');
    window.localStorage.setItem('delta:status-snapshot', '{"savedAt":0}');
    writeSessionScoped(SESSION_A, 'expanded', true);

    garbageCollectSessionScopedStorage(new Set<string>());

    expect(window.localStorage.getItem('delta-nav')).toBe('{"focusedSessionId":null}');
    expect(window.localStorage.getItem('delta:status-snapshot')).toBe(
      '{"savedAt":0}',
    );
    expect(
      window.localStorage.getItem(sessionScopedKey(SESSION_A, 'expanded')),
    ).toBeNull();
  });

  it('is a no-op when no keys match the session-scoped prefix', () => {
    window.localStorage.setItem('delta-nav', '{}');
    const removed = garbageCollectSessionScopedStorage(new Set([SESSION_A]));
    expect(removed).toBe(0);
    expect(window.localStorage.getItem('delta-nav')).toBe('{}');
  });

  it('drops multiple sub-keys for the same orphan session id in one sweep', () => {
    writeSessionScoped(SESSION_A, 'expanded', true);
    writeSessionScoped(SESSION_A, 'someOtherPref', 'x');
    writeSessionScoped(SESSION_B, 'expanded', true);

    const removed = garbageCollectSessionScopedStorage(new Set([SESSION_B]));
    expect(removed).toBe(2);
    expect(window.localStorage.getItem(sessionScopedKey(SESSION_A, 'expanded'))).toBeNull();
    expect(window.localStorage.getItem(sessionScopedKey(SESSION_A, 'someOtherPref'))).toBeNull();
    expect(window.localStorage.getItem(sessionScopedKey(SESSION_B, 'expanded'))).toBe(
      'true',
    );
  });
});

describe('useGarbageCollectSessionScopedStorage', () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it('drops orphan keys once the session list is supplied', () => {
    writeSessionScoped(SESSION_A, 'expanded', true); // still known
    writeSessionScoped(SESSION_B, 'expanded', true); // orphan

    renderHook(({ ids }) => useGarbageCollectSessionScopedStorage(ids), {
      initialProps: { ids: [SESSION_A] as readonly string[] | null },
    });

    expect(
      window.localStorage.getItem(sessionScopedKey(SESSION_A, 'expanded')),
    ).toBe('true');
    expect(
      window.localStorage.getItem(sessionScopedKey(SESSION_B, 'expanded')),
    ).toBeNull();
  });

  it('does not sweep while the session list is still loading (ids === null)', () => {
    writeSessionScoped(SESSION_A, 'expanded', true);
    renderHook(({ ids }) => useGarbageCollectSessionScopedStorage(ids), {
      initialProps: { ids: null as readonly string[] | null },
    });
    // No sweep ran, so even an "orphan-looking" key (nothing is known yet) is
    // preserved through the loading window.
    expect(
      window.localStorage.getItem(sessionScopedKey(SESSION_A, 'expanded')),
    ).toBe('true');
  });

  it('re-sweeps when the session-id set changes', () => {
    writeSessionScoped(SESSION_A, 'expanded', true);
    writeSessionScoped(SESSION_B, 'expanded', true);

    const { rerender } = renderHook(
      ({ ids }) => useGarbageCollectSessionScopedStorage(ids),
      {
        initialProps: {
          ids: [SESSION_A, SESSION_B] as readonly string[] | null,
        },
      },
    );

    // First sweep: both are known, nothing dropped.
    expect(
      window.localStorage.getItem(sessionScopedKey(SESSION_A, 'expanded')),
    ).toBe('true');
    expect(
      window.localStorage.getItem(sessionScopedKey(SESSION_B, 'expanded')),
    ).toBe('true');

    // Session B was just removed from the list (deleted from another device,
    // for instance). The next render with the shrunk list should re-sweep.
    rerender({ ids: [SESSION_A] });
    expect(
      window.localStorage.getItem(sessionScopedKey(SESSION_A, 'expanded')),
    ).toBe('true');
    expect(
      window.localStorage.getItem(sessionScopedKey(SESSION_B, 'expanded')),
    ).toBeNull();
  });
});
