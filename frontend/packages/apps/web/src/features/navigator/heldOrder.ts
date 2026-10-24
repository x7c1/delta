import type { SessionId } from '@delta/model';
import type { SessionListItem } from '@delta/wire-gen';

/**
 * The navigator's order while the mouse is over the list: the cards in
 * `heldIds`, the order they were shown in when the pointer arrived, followed by
 * any session listed since, in the live order.
 *
 * The live order moves a session up whenever it has activity (see
 * `launchesFirst`), so a card can slide away just as the user reaches for it.
 * Holding the order keeps every card the pointer can see where it was; the rows
 * take their content from `rows`, so a held card still shows its latest state.
 * A session listed since the hold began goes below the held ones, where it
 * displaces nothing, and a held session no longer listed is dropped. Like
 * `launchesFirst`, a held session the page walk listed twice is placed once.
 */
export function heldOrder(
  rows: SessionListItem[],
  heldIds: readonly SessionId[],
): SessionListItem[] {
  const byId = new Map<SessionId, SessionListItem>();
  for (const item of rows) {
    if (!byId.has(item.session.id)) {
      byId.set(item.session.id, item);
    }
  }
  const held = new Set<SessionId>();
  const ordered: SessionListItem[] = [];
  for (const id of heldIds) {
    const item = byId.get(id);
    if (item && !held.has(id)) {
      held.add(id);
      ordered.push(item);
    }
  }
  for (const item of rows) {
    if (!held.has(item.session.id)) {
      ordered.push(item);
    }
  }
  return ordered;
}
