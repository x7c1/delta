import type { ThreadId } from '@delta/model';
import type { Message, Thread } from '@delta/wire-gen';

/**
 * Number of leading characters of a subthread's root-message body used as its
 * lane label. Picked from the 20–30 range called out in the spec — long enough
 * to disambiguate at a glance, short enough to fit in a compact label column
 * without truncation noise. The full body text is exposed via the lane's
 * `tooltip` so the trimmed prefix never loses the underlying context.
 */
export const LANE_LABEL_PREFIX_LEN = 24;

/** Special label for the main thread's lane. */
export const MAIN_LANE_LABEL = 'main';

/** Fallback label when a subthread's root message body cannot be resolved. */
export const NO_PREVIEW_LANE_LABEL = '(no preview)';

/**
 * A single message dot in a lane. Its {@link x} is a 0..1 fraction along the
 * shared time axis: 0 = earliest message in the whole session, 1 = latest.
 * Dots are positioned by their `created_at` timestamp, so idle/thinking gaps
 * are visible as horizontal whitespace between dots — the cross-lane axis
 * reads as a chronological scrub rather than equal-spacing speech order.
 */
export interface TimelineDot {
  /** The message uuid; drives the playhead's `data-message-uuid` lookup. */
  uuid: string;
  /** The owning thread's id; mirrors {@link TimelineLane.threadId}. */
  threadId: ThreadId;
  /**
   * Fraction of the shared time axis the message falls on: 0 for the earliest
   * message across the whole session, 1 for the latest. Multiplied by the
   * lane's pixel width at render time to get the dot's absolute x.
   */
  x: number;
  /**
   * Epoch milliseconds of {@link Message.created_at}, exposed so the active
   * message lookup can rank candidates by their absolute time when the
   * playhead lands between dots.
   */
  timeMs: number;
}

/**
 * A swim lane in the timeline footer: one row per (sub)thread. Lanes are
 * sorted oldest → newest by the thread's `created_at`, matching the
 * navigator's tree order.
 */
export interface TimelineLane {
  threadId: ThreadId;
  /** Compact label shown next to the lane: `main` or a body-text prefix. */
  label: string;
  /** Full label content exposed on hover (e.g. the full root body). */
  tooltip: string;
  /** Whether this lane represents the session's main thread. */
  isMain: boolean;
  /** Speech dots, placed on the shared 0..1 time axis. */
  dots: TimelineDot[];
}

/**
 * Earliest/latest epoch milliseconds across every message rendered on the
 * shared axis. `null` when no message has a parseable timestamp — the axis is
 * effectively empty in that case and dots collapse to x=0.
 */
export interface TimelineTimeRange {
  minMs: number;
  maxMs: number;
}

/**
 * Truncate a string to its leading prefix for display next to a swim lane.
 * Returns the input unchanged when shorter than {@link LANE_LABEL_PREFIX_LEN}
 * so a short body is not padded.
 */
export function laneLabelFromText(text: string): string {
  return text.length > LANE_LABEL_PREFIX_LEN
    ? text.slice(0, LANE_LABEL_PREFIX_LEN)
    : text;
}

/**
 * The visible prose of a message, normalised to a single line for use in a
 * compact lane label.
 *
 * Preference order matches the wire contract: the backend-precomputed
 * `content_text` is the canonical flat view, so it is used first; when it is
 * null or empty we fall back to concatenating the message's `text` content
 * blocks (the same shape `MessageItem` renders, minus the formatting). Runs
 * of whitespace — newlines included — collapse to single spaces so the
 * trimmed prefix reads smoothly even when the underlying body began with a
 * code fence or a list.
 */
export function messagePreviewText(message: Message): string {
  const raw =
    message.content_text !== null && message.content_text !== ''
      ? message.content_text
      : message.content
          .filter((block) => block.type === 'text')
          .map((block) => block.text)
          .join(' ');
  return raw.replace(/\s+/g, ' ').trim();
}

/**
 * Parse a message's `created_at` ISO-8601 string into epoch milliseconds, or
 * `null` when the field is empty (the wire contract for "no timestamp on the
 * transcript line") or otherwise unparseable. Messages without a timestamp
 * are dropped from the time-axis layout — there is no meaningful x for them.
 */
export function messageTimeMs(message: Message): number | null {
  if (message.created_at === '') {
    return null;
  }
  const ms = Date.parse(message.created_at);
  return Number.isFinite(ms) ? ms : null;
}

/**
 * Earliest and latest `created_at` (in epoch ms) across every (sub)thread's
 * messages. Returns `null` when no message carries a parseable timestamp.
 */
export function computeTimeRange(
  messagesByThread: Map<ThreadId, Message[]>,
): TimelineTimeRange | null {
  let minMs = Number.POSITIVE_INFINITY;
  let maxMs = Number.NEGATIVE_INFINITY;
  for (const messages of messagesByThread.values()) {
    for (const message of messages) {
      const ms = messageTimeMs(message);
      if (ms === null) {
        continue;
      }
      if (ms < minMs) {
        minMs = ms;
      }
      if (ms > maxMs) {
        maxMs = ms;
      }
    }
  }
  if (!Number.isFinite(minMs) || !Number.isFinite(maxMs)) {
    return null;
  }
  return { minMs, maxMs };
}

/**
 * Map an epoch ms to a 0..1 fraction along the time axis. When the range is
 * degenerate (every message landed at the same instant, or only one message
 * exists) every dot collapses to x=0 so they still render as a stacked column
 * at the lane's left edge instead of triggering a divide-by-zero.
 */
export function xFraction(
  timeMs: number,
  range: TimelineTimeRange | null,
): number {
  if (range === null || range.maxMs === range.minMs) {
    return 0;
  }
  return (timeMs - range.minMs) / (range.maxMs - range.minMs);
}

/**
 * Build the swim-lane structure for the timeline footer from a session's
 * thread list and a per-thread message map.
 *
 * Each thread becomes one lane; the main thread (no parent) is labelled
 * `main`, sub-threads use the first {@link LANE_LABEL_PREFIX_LEN} chars of
 * their root message's body (full body kept in `tooltip`). When the root
 * message cannot be resolved — e.g. the per-thread fetch has not landed yet
 * — the label falls back to {@link NO_PREVIEW_LANE_LABEL} and the tooltip
 * carries the full root uuid so the anchor stays recoverable.
 *
 * Dots within a lane are the thread's messages placed on a SHARED time axis
 * spanning the earliest..latest `created_at` across every (sub)thread. Each
 * dot carries its 0..1 fraction so the renderer multiplies by the lane's
 * pixel width to get the dot's absolute x — idle/thinking gaps become visible
 * as horizontal whitespace, which is what makes the time order readable.
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
  // Sort lanes oldest → newest, matching the navigator's tree order.
  const sortedThreads = [...threads].sort((a, b) => {
    if (a.created_at < b.created_at) {
      return -1;
    }
    if (a.created_at > b.created_at) {
      return 1;
    }
    return a.id - b.id;
  });

  // Build a uuid → message index across ALL subthreads. Used for resolving
  // each subthread's root-message preview (the root often lives in a different
  // lane's message list whenever the subthread branched off from a message
  // authored in the parent lane).
  const messagesByUuid = new Map<string, Message>();
  for (const messages of messagesByThread.values()) {
    for (const message of messages) {
      messagesByUuid.set(message.uuid, message);
    }
  }

  const range = computeTimeRange(messagesByThread);

  return sortedThreads.map((thread) => {
    const isMain = thread.parent_thread_id === null;
    const rootUuid = thread.root_message_uuid ?? '';
    const rootMessage =
      rootUuid !== '' ? messagesByUuid.get(rootUuid) ?? null : null;
    const rootPreview =
      rootMessage !== null ? messagePreviewText(rootMessage) : '';

    let label: string;
    let tooltip: string;
    if (isMain) {
      label = MAIN_LANE_LABEL;
      tooltip = MAIN_LANE_LABEL;
    } else if (rootPreview !== '') {
      label = laneLabelFromText(rootPreview);
      tooltip = rootPreview;
    } else if (rootUuid !== '') {
      // Root uuid is known but the message itself is not in the merged set
      // yet (or the body is empty). Fall back to a generic label and expose
      // the full uuid as the tooltip so the anchor stays recoverable.
      label = NO_PREVIEW_LANE_LABEL;
      tooltip = rootUuid;
    } else {
      label = `thread ${thread.id}`;
      tooltip = `thread ${thread.id}`;
    }

    const rawMessages = messagesByThread.get(thread.id) ?? [];
    // Defensive sort by `seq`: the server returns messages in `seq` order, but
    // the pure helper makes no assumption about the input ordering so the lane
    // stays deterministic even when callers pass an unsorted copy.
    const sortedMessages = [...rawMessages].sort((a, b) => a.seq - b.seq);
    const dots: TimelineDot[] = [];
    for (const message of sortedMessages) {
      const ms = messageTimeMs(message);
      if (ms === null) {
        // No timestamp → no meaningful x. Skip rather than guess a position.
        continue;
      }
      dots.push({
        uuid: message.uuid,
        threadId: thread.id,
        x: xFraction(ms, range),
        timeMs: ms,
      });
    }
    return { threadId: thread.id, label, tooltip, isMain, dots };
  });
}

/** A dot annotated with its owning lane index — what the playhead resolves to. */
export interface ActiveMessageMatch {
  /** Owning lane's index in the rendered lane array. */
  laneIndex: number;
  /** The matched dot's lane id, redundantly exposed for convenience. */
  threadId: ThreadId;
  /** The matched message's uuid. */
  uuid: string;
  /** The matched dot's 0..1 x on the shared time axis. */
  x: number;
  /** The matched dot's epoch ms (mirrors {@link TimelineDot.timeMs}). */
  timeMs: number;
}

/**
 * Find the message dot whose x is closest to the playhead's x across every
 * lane. Returns `null` when no lane has a dot to land on. Ties (two dots
 * equidistant from the playhead) are broken by the smaller `timeMs` first,
 * then by `uuid` lexicographically — both deterministic so the lookup never
 * flickers between equally-good candidates as the playhead moves.
 */
export function findActiveMessage(
  lanes: TimelineLane[],
  playheadX: number,
): ActiveMessageMatch | null {
  let best: ActiveMessageMatch | null = null;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (let laneIndex = 0; laneIndex < lanes.length; laneIndex += 1) {
    const lane = lanes[laneIndex];
    for (const dot of lane.dots) {
      const distance = Math.abs(dot.x - playheadX);
      if (distance < bestDistance) {
        bestDistance = distance;
        best = {
          laneIndex,
          threadId: dot.threadId,
          uuid: dot.uuid,
          x: dot.x,
          timeMs: dot.timeMs,
        };
        continue;
      }
      if (distance === bestDistance && best !== null) {
        // Deterministic tie-break: earlier `timeMs` first, then smaller uuid.
        if (
          dot.timeMs < best.timeMs ||
          (dot.timeMs === best.timeMs && dot.uuid < best.uuid)
        ) {
          best = {
            laneIndex,
            threadId: dot.threadId,
            uuid: dot.uuid,
            x: dot.x,
            timeMs: dot.timeMs,
          };
        }
      }
    }
  }
  return best;
}
