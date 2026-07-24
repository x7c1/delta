import { create } from 'zustand';
import type { LiveState } from './live/liveState';
import { createConnectionSlice } from './live/connectionSlice';
import { createSendsSlice } from './live/sendsSlice';
import { createSpawnsSlice } from './live/spawnsSlice';
import { createRunningThreadsSlice } from './live/runningThreadsSlice';
import { createNoticesSlice } from './live/noticesSlice';
import { createUnreadSlice } from './live/unreadSlice';
import { createStreamingSlice } from './live/streamingSlice';
import { createSubagentsSlice } from './live/subagentsSlice';
import { createStatusSlice } from './live/statusSlice';
import { createTurnLifecycleSlice } from './live/turnLifecycleSlice';
import { createEventsSlice } from './live/eventReducers';

/**
 * Ephemeral, session-only live UI state that is NOT a REST resource. REST
 * resources (session, threads, messages, the open-send list) live in the
 * TanStack Query cache; this store holds only volatile UI signals derived from
 * the live channel and from optimistic local actions.
 *
 * The store is composed from per-family slices under `./live/` — one module
 * per state family (its fields, its actions, and the reducers for the live
 * events that touch it), plus `eventReducers.ts`, which routes each incoming
 * `SessionEvent` kind to its family's reducer. This module only composes the
 * slices and re-exports the public surface, so consumers keep importing
 * everything from `store/liveStore`.
 *
 * The pending-send strip is server-authoritative: its rows come from
 * `GET /api/sessions/{id}/sends` via the query cache. This store keeps only
 * the three client-side complements the server cannot provide yet:
 *
 * - {@link SendsSlice.sending} — a submit whose `POST /api/sends` has not
 *   resolved (the server knows nothing about it yet), or whose POST failed.
 * - {@link SendsSlice.localSends} — an accepted send tracked until its turn
 *   ends. A send leaves the server's open list the moment it correlates with
 *   its transcript line (status `matched`), which is usually *while* its turn
 *   is still running; this local twin keeps the chip visible until the
 *   `turn_completed`/`turn_interrupted` event actually lands.
 * - {@link SpawnsSlice.spawns} — a new-session spawn tracked from the POST
 *   response until it registers or fails. The server deletes a failed spawn's
 *   contentless row at reap, so the failure chip (and its Retry payload)
 *   cannot be server-rendered.
 *
 * Per-session notices (the permission prompt, the external-input marker, the
 * resume-impossible flag, the buffered early spawn failure) live in one
 * {@link NoticesSlice.notices} map as a discriminated union, with each kind's
 * clearing rule declared in {@link NOTICE_LIFECYCLE} — one add path, one clear
 * path, instead of a separate `Record<SessionId, …>` per kind.
 */

export type { LiveState } from './live/liveState';
export type { SendingItem, LocalSend } from './live/sendsSlice';
export type { SpawnItem } from './live/spawnsSlice';
export type { StreamingMessage } from './live/streamingSlice';
export type { SubagentActivity } from './live/subagentsSlice';
export { threadIsRunning } from './live/runningThreadsSlice';
export type {
  PermissionNotice,
  QuestionNotice,
  ExternalInputNotice,
  ResumeUnavailableNotice,
  SpawnFailureBufferedNotice,
  SessionNotice,
  SessionNoticeKind,
} from './live/noticesSlice';
export { noticeOf } from './live/noticesSlice';

export const useLiveStore = create<LiveState>()((...args) => ({
  ...createConnectionSlice(...args),
  ...createSendsSlice(...args),
  ...createSpawnsSlice(...args),
  ...createRunningThreadsSlice(...args),
  ...createNoticesSlice(...args),
  ...createUnreadSlice(...args),
  ...createStreamingSlice(...args),
  ...createSubagentsSlice(...args),
  ...createStatusSlice(...args),
  ...createTurnLifecycleSlice(...args),
  ...createEventsSlice(...args),
}));
