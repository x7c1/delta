import type { SessionId } from '@delta/model';
import type { SessionListItem } from '@delta/wire-gen';

/**
 * The navigator's display order: the sessions this window launched and still
 * tracks first, then every other session in the server's open-first order.
 *
 * `launchedIds` are the tracked spawns (see `SpawnItem`), newest launch first.
 * A launch is tracked from the moment its first send is accepted until it binds
 * (`session_registered`), or — when it fails — until the user retries or
 * removes it. Pinning those cards to the top keeps the one the user just
 * started where they are looking for the whole of its launch: a failed launch
 * belongs to the closed group in the server's order, below every open session,
 * so with enough of them open it would otherwise turn failed somewhere out of
 * view. A launch that binds is released and drops back into the server's
 * order, where its first turn's activity already ranks it near the top.
 *
 * Only a card the list still shows as launching or failed is pinned. The
 * registry is released by events, and the live stream does not replay what it
 * missed while reconnecting: a launch whose `session_registered` was lost stays
 * tracked as spawning, but its row comes back `active` on the resync refetch,
 * and that row returns to the server's order instead of staying pinned for the
 * rest of the page's life.
 *
 * Every other session keeps its relative order. A launched id that is not in
 * the loaded pages is skipped — there is no card to move — and a session the
 * page walk listed twice (see `WorkspaceScreen`) is pinned once.
 */
export function launchesFirst(
  sessions: SessionListItem[],
  launchedIds: readonly SessionId[],
): SessionListItem[] {
  if (launchedIds.length === 0) {
    return sessions;
  }
  const launched = new Set(launchedIds);
  const byId = new Map<SessionId, SessionListItem>();
  const rest: SessionListItem[] = [];
  for (const item of sessions) {
    if (!launched.has(item.session.id) || !isLaunching(item)) {
      rest.push(item);
    } else if (!byId.has(item.session.id)) {
      byId.set(item.session.id, item);
    }
  }
  if (byId.size === 0) {
    return sessions;
  }
  const pinned = launchedIds.flatMap((id) => {
    const item = byId.get(id);
    return item ? [item] : [];
  });
  return [...pinned, ...rest];
}

/** Whether the row is still a launch: starting, or ended without binding. */
function isLaunching(item: SessionListItem): boolean {
  return item.session.status === 'spawning' || item.session.status === 'failed';
}
