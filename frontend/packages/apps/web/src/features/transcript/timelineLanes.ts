import {
  MAIN_THREAD_DISPLAY_NAME,
  threadDisplayName,
  threadTooltip,
  type ThreadId,
} from '@delta/model';
import type { Message, Thread } from '@delta/wire-gen';

/**
 * Special label for the main thread's lane.
 *
 * Re-exported from the shared {@link MAIN_THREAD_DISPLAY_NAME} so the
 * conventional name lives in one place. The timeline used to publish its own
 * constant under this name — kept as a back-compat alias for existing tests
 * and imports while the rename settles.
 */
export const MAIN_LANE_LABEL = MAIN_THREAD_DISPLAY_NAME;

/**
 * Classification of a message's author for the timeline mark's color.
 *
 * - `user` — a genuine human turn (matches `MessageItem`'s `isUserTurn` rule:
 *   `role === 'user'` AND at least one author-written text block). A user-role
 *   message that only carries tool results is NOT classified as `user` here,
 *   mirroring the transcript's left/right split where those render assistant-
 *   side.
 * - `other` — everything else (assistant, meta, system, tool-result carriers,
 *   etc.). MVP picks a two-color scheme; finer-grained distinction within
 *   `other` is deferred to a follow-up.
 */
export type TimelineDotKind = 'user' | 'other';

/**
 * Size class for the timeline mark: marks the "main" turns of the conversation
 * (a human turn or Claude's prose reply) larger than the surrounding tool
 * calls, meta lines, and question cards so the eye reads the headline shape of
 * the chat at a glance.
 *
 * - `large` — a human turn or an assistant turn that carries prose text. These
 *   are the bubbles the transcript renders as the conversation's headline.
 * - `small` — everything else (tool calls and their result carriers, meta
 *   lines, system rows, the harness-injected task-notification card, etc.).
 */
export type TimelineDotSize = 'large' | 'small';

/**
 * A single message mark in a lane. Its {@link x} is a 0..1 fraction along the
 * shared time axis: 0 = earliest message in the whole session, 1 = latest.
 * Marks are positioned by their `created_at` timestamp, so idle/thinking gaps
 * are visible as horizontal whitespace between marks — the cross-lane axis
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
   * lane's pixel width at render time to get the mark's absolute x.
   */
  x: number;
  /**
   * Epoch milliseconds of {@link Message.created_at}, exposed so the active
   * message lookup can rank candidates by their absolute time when the
   * playhead lands between marks.
   */
  timeMs: number;
  /**
   * Monotonic per-session sequence number from {@link Message.seq}. Used as
   * the deterministic tie-break when two messages share the same `created_at`
   * (the time axis cannot distinguish them, but `seq` always can) when the
   * cross-lane sorted list orders messages for discrete step navigation.
   */
  seq: number;
  /**
   * Author classification for the mark's color (see {@link TimelineDotKind}).
   * Computed at lane-build time so the renderer is a pure function of the
   * pre-computed dot data.
   */
  kind: TimelineDotKind;
  /**
   * Size class for the mark's circle radius (see {@link TimelineDotSize}).
   * The "main conversation" turns — a user message or an assistant text reply
   * — render as the larger circle; everything else (tool calls, meta,
   * question cards, etc.) renders smaller.
   */
  size: TimelineDotSize;
}

/**
 * A swim lane in the timeline footer: one row per (sub)thread. Lanes are
 * sorted oldest → newest by the thread's `created_at`, matching the
 * navigator's tree order.
 */
export interface TimelineLane {
  threadId: ThreadId;
  /**
   * Label shown next to the lane: `main` for the main thread, or the wire
   * `thread.title` for a subthread (same source Navigator displays, via the
   * shared {@link threadDisplayName} helper). Visual truncation is the
   * renderer's job (it reserves a fixed label column and CSS-truncates).
   */
  label: string;
  /**
   * Tooltip content exposed on hover. For a subthread this is the full
   * untrimmed title so a label cut by CSS truncation can still be read.
   */
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
 * Classify a message's author for the timeline mark's color.
 *
 * Mirrors `MessageItem`'s `isUserTurn` rule so the timeline mark agrees with
 * the transcript's own left/right split: a `role: 'user'` message that only
 * carries tool results (no author-written text block) is a tool-result
 * carrier, not a human turn — it counts as `other` here, the same side the
 * transcript renders it on.
 *
 * Finer-grained distinction within `other` (assistant vs tool vs question
 * card vs meta) is intentionally deferred — MVP uses a two-color scheme.
 */
export function classifyMessage(message: Message): TimelineDotKind {
  if (
    message.role === 'user' &&
    message.content.some((block) => block.type === 'text')
  ) {
    return 'user';
  }
  return 'other';
}

/**
 * Decide whether a message is part of the "main" conversation (the headline
 * turns the eye should track first) or an auxiliary line.
 *
 * A user-role message with an author-written text block is a real human turn —
 * `large`. An assistant message that carries a prose `text` block is Claude's
 * reply — also `large`. Everything else is auxiliary: pure tool calls and
 * their result carriers, meta lines, harness-injected cards, system rows. The
 * rule cares about "is there author prose to read?", not the role alone, so an
 * assistant message that is JUST a tool call (no prose) is `small` and a
 * user-role tool-result carrier (no prose) is `small` — matching the
 * transcript's bubble-vs-collapsible split.
 *
 * The wheel-driven step navigation walks the `large` subset only, while click
 * jumps still target every mark — small ones are visible anchors the user can
 * still tap precisely when they want a specific tool call.
 */
export function classifyMessageSize(message: Message): TimelineDotSize {
  const hasText = message.content.some((block) => block.type === 'text');
  if (hasText && (message.role === 'user' || message.role === 'assistant')) {
    return 'large';
  }
  return 'small';
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
 * Each thread becomes one lane; lane labels go through the shared
 * {@link threadDisplayName} / {@link threadTooltip} helpers — the same
 * helpers Navigator uses — so a subthread cannot ever show two different
 * names in two different panes. The main thread is labelled with the
 * conventional {@link MAIN_THREAD_DISPLAY_NAME}.
 *
 * Marks within a lane are the thread's messages placed on a SHARED time axis
 * spanning the earliest..latest `created_at` across every (sub)thread. Each
 * mark carries its 0..1 fraction so the renderer multiplies by the lane's
 * pixel width to get the absolute x — idle/thinking gaps become visible as
 * horizontal whitespace, which is what makes the time order readable. Each
 * mark also carries a {@link TimelineDotKind} so the renderer can color it
 * by author (user vs everything else).
 *
 * A thread missing from `messagesByThread` contributes an empty lane (no
 * marks). This lets the footer still draw the lane row while the per-thread
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

  const range = computeTimeRange(messagesByThread);

  return sortedThreads.map((thread) => {
    const isMain = thread.parent_thread_id === null;
    const label = threadDisplayName(thread, { isMain });
    const tooltip = threadTooltip(thread, { isMain });

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
        seq: message.seq,
        kind: classifyMessage(message),
        size: classifyMessageSize(message),
      });
    }
    return { threadId: thread.id, label, tooltip, isMain, dots };
  });
}

/**
 * A single message entry on the global, cross-lane sorted list that drives
 * discrete step navigation. Each entry carries everything the navigation
 * needs (uuid, owning thread, axis x) so the wheel handler can advance the
 * active index without re-resolving the dot through its lane.
 */
export interface SortedMessage {
  /** The message uuid; drives the conversation pane's scroll-into-view lookup. */
  uuid: string;
  /** The owning thread's id; consumed by the active-thread switch. */
  threadId: ThreadId;
  /** Fraction of the shared time axis the message falls on (mirrors {@link TimelineDot.x}). */
  x: number;
  /** Epoch milliseconds of `created_at` (mirrors {@link TimelineDot.timeMs}). */
  timeMs: number;
  /** Monotonic per-session sequence number (mirrors {@link TimelineDot.seq}). */
  seq: number;
  /**
   * Size class of the corresponding {@link TimelineDot}. Carried through so
   * the wheel-step navigation can filter the global list down to the `large`
   * (main-conversation) subset without needing to look up the original dot.
   */
  size: TimelineDotSize;
}

/**
 * Flatten every lane's dots into a single timeline-sorted list of messages.
 * The sort is `created_at` ascending, tie-broken by `seq` ascending — so a
 * batch of messages emitted at the same millisecond still orders by the
 * monotonic per-session sequence the transcript already shows them in.
 *
 * Lane / subthread boundaries are ignored: the next message after lane A's
 * last entry may live in lane B, and the navigation steps right across that
 * boundary the same way the transcript reads.
 */
export function buildSortedMessages(lanes: TimelineLane[]): SortedMessage[] {
  const entries: SortedMessage[] = [];
  for (const lane of lanes) {
    for (const dot of lane.dots) {
      entries.push({
        uuid: dot.uuid,
        threadId: dot.threadId,
        x: dot.x,
        timeMs: dot.timeMs,
        seq: dot.seq,
        size: dot.size,
      });
    }
  }
  entries.sort((a, b) => {
    if (a.timeMs !== b.timeMs) {
      return a.timeMs - b.timeMs;
    }
    return a.seq - b.seq;
  });
  return entries;
}

/**
 * Same shape as {@link buildSortedMessages} but restricted to the `large`
 * (main-conversation) subset: user turns and Claude's prose replies. Drives
 * the wheel-step navigation so one notch advances by one headline turn,
 * skipping the surrounding tool calls, meta lines, and question cards. The
 * click-to-jump path still uses the full {@link buildSortedMessages} so a
 * direct tap on a small mark remains precise.
 *
 * Returns a fresh array each call, with the same `(timeMs, seq)` ascending
 * order as the unfiltered list.
 */
export function buildLargeSortedMessages(
  lanes: TimelineLane[],
): SortedMessage[] {
  return buildSortedMessages(lanes).filter((m) => m.size === 'large');
}

/**
 * Result of {@link buildGlobalXMap}: the per-message absolute x in pixels and
 * the total axis width those positions occupy. Lane rendering, the playhead,
 * click-to-jump, and the scroll-follow effect all read from the same map so
 * one message has the same x in every lane and the spacing logic is the only
 * source of truth.
 */
export interface GlobalXMap {
  /**
   * Absolute x in pixels for each message, keyed by uuid. Looking up a uuid
   * not in the map yields `undefined` — callers should treat that as "the
   * message is not on the timeline" and fall back to 0.
   */
  pxByUuid: Map<string, number>;
  /**
   * The widest x across every message, never below the caller-supplied
   * `minWidth`. Lane renderers use this as the shared axis width so a dense
   * session pushes the right edge out and an idle session stays at the
   * minimum.
   */
  axisWidth: number;
}

/**
 * Compute the per-message x positions for the shared lane axis. Each message
 * is projected to its ideal `xFraction(timeMs, range) * baseWidth`, then a
 * left-to-right greedy pass nudges any neighbour that would overlap to the
 * right so adjacent marks always clear each other by at least the sum of
 * their radii.
 *
 * The minimum spacing between two adjacent messages is the average of their
 * mark diameters — i.e. the sum of their radii — so two large marks land 6 px
 * apart, two small marks 4 px apart, and a large/small neighbour pair 5 px
 * apart. That is the tightest spacing that still guarantees the two circles
 * do not paint into each other; any tighter and dense clusters smear back
 * into the illegible blur the soft alpha + ring workaround was patching over.
 *
 * The greedy push is one-directional (left → right): the leftmost message
 * always sits on its ideal x, while the right edge can drift past `baseWidth`
 * when the session is dense. `axisWidth` reflects that drift so the renderer
 * extends the lane to fit — never below `minWidth`, so a single-dot session
 * still has a scrubbable axis.
 *
 * Sharing one map across every lane is load-bearing: a message at a given
 * timestamp must land at the same x in every lane so cross-lane playhead
 * jumps line up with the marks the user sees.
 */
export function buildGlobalXMap(
  sortedMessages: SortedMessage[],
  range: TimelineTimeRange | null,
  baseWidth: number,
  markDiameter: (size: TimelineDotSize) => number,
  minWidth: number,
): GlobalXMap {
  const pxByUuid = new Map<string, number>();
  if (sortedMessages.length === 0) {
    return { pxByUuid, axisWidth: minWidth };
  }
  let lastX = 0;
  let lastDiameter = 0;
  let widest = 0;
  for (let i = 0; i < sortedMessages.length; i += 1) {
    const msg = sortedMessages[i];
    const ideal = xFraction(msg.timeMs, range) * baseWidth;
    const diameter = markDiameter(msg.size);
    let actual: number;
    if (i === 0) {
      actual = ideal;
    } else {
      const minSpacing = (lastDiameter + diameter) / 2;
      actual = Math.max(ideal, lastX + minSpacing);
    }
    pxByUuid.set(msg.uuid, actual);
    lastX = actual;
    lastDiameter = diameter;
    if (actual > widest) {
      widest = actual;
    }
  }
  return { pxByUuid, axisWidth: Math.max(minWidth, widest) };
}

/**
 * Index of the message in {@link sortedMessages} whose absolute px x is
 * closest to the given target. Reads each candidate's x from
 * {@link GlobalXMap.pxByUuid} so the click-to-jump answer agrees with the
 * marks the user actually sees. Returns `-1` when the list is empty. Ties
 * (two messages equidistant from the target) are broken by the smaller
 * `timeMs` first, then by smaller `seq` — both deterministic so a click
 * never flickers between equally-good candidates.
 */
export function findNearestMessageIndex(
  sortedMessages: SortedMessage[],
  pxByUuid: Map<string, number>,
  pxTarget: number,
): number {
  if (sortedMessages.length === 0) {
    return -1;
  }
  const xOf = (msg: SortedMessage) => pxByUuid.get(msg.uuid) ?? 0;
  let bestIndex = 0;
  let bestDistance = Math.abs(xOf(sortedMessages[0]) - pxTarget);
  for (let i = 1; i < sortedMessages.length; i += 1) {
    const candidate = sortedMessages[i];
    const distance = Math.abs(xOf(candidate) - pxTarget);
    if (distance < bestDistance) {
      bestDistance = distance;
      bestIndex = i;
      continue;
    }
    if (distance === bestDistance) {
      const current = sortedMessages[bestIndex];
      if (
        candidate.timeMs < current.timeMs ||
        (candidate.timeMs === current.timeMs && candidate.seq < current.seq)
      ) {
        bestIndex = i;
      }
    }
  }
  return bestIndex;
}
