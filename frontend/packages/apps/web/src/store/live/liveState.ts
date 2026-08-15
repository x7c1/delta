import type { SessionEvent } from '@delta/wire-gen';
import type { ClonesSlice } from './clonesSlice';
import type { ConnectionSlice } from './connectionSlice';
import type { SendsSlice } from './sendsSlice';
import type { SpawnsSlice } from './spawnsSlice';
import type { RunningThreadsSlice } from './runningThreadsSlice';
import type { NoticesSlice } from './noticesSlice';
import type { UnreadSlice } from './unreadSlice';
import type { StreamingSlice } from './streamingSlice';
import type { SubagentsSlice } from './subagentsSlice';
import type { StatusSlice } from './statusSlice';
import type { TurnLifecycleSlice } from './turnLifecycleSlice';

/**
 * The event-application entry point, implemented in `eventReducers.ts`. Its
 * interface is declared here (not in `eventReducers.ts`) so this module's
 * imports stay type-only slice interfaces: `eventReducers.ts` imports the
 * reducer FUNCTIONS from the slice modules at runtime, so a type edge from
 * here back into it would close a cycle through those runtime edges.
 */
export interface EventsSlice {
  /**
   * Apply a live session event, mutating only session-scoped ephemeral state
   * (turn tracking, the spawn registry, the permission notice). Focus-dependent
   * signals (the external-input notice, unread badges) are recorded by the
   * router under a focus guard, not here.
   */
  applyEvent: (event: SessionEvent) => void;
}

/**
 * The complete live-store state: the per-family slices composed into the one
 * store `useLiveStore` creates. Each family (its state fields, its actions,
 * and its event reducers) lives in its own `./*Slice.ts` module; this type is
 * the composition only the assembling modules (`liveStore.ts`,
 * `eventReducers.ts`) are typed against.
 *
 * The slice modules themselves never reference this type — each is typed
 * against the narrow state it actually touches. That is a structural
 * requirement, not just taste: this module type-imports every slice, so a
 * slice referencing `LiveState` (even type-only) would close a cycle through
 * any runtime edge between slices (e.g. `spawnsSlice` calling a `noticesSlice`
 * helper), which the dependency linter rejects for non-type-only edges.
 */
export type LiveState = ConnectionSlice &
  SendsSlice &
  SpawnsSlice &
  RunningThreadsSlice &
  NoticesSlice &
  UnreadSlice &
  StreamingSlice &
  SubagentsSlice &
  StatusSlice &
  TurnLifecycleSlice &
  ClonesSlice &
  EventsSlice;
