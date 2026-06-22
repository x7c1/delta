import { useEffect, useRef } from 'react';

/**
 * Helper for per-session preferences persisted to `localStorage`.
 *
 * UI preferences keyed by session id (e.g. the timeline footer's
 * expand/collapse state) accumulate one localStorage key per session over a
 * device's lifetime. The keys are tiny individually but, left alone, they
 * pile up forever — sessions deleted from another device, sessions wiped from
 * SQLite by hand, sessions removed while disconnected — every loss path leaks
 * an orphan key. This helper provides:
 *
 *  - a uniform `delta.session.<sessionId>.<subKey>` shape, so a startup scan
 *    can recognise session-scoped keys by pure string prefix;
 *  - best-effort `read` / `write` with the storage layer's failure modes
 *    (quota, privacy mode, SSR) swallowed — exactly the style of
 *    {@link statusPersistence}, but parametrised per session;
 *  - a lazy garbage-collector that drops orphan keys once the app knows the
 *    list of live session ids, plus a React hook that runs it idempotently
 *    next to the session-list fetch (see {@link useGarbageCollectSessionScopedStorage}).
 *
 * Direct `localStorage.setItem` against session-id-bearing keys is
 * deliberately discouraged — all per-session preferences should pass through
 * this module so the GC stays effective and the shape stays consistent.
 */

/**
 * Prefix every session-scoped key carries. Kept as a `delta.session.`
 * namespace so the GC's "is this one of ours?" check is a pure string prefix
 * test, and so DevTools shows every session-keyed entry grouped together.
 * Exported so tests can scan / clear matching keys without re-deriving the
 * literal.
 */
export const SESSION_SCOPED_PREFIX = 'delta.session.';

/**
 * Delimiter between the session id and the per-feature sub-key inside the
 * composed key. `.` is safe because delta's session ids are UUIDs (v7),
 * which contain only hex chars and `-` — see
 * `delta-usecase/.../routing.rs`. If the id shape ever changes to allow
 * `.` this delimiter must change in lockstep (the parser would otherwise
 * mis-extract the id).
 */
const KEY_DELIMITER = '.';

/**
 * Compose the storage key for a `(sessionId, subKey)` pair. Exported so the
 * (rare) callers that need to interact with the raw key (e.g. tests stubbing
 * `localStorage` directly) can stay aligned with the helper without
 * duplicating the literal.
 */
export function sessionScopedKey(sessionId: string, subKey: string): string {
  return `${SESSION_SCOPED_PREFIX}${sessionId}${KEY_DELIMITER}${subKey}`;
}

/**
 * Extract the `(sessionId, subKey)` pair from a composed key. Returns `null`
 * when the key does not match the session-scoped shape (so the GC ignores
 * unrelated keys like `delta-nav` or `delta:status-snapshot`).
 *
 * Exported for tests; the GC is the only runtime caller.
 */
export function parseSessionScopedKey(
  rawKey: string,
): { sessionId: string; subKey: string } | null {
  if (!rawKey.startsWith(SESSION_SCOPED_PREFIX)) {
    return null;
  }
  const remainder = rawKey.slice(SESSION_SCOPED_PREFIX.length);
  const delimIndex = remainder.indexOf(KEY_DELIMITER);
  // A bare prefix (no sub-key) is not a well-formed entry; skip it. Same for
  // a delimiter at position 0, which would yield an empty session id, or one
  // at the very end which would yield an empty sub-key.
  if (delimIndex <= 0 || delimIndex === remainder.length - 1) {
    return null;
  }
  return {
    sessionId: remainder.slice(0, delimIndex),
    subKey: remainder.slice(delimIndex + 1),
  };
}

/**
 * Best-effort read of a session-scoped value. Returns `null` on a missing
 * key, an unparseable value (the caller's `parse` throws), or a storage
 * failure (private mode, SSR). The caller picks the type by supplying the
 * `parse` callback — for a boolean preference, `(raw) => raw === 'true'`;
 * for JSON, `JSON.parse` works directly.
 */
export function readSessionScoped<T>(
  sessionId: string,
  subKey: string,
  parse: (raw: string) => T,
): T | null {
  if (typeof window === 'undefined') {
    return null;
  }
  try {
    const raw = window.localStorage.getItem(sessionScopedKey(sessionId, subKey));
    if (raw === null) {
      return null;
    }
    return parse(raw);
  } catch {
    return null;
  }
}

/**
 * Best-effort write. Failures (quota, private mode, SSR) are swallowed so a
 * preference write never crashes the UI — the value lives in memory for the
 * remainder of the page session and the next reload reads back whatever the
 * browser was able to persist.
 *
 * The `serialize` callback defaults to `String(value)`, which handles
 * booleans and numbers; JSON-shaped values should pass `JSON.stringify`
 * explicitly.
 */
export function writeSessionScoped<T>(
  sessionId: string,
  subKey: string,
  value: T,
  serialize: (value: T) => string = String,
): void {
  if (typeof window === 'undefined') {
    return;
  }
  try {
    window.localStorage.setItem(
      sessionScopedKey(sessionId, subKey),
      serialize(value),
    );
  } catch {
    // Storage may be unavailable (quota, privacy mode); ignore.
  }
}

/**
 * Scan `localStorage` for keys matching {@link SESSION_SCOPED_PREFIX} and
 * drop any whose session id is not in `knownSessionIds`. Idempotent — safe
 * to call multiple times. Storage failures are swallowed.
 *
 * Returns the number of keys removed, primarily so tests can assert the GC
 * did something without re-reading `localStorage`.
 */
export function garbageCollectSessionScopedStorage(
  knownSessionIds: ReadonlySet<string>,
): number {
  if (typeof window === 'undefined') {
    return 0;
  }
  let removed = 0;
  try {
    // Snapshot the key list first: `removeItem` shifts the iteration index,
    // so reading `localStorage.key(i)` inside the removal loop skips entries.
    const candidates: string[] = [];
    for (let i = 0; i < window.localStorage.length; i++) {
      const key = window.localStorage.key(i);
      if (key !== null) {
        candidates.push(key);
      }
    }
    for (const key of candidates) {
      const parsed = parseSessionScopedKey(key);
      if (parsed === null) {
        continue;
      }
      if (!knownSessionIds.has(parsed.sessionId)) {
        try {
          window.localStorage.removeItem(key);
          removed += 1;
        } catch {
          // A single failed removal must not abort the rest of the sweep.
        }
      }
    }
  } catch {
    // `localStorage.length` / `.key()` themselves can throw in privacy mode;
    // silent skip is consistent with the read/write best-effort policy.
  }
  return removed;
}

/**
 * React hook: run {@link garbageCollectSessionScopedStorage} once per unique
 * session-id set. Pass the current list of known session ids (or `null`
 * while still loading); the hook re-runs only when the set genuinely
 * changes — adding or removing one id triggers a sweep, while a re-render
 * with the same set is a no-op.
 *
 * Call this from a component that has already loaded the session list
 * (e.g. `WorkspaceScreen` after `useSessionsQuery`). The GC is an
 * app-shell concern, not a per-feature one, so feature components like the
 * timeline must not invoke it directly.
 */
export function useGarbageCollectSessionScopedStorage(
  sessionIds: readonly string[] | null,
): void {
  // Track the last set we swept against so re-renders with the same ids
  // skip the scan. A `null` argument (still loading) is also tracked so the
  // first load → first non-null transition triggers a sweep exactly once.
  const lastSweptSignatureRef = useRef<string | null>(null);

  useEffect(() => {
    if (sessionIds === null) {
      return;
    }
    // A stable signature so the dependency comparison stays cheap. The set
    // is built from the signature only after the change check passes.
    const signature = [...sessionIds].sort().join('\x00');
    if (lastSweptSignatureRef.current === signature) {
      return;
    }
    lastSweptSignatureRef.current = signature;
    garbageCollectSessionScopedStorage(new Set(sessionIds));
  }, [sessionIds]);
}
