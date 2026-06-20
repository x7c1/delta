import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from 'react';
import type { ThreadId } from '@delta/model';
import type { Message, Thread } from '@delta/wire-gen';
import { useThreadsMessagesQueries } from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { buildTimelineLanes, type TimelineDot } from './timelineLanes';

/**
 * localStorage key for the timeline footer's expanded/collapsed state. Per
 * device, not per session — the user's preference travels across sessions.
 */
export const TIMELINE_EXPANDED_STORAGE_KEY = 'delta.thread-timeline-overlay.expanded';

/** Debounce window (ms) between hovering a dot and scrolling its message. */
export const HOVER_JUMP_DEBOUNCE_MS = 250;

/**
 * Read the persisted expanded preference; defaults to collapsed when no
 * preference has been saved yet or the storage layer is unavailable (SSR /
 * privacy-mode browsers).
 */
function readPersistedExpanded(): boolean {
  if (typeof window === 'undefined') {
    return false;
  }
  try {
    return window.localStorage.getItem(TIMELINE_EXPANDED_STORAGE_KEY) === 'true';
  } catch {
    return false;
  }
}

/**
 * Persist the expanded preference. Failures are swallowed so a quota error or
 * a disabled-storage browser never crashes the footer — the UI keeps working
 * in-memory for the session.
 */
function writePersistedExpanded(expanded: boolean): void {
  if (typeof window === 'undefined') {
    return;
  }
  try {
    window.localStorage.setItem(
      TIMELINE_EXPANDED_STORAGE_KEY,
      expanded ? 'true' : 'false',
    );
  } catch {
    // Storage may be unavailable (quota, privacy mode); ignore.
  }
}

/**
 * Expanded/collapsed state for the timeline footer, persisted to localStorage
 * so the preference survives reloads. Initial state is collapsed when no
 * preference has been saved. Exported so tests can drive the toggle directly.
 */
export function useTimelineExpanded(): [boolean, () => void] {
  const [expanded, setExpanded] = useState<boolean>(() => readPersistedExpanded());
  const toggle = useCallback(() => {
    setExpanded((prev) => {
      const next = !prev;
      writePersistedExpanded(next);
      return next;
    });
  }, []);
  return [expanded, toggle];
}

/**
 * Wire a debounced hover-jump on a swim-lane dot: hovering for at least
 * {@link HOVER_JUMP_DEBOUNCE_MS} fires {@link onJump} with the dot's uuid.
 * The dot keeps its hover visual immediately; the conversation pane reacts
 * only after the debounce elapses, intentionally minimising misfire churn.
 *
 * Exported for tests; the component below wires it into its dot handlers.
 */
export function useHoverJump(onJump: (uuid: string) => void): {
  onHover: (uuid: string) => void;
  onLeave: () => void;
} {
  const timerRef = useRef<number | null>(null);
  const cancel = useCallback(() => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);
  useEffect(() => cancel, [cancel]);
  const onHover = useCallback(
    (uuid: string) => {
      cancel();
      timerRef.current = window.setTimeout(() => {
        timerRef.current = null;
        onJump(uuid);
      }, HOVER_JUMP_DEBOUNCE_MS);
    },
    [cancel, onJump],
  );
  return { onHover, onLeave: cancel };
}

/**
 * Scroll the matching transcript message into view, centred. Scoped to the
 * given container so a duplicate `data-message-uuid` outside the transcript
 * (e.g. in a portaled preview) cannot misdirect the jump.
 */
export function scrollMessageIntoView(
  container: HTMLElement | null,
  uuid: string,
): void {
  if (!container) {
    return;
  }
  const target = container.querySelector(
    `[data-message-uuid="${CSS.escape(uuid)}"]`,
  );
  if (target) {
    target.scrollIntoView({ block: 'center' });
  }
}

export interface ThreadTimelineOverlayProps {
  /** All threads (main + subthreads) in the focused session. */
  threads: Thread[];
  /** The active thread; its lane is highlighted. */
  activeThreadId: ThreadId | null;
  /**
   * The conversation-pane scroll container hover-jump targets. The lookup is
   * scoped to it so an off-screen duplicate id (e.g. a portaled preview) does
   * not misdirect the scroll.
   */
  conversationBodyRef: RefObject<HTMLElement | null>;
}

/** Lane row height in pixels. */
const LANE_HEIGHT_PX = 18;
/** Equal horizontal spacing between dots within a lane. */
const DOT_SPACING_PX = 14;
/** Dot diameter; the hit area is enlarged via padding in the wrapper. */
const DOT_SIZE_PX = 8;
/** Width reserved on the left for lane labels. */
const LABEL_COLUMN_PX = 88;
/** Width reserved for the right-hand padding inside the lane area. */
const LANE_RIGHT_PAD_PX = 16;

/**
 * The fixed footer between the conversation pane and the composer: a swim-lane
 * timeline of every subthread (and the main thread). Each thread is a row,
 * each speech turn is a dot at its sequence index, and hovering a dot scrolls
 * the matching message into view in the conversation pane (debounced by
 * {@link HOVER_JUMP_DEBOUNCE_MS} ms). The footer is always present; clicking
 * the title bar collapses or expands the lanes, and the preference is
 * persisted per device.
 *
 * MVP intentionally omits cross-row derivation lines, kind-coded dots, and
 * click-to-pin — they are tracked as separate follow-ups.
 */
export function ThreadTimelineOverlay({
  threads,
  activeThreadId,
  conversationBodyRef,
}: ThreadTimelineOverlayProps) {
  const client = useApiClient();
  const [expanded, toggle] = useTimelineExpanded();

  // N+1 is acceptable for MVP; the dedicated `all_threads=true` REST is
  // intentionally deferred. The query keys are shared with the focused
  // thread's `useThreadMessagesQuery`, so its messages are reused — no double
  // request.
  const threadIds = useMemo(() => threads.map((t) => t.id), [threads]);
  const messagesQueries = useThreadsMessagesQueries(client, threadIds);
  const messagesByThread = useMemo(() => {
    const map = new Map<ThreadId, Message[]>();
    for (const entry of messagesQueries) {
      const data = entry.result.data;
      if (data) {
        map.set(entry.threadId, data.messages);
      }
    }
    return map;
  }, [messagesQueries]);

  const lanes = useMemo(
    () => buildTimelineLanes(threads, messagesByThread),
    [threads, messagesByThread],
  );

  const handleJump = useCallback(
    (uuid: string) => {
      scrollMessageIntoView(conversationBodyRef.current, uuid);
    },
    [conversationBodyRef],
  );
  const { onHover, onLeave } = useHoverJump(handleJump);

  const widestLaneDots = lanes.reduce(
    (max, lane) => Math.max(max, lane.dots.length),
    0,
  );
  const laneAxisWidth =
    widestLaneDots > 0
      ? DOT_SPACING_PX * Math.max(widestLaneDots - 1, 1) + DOT_SIZE_PX
      : DOT_SIZE_PX;

  return (
    <section
      data-testid="thread-timeline-overlay"
      data-expanded={expanded ? 'true' : 'false'}
      className="select-none rounded-md border border-slate-200 bg-white text-xs text-slate-600 shadow-sm"
      aria-label="Subthread timeline"
    >
      <button
        type="button"
        onClick={toggle}
        data-testid="thread-timeline-toggle"
        aria-expanded={expanded}
        className="flex w-full items-center justify-between gap-2 rounded-md px-2 py-1 text-left font-medium text-slate-500 transition-colors hover:bg-slate-50"
      >
        <span className="flex items-center gap-1.5">
          <span
            aria-hidden="true"
            className={`inline-block h-1.5 w-1.5 rounded-full ${
              expanded ? 'bg-slate-500' : 'bg-slate-300'
            }`}
          />
          Thread timeline
          {lanes.length > 0 && (
            <span className="text-slate-400">({lanes.length})</span>
          )}
        </span>
        <span aria-hidden="true" className="text-slate-400">
          {expanded ? '▾' : '▸'}
        </span>
      </button>
      {expanded && (
        <div
          data-testid="thread-timeline-body"
          className="max-h-40 overflow-auto px-2 pb-1"
          onMouseLeave={onLeave}
        >
          {lanes.length === 0 ? (
            <p className="px-1 py-1 text-[0.7rem] text-slate-400">
              No threads to show yet.
            </p>
          ) : (
            <ul className="flex flex-col gap-0.5" role="list">
              {lanes.map((lane) => {
                const isActive = lane.threadId === activeThreadId;
                return (
                  <li
                    key={lane.threadId}
                    data-testid="thread-timeline-lane"
                    data-thread-id={lane.threadId}
                    data-active={isActive ? 'true' : 'false'}
                    className={`flex items-center gap-2 rounded-sm px-1 ${
                      isActive
                        ? 'border-y border-slate-200 bg-slate-50'
                        : 'border-y border-transparent'
                    }`}
                    style={{ minHeight: LANE_HEIGHT_PX }}
                  >
                    <span
                      title={lane.tooltip}
                      data-testid="thread-timeline-lane-label"
                      className={`block shrink-0 truncate font-mono text-[0.65rem] ${
                        lane.isMain ? 'text-slate-700' : 'text-slate-500'
                      }`}
                      style={{ width: LABEL_COLUMN_PX }}
                    >
                      {lane.label}
                    </span>
                    <div
                      className="relative shrink-0"
                      style={{
                        width: laneAxisWidth + LANE_RIGHT_PAD_PX,
                        height: LANE_HEIGHT_PX,
                      }}
                    >
                      <span
                        aria-hidden="true"
                        className="absolute left-0 top-1/2 h-px -translate-y-1/2 bg-slate-200"
                        style={{ width: laneAxisWidth }}
                      />
                      {lane.dots.map((dot) => (
                        <TimelineDotMark
                          key={dot.uuid}
                          dot={dot}
                          onHover={onHover}
                          onLeave={onLeave}
                        />
                      ))}
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      )}
    </section>
  );
}

interface TimelineDotMarkProps {
  dot: TimelineDot;
  onHover: (uuid: string) => void;
  onLeave: () => void;
}

/**
 * One dot within a lane. Its hit area is intentionally larger than the visible
 * dot via padding, so the hover-jump triggers comfortably on small marks; the
 * visible dot only enlarges on hover so the user gets confirmation feedback
 * before the debounce fires.
 */
function TimelineDotMark({ dot, onHover, onLeave }: TimelineDotMarkProps) {
  const [hovered, setHovered] = useState(false);
  const left = dot.order * DOT_SPACING_PX;
  return (
    <button
      type="button"
      data-testid="thread-timeline-dot"
      data-message-uuid={dot.uuid}
      data-order={dot.order}
      onMouseEnter={() => {
        setHovered(true);
        onHover(dot.uuid);
      }}
      onMouseLeave={() => {
        setHovered(false);
        onLeave();
      }}
      onFocus={() => {
        setHovered(true);
        onHover(dot.uuid);
      }}
      onBlur={() => {
        setHovered(false);
        onLeave();
      }}
      aria-label={`Jump to message ${dot.uuid}`}
      className="absolute top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-full p-1"
      style={{ left: left + DOT_SIZE_PX / 2 }}
    >
      <span
        aria-hidden="true"
        className={`block rounded-full transition-colors ${
          hovered ? 'bg-slate-700' : 'bg-slate-400'
        }`}
        style={{
          width: hovered ? DOT_SIZE_PX + 2 : DOT_SIZE_PX,
          height: hovered ? DOT_SIZE_PX + 2 : DOT_SIZE_PX,
        }}
      />
    </button>
  );
}
