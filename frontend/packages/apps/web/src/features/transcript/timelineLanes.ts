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
 * A single message dot in a lane. The dot is rendered at index {@link order}
 * along a SHARED horizontal axis that spans every lane in lockstep, so dots
 * from different subthreads at the same x coordinate happened at the same
 * point in the global utterance sequence.
 */
export interface TimelineDot {
  /** The message uuid; drives hover-jump's `data-message-uuid` lookup. */
  uuid: string;
  /** The owning thread's id; mirrors {@link TimelineLane.threadId}. */
  threadId: ThreadId;
  /**
   * Zero-based position along the shared global utterance axis (all
   * subthreads' messages sorted by `created_at`, ties broken by `seq`). Dots
   * at the same `order` value across lanes lined up vertically happened at the
   * same global step, which is the whole point of the cross-lane axis.
   */
  order: number;
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
  /** Speech dots, placed on the shared global utterance axis. */
  dots: TimelineDot[];
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
 * Dots within a lane are the thread's messages placed on a SHARED global
 * utterance axis: every subthread's messages are merged, sorted by
 * `created_at` (ties broken by `seq`), and each message's index in that
 * global list becomes its `order`. Dots from different lanes sharing the same
 * `order` therefore align vertically because they happened at the same global
 * step — which is what makes the cross-thread time order readable.
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

  // Build a uuid → message index across ALL subthreads. Used both for the
  // global utterance ordering and for resolving each subthread's root-message
  // preview (which lives in a different lane's message list whenever the
  // subthread branched off from a message authored in the parent lane).
  const messagesByUuid = new Map<string, Message>();
  for (const messages of messagesByThread.values()) {
    for (const message of messages) {
      messagesByUuid.set(message.uuid, message);
    }
  }

  // Flatten every subthread's messages into a single list and sort by
  // `created_at` ascending so the timeline reads chronologically across lanes;
  // `seq` breaks ties when timestamps collide (or are equal at second
  // resolution). The resulting index becomes each dot's `order` so dots from
  // different lanes sit at the same x when they happened at the same step.
  const allMessages: Message[] = [];
  for (const messages of messagesByThread.values()) {
    for (const message of messages) {
      allMessages.push(message);
    }
  }
  allMessages.sort((a, b) => {
    if (a.created_at < b.created_at) {
      return -1;
    }
    if (a.created_at > b.created_at) {
      return 1;
    }
    return a.seq - b.seq;
  });
  const globalOrderByUuid = new Map<string, number>();
  allMessages.forEach((message, index) => {
    globalOrderByUuid.set(message.uuid, index);
  });

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
    // Defensive sort: the server returns messages in `seq` order, but the pure
    // helper makes no assumption about the input ordering so the lane remains
    // deterministic even when callers pass an unsorted copy.
    const sortedMessages = [...rawMessages].sort((a, b) => a.seq - b.seq);
    const dots: TimelineDot[] = sortedMessages.map((message) => ({
      uuid: message.uuid,
      threadId: thread.id,
      // Every message in `sortedMessages` was included in `allMessages`, so
      // the lookup always hits. The `?? 0` clause is a belt-and-braces guard
      // against an unexpected gap (e.g. a duplicate uuid) — keeping the dot
      // at the lane's start is preferable to crashing the footer.
      order: globalOrderByUuid.get(message.uuid) ?? 0,
    }));
    return { threadId: thread.id, label, tooltip, isMain, dots };
  });
}
