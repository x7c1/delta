import type { SessionEvent } from '@delta/wire-gen';

/**
 * One live-event reducer: given the state slice it reads and the event
 * narrowed to one `kind`, return the changed fields (or the identity-stable
 * `state` when nothing changed — the same contract the original `applyEvent`
 * switch cases had, which is what lets zustand skip notifying subscribers).
 *
 * `S` is the narrow state each reducer actually touches — a `Pick` of its own
 * slice (plus any sibling fields it reads) rather than the composed store
 * state. Keeping the slice modules off the composed type matters structurally:
 * it is what keeps every runtime edge between store modules acyclic (see
 * `liveState.ts`). Because the narrow types are structural subsets, every
 * reducer is assignable to `EventReducer<LiveState, K>` where the dispatch map
 * in `eventReducers.ts` collects them.
 */
export type EventReducer<S, K extends SessionEvent['kind']> = (
  state: S,
  event: Extract<SessionEvent, { kind: K }>,
) => Partial<S> | S;
