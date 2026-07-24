import type { StateCreator } from 'zustand';
import type { SessionEvent } from '@delta/wire-gen';
import type { EventsSlice, LiveState } from './liveState';
import {
  reduceExternalInput,
  reducePermissionRequested,
  reducePermissionResolved,
  reduceQuestionAsked,
  reduceSessionOpened,
  reduceSessionRegistered,
} from './noticesSlice';
import { reduceTurnStarted } from './runningThreadsSlice';
import { reduceSpawnFailed } from './spawnsSlice';
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
 * of {@link LiveState}). A kind with no entry (`send_dispatched`,
 * `transcript_updated`) never touches live-store state — those events only
 * drive query-cache refetches in `applySessionEvent` — so `applyEvent` keeps
 * the identity-stable state for them.
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
  session_registered: reduceSessionRegistered,
  session_opened: reduceSessionOpened,
  session_closed: reduceSessionClosed,
  status_updated: reduceStatusUpdated,
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
