import type { ThreadId } from '@delta/model';
import type { Message, Thread } from '@delta/wire-gen';

/**
 * Number of leading characters of a subthread's `root_message_uuid` used as
 * its lane label. Picked from the 20–30 range called out in the spec — long
 * enough to disambiguate at a glance, short enough to fit in a compact label
 * column without truncation noise. The full uuid is exposed via the lane's
 * `tooltip` so the trimmed prefix never loses the underlying anchor.
 */
export const LANE_LABEL_PREFIX_LEN = 24;

/** Special label for the main thread's lane. */
export const MAIN_LANE_LABEL = 'main';

/**
 * A single message dot in a lane. The dot is rendered at index {@link order}
 * along the lane's horizontal axis with equal spacing between dots, so the
 * placement collapses wall-clock gaps and matches speech-sequence order only.
 */
export interface TimelineDot {
  /** The message uuid; drives hover-jump's `data-message-uuid` lookup. */
  uuid: string;
  /** The owning thread's id; mirrors {@link TimelineLane.threadId}. */
  threadId: ThreadId;
  /** Zero-based position within the lane's dots. */
  order: number;
}

/**
 * A swim lane in the timeline footer: one row per (sub)thread. Lanes are
 * sorted oldest → newest by the thread's `created_at`, matching the
 * navigator's tree order.
 */
export interface TimelineLane {
  threadId: ThreadId;
  /** Compact label shown next to the lane: `main` or a uuid prefix. */
  label: string;
  /** Full label content exposed on hover (e.g. the full root uuid). */
  tooltip: string;
  /** Whether this lane represents the session's main thread. */
  isMain: boolean;
  /** Speech dots, in sequence order. */
  dots: TimelineDot[];
}

/**
 * Truncate a uuid to its leading prefix for display next to a swim lane.
 * Returns the input unchanged when shorter than {@link LANE_LABEL_PREFIX_LEN}
 * so an unusual short uuid is not padded.
 */
export function laneLabelFromUuid(uuid: string): string {
  return uuid.length > LANE_LABEL_PREFIX_LEN
    ? uuid.slice(0, LANE_LABEL_PREFIX_LEN)
    : uuid;
}

/**
 * Build the swim-lane structure for the timeline footer from a session's
 * thread list and a per-thread message map.
 *
 * Each thread becomes one lane; the main thread (no parent) is labelled
 * `main`, sub-threads use the first {@link LANE_LABEL_PREFIX_LEN} chars of
 * `root_message_uuid` (full uuid kept in `tooltip`). Dots within a lane are
 * the thread's messages in `seq` order, indexed 0..N-1 so the renderer can
 * place them at equal horizontal spacing — wall-clock gaps are intentionally
 * collapsed.
 *
 * A thread missing from `messagesByThread` contributes an empty lane (no
 * dots). This lets the footer still draw the lane row while the per-thread
 * fetch is in flight, instead of suppressing it and resizing the moment the
 * data arrives.
 */
export function buildTimelineLanes(
  threads: Thread[],
  messagesByThread: Map<ThreadId, Message[]>,
): TimelineLane[] {
  // Sort oldest → newest, matching the navigator's tree order.
  const sorted = [...threads].sort((a, b) => {
    if (a.created_at < b.created_at) {
      return -1;
    }
    if (a.created_at > b.created_at) {
      return 1;
    }
    return a.id - b.id;
  });

  return sorted.map((thread) => {
    const isMain = thread.parent_thread_id === null;
    const rootUuid = thread.root_message_uuid ?? '';
    const label = isMain
      ? MAIN_LANE_LABEL
      : rootUuid !== ''
        ? laneLabelFromUuid(rootUuid)
        : `thread ${thread.id}`;
    const tooltip = isMain
      ? MAIN_LANE_LABEL
      : rootUuid !== ''
        ? rootUuid
        : `thread ${thread.id}`;
    const rawMessages = messagesByThread.get(thread.id) ?? [];
    // Defensive sort: the server returns messages in `seq` order, but the
    // pure helper makes no assumption about the input ordering so the lane
    // remains deterministic even when callers pass an unsorted copy.
    const sortedMessages = [...rawMessages].sort((a, b) => a.seq - b.seq);
    const dots: TimelineDot[] = sortedMessages.map((message, order) => ({
      uuid: message.uuid,
      threadId: thread.id,
      order,
    }));
    return { threadId: thread.id, label, tooltip, isMain, dots };
  });
}
