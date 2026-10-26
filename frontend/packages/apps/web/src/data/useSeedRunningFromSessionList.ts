import { useEffect, useRef } from 'react';
import type { SessionListItem } from '@delta/wire-gen';
import { useLiveStore } from '../store/liveStore';

/**
 * Re-seed every listed session's running flags — its running thread
 * (`seedActiveTurn`) and its running subagents (`seedRunningSubagents`) — from
 * the session list's `turn` and `running_subagents` whenever a fresh list
 * lands.
 *
 * The running flags are otherwise built from events (`turn_started`,
 * `subagent_started`, …), so whenever the store loses them every session's
 * spinner is lost while its turn keeps running: on a page reload (the store
 * starts empty, and the list's first fetch is what restores it) and on a
 * live-stream reconnect (`resetTurnEphemera` drops every session's flags, and
 * the `invalidateAll` refetch of the list restores them). The sends envelope
 * re-seeds only the open session; the list covers every session.
 *
 * The same guard `useSendActivity` keeps against a stale read, adapted to a
 * list that seeds every session:
 *
 * - **Only a settled fetch seeds, and it seeds authoritatively.** While a
 *   refetch is in flight the rows on hand are the previous answer, which may
 *   predate a `turn_completed` that has since cleared the flag. Seeding from
 *   them — even set-only — would bring that spinner back. The turn and
 *   subagent events invalidate the list (see `applySessionEvent`), so a list
 *   requested before such an event is discarded rather than landed, and the
 *   answer that does land was computed after the server recorded the change.
 * - **Each fresh observation seeds once, keyed by `dataUpdatedAt`.** The
 *   timestamp is what re-triggers the seed when the payload is unchanged across
 *   the reconnect reset (structural sharing hands back the identical rows), and
 *   remembering the last one seeded keeps a refetch that FAILED — which settles
 *   back onto the old rows without a new timestamp — from reconciling the store
 *   to them again.
 *
 * For the open session this coexists with the `/sends` seed in
 * `useSendActivity`: both read the same live state on the server, both are
 * authoritative only from a settled fetch, and the same turn and subagent
 * events invalidate both, so whichever lands last was computed after the last
 * event and the two agree; between two events they report the same thing.
 */
export function useSeedRunningFromSessionList(
  sessions: readonly SessionListItem[],
  dataUpdatedAt: number,
  settled: boolean,
): void {
  const seededAt = useRef<number | null>(null);
  useEffect(() => {
    if (!settled || dataUpdatedAt === 0 || seededAt.current === dataUpdatedAt) {
      return;
    }
    seededAt.current = dataUpdatedAt;
    const store = useLiveStore.getState();
    for (const item of sessions) {
      store.seedActiveTurn(item.session.id, item.turn, true);
      store.seedRunningSubagents(item.session.id, item.running_subagents);
    }
  }, [sessions, dataUpdatedAt, settled]);
}
