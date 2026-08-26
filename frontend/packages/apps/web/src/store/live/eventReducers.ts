import type { StateCreator } from 'zustand';
import type { SessionEvent } from '@delta/wire-gen';
import type { EventsSlice, LiveState } from './liveState';
import {
  reduceRepositoryCloneCompleted,
  reduceRepositoryCloneFailed,
} from './clonesSlice';
import {
  reduceExternalInput,
  reducePermissionRequested,
  reducePermissionResolved,
  reduceQuestionAsked,
  reduceSendDispatched,
  reduceSendParked,
  reduceSessionOpened,
  reduceSessionRegistered,
} from './noticesSlice';
import { reduceTurnStarted } from './runningThreadsSlice';
import {
  reduceSessionRegistered as reduceSpawnRegistered,
  reduceSpawnFailed,
} from './spawnsSlice';
import { reduceStatusUpdated } from './statusSlice';
import { reduceAssistantStreaming } from './streamingSlice';
import {
  reduceSubagentFinished,
  reduceSubagentStarted,
} from './subagentsSlice';
import {
  reduceSessionClosed,
  reduceTurnCompleted,
  reduceTurnInterrupted,
} from './turnLifecycleSlice';

/**
 * The per-event-kind reducers, each defined next to the slice whose state it
 * owns (and assignable here because its narrow state is a structural subset
 * of {@link LiveState}). A kind with no entry (`transcript_updated`) never
 * touches live-store state — those events only drive query-cache refetches in
 * `applySessionEvent` — so `applyEvent` keeps the identity-stable state for
 * them.
 */
/**
 * A reducer as the dispatch map sees it: over the full store state. Written
 * out structurally (rather than as `EventReducer<LiveState, K>`) on purpose —
 * `EventReducer`'s state parameter is invariant (it appears in both parameter
 * and return position), so TypeScript would reject the narrow-state reducers
 * when comparing two instantiations of the SAME alias; against this separate
 * alias it checks structurally, where the narrow reducers are sound (their
 * state is a subset of {@link LiveState}, so contravariance on the parameter
 * and `Partial` covariance on the result both hold).
 */
type StoreEventReducer<K extends SessionEvent['kind']> = (
  state: LiveState,
  event: Extract<SessionEvent, { kind: K }>,
) => Partial<LiveState> | LiveState;

/**
 * Run several reducers for the same event kind, left to right, and merge what
 * each of them changed.
 *
 * One event can move state two slices own — `session_registered` sweeps the
 * session's notices AND releases its tracked spawn — and the dispatch map holds
 * exactly one entry per kind, so the composition happens here rather than by
 * one slice reaching into another's state. Each reducer sees the state as the
 * ones before it left it, so an ordering dependency would still work; none
 * exists today. The identity-stable contract is preserved: when no reducer
 * changed anything the original `state` is handed back unchanged, so zustand
 * skips notifying subscribers.
 */
function chain<K extends SessionEvent['kind']>(
  ...reducers: StoreEventReducer<K>[]
): StoreEventReducer<K> {
  return (state, event) => {
    let changes: Partial<LiveState> | null = null;
    let current = state;
    for (const reducer of reducers) {
      const result: Partial<LiveState> = reducer(current, event);
      if (result === current) {
        continue;
      }
      changes = { ...(changes ?? {}), ...result };
      current = { ...current, ...result };
    }
    return changes ?? state;
  };
}

const EVENT_REDUCERS: {
  [K in SessionEvent['kind']]?: StoreEventReducer<K>;
} = {
  turn_started: reduceTurnStarted,
  turn_completed: reduceTurnCompleted,
  turn_interrupted: reduceTurnInterrupted,
  permission_requested: reducePermissionRequested,
  question_asked: reduceQuestionAsked,
  permission_resolved: reducePermissionResolved,
  spawn_failed: reduceSpawnFailed,
  assistant_streaming: reduceAssistantStreaming,
  subagent_started: reduceSubagentStarted,
  subagent_finished: reduceSubagentFinished,
  external_input: reduceExternalInput,
  send_dispatched: reduceSendDispatched,
  send_parked: reduceSendParked,
  session_registered: chain(reduceSessionRegistered, reduceSpawnRegistered),
  session_opened: reduceSessionOpened,
  session_closed: reduceSessionClosed,
  status_updated: reduceStatusUpdated,
  repository_clone_completed: reduceRepositoryCloneCompleted,
  repository_clone_failed: reduceRepositoryCloneFailed,
};

export const createEventsSlice: StateCreator<LiveState, [], [], EventsSlice> = (
  set,
) => ({
  applyEvent: (event) =>
    set((state) => {
      // The map is keyed by `kind`, so the reducer looked up under
      // `event.kind` accepts exactly this event. TypeScript cannot see that
      // key/value correlation across the union, hence the localized cast.
      const reducer = EVENT_REDUCERS[event.kind] as
        | StoreEventReducer<SessionEvent['kind']>
        | undefined;
      return reducer ? reducer(state, event) : state;
    }),
});
