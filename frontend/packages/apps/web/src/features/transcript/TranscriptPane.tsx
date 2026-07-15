import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from 'react';
import { threadAncestry, type ThreadId } from '@delta/model';
import type { Message, Thread } from '@delta/wire-gen';
import { useThreadMessagesQuery } from '@delta/api-client';
import { Badge, Breadcrumb, Button, Chip, Panel } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useComposerStore } from '../../store/composerStore';
import { noticeOf, useLiveStore } from '../../store/liveStore';
import { Composer } from '../composer/Composer';
import { PendingQueue } from '../composer/PendingQueue';
import {
  usePendingSends,
  type PendingSurface,
} from '../composer/usePendingSends';
import { WorkdirChip, WorkdirDialog } from '../composer/WorkdirDialog';
import { NewSessionPanel } from '../new-session/NewSessionPanel';
import { NewSessionTabBar } from '../new-session/NewSessionTabBar';
import { WorktreeOptions } from '../composer/WorktreeOptions';
import { LaunchOptionsPicker } from '../composer/LaunchOptionsPicker';
import { AssistantMarkdown } from './AssistantMarkdown';
import { isTaskNotificationMessage } from './claudeFormat';
import { MessageItem } from './MessageItem';
import { PermissionNoticeCard } from './PermissionNotice';
import { QuestionCard } from './QuestionCard';
import { SubagentRunningIndicator } from './SubagentRunningIndicator';
import {
  ThreadTimelineOverlay,
  useTimelineExpanded,
} from './ThreadTimelineOverlay';
import { childThreadsByMessage } from './branches';
import { buildToolPairing, messageRendersNothing } from './toolPairs';
import { persistedHasStreamedText } from './streamingHandoff';
import {
  clearBranchHighlight,
  findAllQuoteRanges,
  setBranchHighlight,
} from './branchHighlight';

/**
 * Distance from the bottom (in px) under which the transcript is considered
 * "at the bottom" and keeps following new content.
 */
const STICK_THRESHOLD_PX = 64;

/**
 * Pointer travel (px, per axis) between `mousedown` and `click` under which a
 * click counts as stationary — a plain click rather than the release of a
 * drag-select. Kept small so a genuine drag (which arms a pending branch) is
 * never mistaken for a plain dismiss click, while the sub-pixel jitter a real
 * "stationary" click can carry still reads as stationary.
 */
const CLICK_DRAG_SLOP_PX = 6;

/**
 * Fallback gap (px) between the bottom overlay and the resting content when the
 * `--delta-overlay-inset` token cannot be read (e.g. jsdom, which computes no
 * styles). Mirrors the token's resting value (0.75rem at the 16px default root)
 * so the measured reserve still leaves the same gap the floating card uses.
 */
const OVERLAY_INSET_FALLBACK_PX = 12;

/**
 * Comfortable reading gap (px) kept between the last turn and the bottom overlay,
 * added on top of the measured reserve. The measured reserve (overlay height +
 * inset) alone parks the last turn flush against the composer, which reads worse
 * than leaving some air there — so a slice of the breathing room the old fixed
 * reserve always had is preserved here while the rest of the reserve still tracks
 * the overlay's real height. Present at every composer size and grows with it.
 */
const BODY_BOTTOM_READING_GAP_PX = 192;

/**
 * Shared chrome for the transcript pane's cards: the floating bottom notices
 * card and composer card that hover over the conversation, plus the in-flow
 * breadcrumb card that sits in the top region above it. A full border,
 * rounded corners, an opaque surface fill that occludes the conversation
 * beneath, and a shadow so the card reads as lifted above its surroundings
 * rather than fused to them. Per-card padding is applied at each use site.
 */
const FLOATING_CARD_CLASS =
  'rounded-md border border-border-default bg-surface shadow-md';

/**
 * The overlay inset in pixels: the gap the floating cards leave from the body
 * edges (`--delta-overlay-inset`). Read from the live computed style so the
 * measured bottom reserve (overlay height + this gap) stays in lockstep with the
 * card's own `bottom-overlay-inset`, even if the token is themed. Falls back to
 * {@link OVERLAY_INSET_FALLBACK_PX} when the value is unavailable or non-numeric.
 */
function overlayInsetPx(el: Element): number {
  const raw = getComputedStyle(el)
    .getPropertyValue('--delta-overlay-inset')
    .trim();
  if (raw.endsWith('rem')) {
    const rem = Number.parseFloat(raw);
    const rootSize = Number.parseFloat(
      getComputedStyle(document.documentElement).fontSize,
    );
    const px = rem * (Number.isFinite(rootSize) ? rootSize : 16);
    return Number.isFinite(px) ? px : OVERLAY_INSET_FALLBACK_PX;
  }
  const px = Number.parseFloat(raw);
  return Number.isFinite(px) ? px : OVERLAY_INSET_FALLBACK_PX;
}

export interface TranscriptPaneProps {
  threads: Thread[];
  /** The active thread, or null for the cold-start / new-session state. */
  activeThread: Thread | null;
  /** True when the focused session is closed (read-only viewing; a Send resumes it). */
  readOnly: boolean;
  /** True for the new-session composer state (no session/thread exists yet). */
  newSession?: boolean;
  /**
   * True when choosing a working directory is mandatory (the first run, with no
   * sessions to fall back to). Makes the directory picker non-dismissable, so the
   * user must select a directory before they can reach the new-session screen.
   */
  workdirMandatory?: boolean;
  /**
   * The "Terminal" reopen button, rendered at the right end of the top region
   * (next to the collapsed timeline toggle) so the two controls share one row
   * and the timeline card can grow downward without overlapping anything else.
   * Optional: `null` (or absent) hides the slot entirely — used while the
   * terminal pane is already open, or in tests that do not exercise the
   * terminal at all.
   */
  terminalButton?: ReactNode;
}

/**
 * The right pane. For an existing session it shows the active thread's trunk as
 * a linear list (breadcrumb, branch chips, external-input marker) in the
 * scrolling body, with a pinned footer that stacks the optimistic pending-send
 * strip directly above the composer. A closed session is read-only for viewing,
 * but the composer stays available: a plain Send resumes it, and a
 * branch-from-quote both resumes it and drills into the new sub-thread. For the
 * new-session state it shows a blank prompt and a new-session composer.
 */
export function TranscriptPane({
  threads,
  activeThread,
  readOnly,
  newSession = false,
  workdirMandatory = false,
  terminalButton = null,
}: TranscriptPaneProps) {
  const client = useApiClient();
  const setActiveThread = useNavStore((state) => state.setActiveThread);
  const setTerminalOpen = useNavStore((state) => state.setTerminalOpen);
  const cancelNewSession = useNavStore((state) => state.cancelNewSession);
  const branchOrigin = useComposerStore((state) => state.branchOrigin);
  const setBranchOrigin = useComposerStore((state) => state.setBranchOrigin);
  const setNewSessionWorkdir = useComposerStore(
    (state) => state.setNewSessionWorkdir,
  );
  const resetNewSessionLaunchOptions = useComposerStore(
    (state) => state.resetNewSessionLaunchOptions,
  );
  const setNewSessionSelectedPrUrl = useComposerStore(
    (state) => state.setNewSessionSelectedPrUrl,
  );
  // The picker's open state lives in the store (not local component state) so
  // the navigator's "New" button can (re)open it without a focus transition.
  const workdirDialogOpen = useComposerStore(
    (state) => state.workdirDialogOpen,
  );
  const openWorkdirDialog = useComposerStore(
    (state) => state.openWorkdirDialog,
  );
  const closeWorkdirDialog = useComposerStore(
    (state) => state.closeWorkdirDialog,
  );
  // The focused session's external-input notice, if any. Keyed per session like
  // the permission notice; visibility is further gated to the active thread below.
  const externalInput = useLiveStore((state) =>
    activeThread
      ? noticeOf(state.notices, activeThread.session_id, 'external_input')
      : null,
  );
  // Whether the focused (closed) session just failed to resume because its
  // transcript is gone; drives the inline "cannot be resumed" notice.
  const resumeUnavailable = useLiveStore((state) =>
    activeThread
      ? noticeOf(state.notices, activeThread.session_id, 'resume_unavailable') !==
        null
      : false,
  );
  // The focused session's pending permission prompt, if any. Emitted by the
  // `PermissionRequest` hook, which fires only when an interactive dialog
  // actually appears, so it is a genuine "answer needed" signal and is shown
  // directly — no debounce. It clears on dismiss, on resolution, or when the
  // turn completes.
  const permission = useLiveStore((state) =>
    activeThread
      ? noticeOf(state.notices, activeThread.session_id, 'permission')
      : null,
  );
  // The focused session's live assistant preview, if a turn is streaming. It is
  // shown as a provisional bubble at the conversation tail while the turn
  // generates, then dropped when the turn ends (the persisted message renders
  // via the normal pipeline). Gated to the active thread below.
  const streaming = useLiveStore((state) =>
    activeThread
      ? state.streamingMessages[activeThread.session_id] ?? null
      : null,
  );
  // The focused session's running subagents (the `Agent`/`Task` tool), if any.
  // A subagent runs in its own transcript Delta never tails, so nothing else
  // appears at the conversation tail while it works — this drives a small
  // running indicator near the live bubble so the user knows it is active.
  const sessionSubagents = useLiveStore((state) =>
    activeThread
      ? state.runningSubagents[activeThread.session_id] ?? null
      : null,
  );
  // Scope the indicator to the subagents launched from the thread in view: a
  // subagent belongs to the thread that started it (`threadId`), so a different
  // thread of the same session must not show its activity. Memoized so the
  // filtered array keeps a stable identity across unrelated store updates (the
  // selector above returns the session's stored array by reference).
  const subagents = useMemo(
    () =>
      activeThread
        ? sessionSubagents?.filter((s) => s.threadId === activeThread.id) ??
          null
        : null,
    [sessionSubagents, activeThread],
  );
  // The focused session's latest context-window usage (a `status_updated`
  // snapshot's `used_percentage`, replace-latest in the store). Drives the
  // ambient fill along the composer card's top edge — right where the user is
  // about to send. `undefined` when no snapshot has arrived yet (or after a
  // `/compact` cleared it), in which case the fill is omitted rather than shown
  // at 0%. Forwarded straight through; never recomputed here.
  const contextUsage = useLiveStore((state) =>
    activeThread ? state.contextUsage[activeThread.session_id] : undefined,
  );
  const dismissPermission = useLiveStore((state) => state.dismissPermission);
  // The focused session's pending AskUserQuestion, if any. Emitted by the
  // `PreToolUse` hook for that built-in tool, so it is a genuine "answer
  // needed" signal shown directly as a readable question card. It clears on
  // dismiss, when the correlated tool_result resolves it (the user picked in
  // the terminal), or when the turn ends.
  const question = useLiveStore((state) =>
    activeThread
      ? noticeOf(state.notices, activeThread.session_id, 'question')
      : null,
  );
  const dismissQuestion = useLiveStore((state) => state.dismissQuestion);
  const dismissExternalInput = useLiveStore(
    (state) => state.dismissExternalInput,
  );

  // The sub-thread chip currently hovered; its text is highlighted in the body.
  const [hoveredBranchTitle, setHoveredBranchTitle] = useState<string | null>(
    null,
  );

  // The surface the pending strip renders for in this view: the new-session
  // screen, or the active thread. The merged rows (server open sends plus the
  // thin client complements) drive both the strip and the count used by
  // stick-to-bottom / the empty-state gate.
  const pendingSurface: PendingSurface | null = newSession
    ? { kind: 'new-session' }
    : activeThread
      ? {
          kind: 'thread',
          sessionId: activeThread.session_id,
          threadId: activeThread.id,
        }
      : null;
  const pendingEntries = usePendingSends(pendingSurface);
  const pendingCount = pendingEntries.length;

  const messagesQuery = useThreadMessagesQuery(
    client,
    activeThread?.id ?? null,
  );
  const allMessages: Message[] = messagesQuery.data?.messages ?? [];

  // Render user and assistant turns, plus meta lines (shown collapsed);
  // system/other rows are ingest-only.
  const messages = useMemo(
    () =>
      allMessages.filter(
        (m) =>
          m.role === 'user' || m.role === 'assistant' || m.role === 'meta',
      ),
    [allMessages],
  );

  // Resolve each tool call to its result across the thread (the result arrives
  // in a separate `role: "user"` message), so a call renders together with its
  // result and the result-only carrier message is not shown on its own.
  const pairing = useMemo(() => buildToolPairing(messages), [messages]);
  // Drop messages that render nothing on their own (empty thinking blocks,
  // inline-absorbed tool results, or empty content). Without this their block
  // wrapper would still emit its padding — an empty, mysteriously large gap.
  const renderedMessages = useMemo(
    () => messages.filter((m) => !messageRendersNothing(m, pairing)),
    [messages, pairing],
  );
  // The uuid of the latest assistant message: only it shows the richer two-line
  // meta (model + cwd/branch, the "current working location"); older assistant
  // messages show only `time · info`.
  const latestAssistantUuid = useMemo(() => {
    for (let i = renderedMessages.length - 1; i >= 0; i -= 1) {
      if (renderedMessages[i].role === 'assistant') {
        return renderedMessages[i].uuid;
      }
    }
    return null;
  }, [renderedMessages]);

  // Stabilize the per-message `onSelectQuote` callback so MessageItem (memoized)
  // does not re-render every item on unrelated state churn (timeline scrub,
  // branch-chip hover, bottom-reserve measurement, etc.). The dependency list
  // is just the active thread id and the zustand setter (both stable across
  // those updates), so the callback identity flips only when the user actually
  // navigates to a different thread — exactly when a fresh closure is needed.
  const activeThreadId = activeThread?.id ?? null;
  const handleSelectQuote = useCallback(
    (msg: Message, quote: string) => {
      if (!activeThreadId) {
        return;
      }
      // Branch-from-quote works on closed sessions too: the branch send
      // resumes the session before creating the child thread, so an old
      // conversation can be picked up from a selected passage.
      setBranchOrigin({
        parentThreadId: activeThreadId,
        semanticParentUuid: msg.uuid,
        locatorQuote: quote,
      });
    },
    [activeThreadId, setBranchOrigin],
  );

  // Stick-to-bottom: auto-scroll the transcript when new content arrives, but
  // only while the user is already near the bottom (so reading scrollback is
  // never yanked away). The scroll region is the Panel body.
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const stickRef = useRef(true);
  const prevPendingRef = useRef(pendingCount);

  // The pending timeline-jump intent (set atomically with a cross-lane
  // active-thread switch by the overlay). While an intent for THIS lane is
  // live the pane must land on the jump target, never the lane's tail: the
  // thread-change effect skips its stick-to-bottom jump (below) and the scroll
  // listener refuses to re-arm stick from the jump's own programmatic
  // scrollIntoView — even when the target sits near the tail and the landing
  // scroll clamps at the bottom. Only a genuine user scroll (after the intent
  // is consumed) re-arms stick. Subscribing reactively is cheap: the intent
  // flips at most twice per cross-lane jump (set, then cleared on settle). The
  // ref mirror lets the scroll listener read the latest value without
  // re-binding its native listener every render.
  const activeThreadJumpTarget = useNavStore(
    (state) => state.activeThreadJumpTarget,
  );
  const jumpTargetRef = useRef(activeThreadJumpTarget);
  useLayoutEffect(() => {
    jumpTargetRef.current = activeThreadJumpTarget;
  }, [activeThreadJumpTarget]);
  // The active thread id mirrored into a ref so the once-bound scroll listener
  // can compare a live jump intent's lane against the pane's current lane
  // without re-binding on every thread switch.
  const activeThreadIdRef = useRef<ThreadId | null>(activeThread?.id ?? null);
  useLayoutEffect(() => {
    activeThreadIdRef.current = activeThread?.id ?? null;
  }, [activeThread?.id]);

  // The bottom overlay (composer + pending strip + bottom notices) floats over
  // the scrolling body and grows with the composer's content. Reserve bottom
  // padding equal to its MEASURED height (plus the overlay inset as a gap) so
  // the last turn always rests just above it and stays readable as it grows —
  // replacing the old fixed `pb-composer-reserve`, which a grown composer would
  // cover. `null` until measured: the body falls back to the fixed reserve so a
  // first paint (or a body without an overlay) never under-reserves.
  const bottomOverlayRef = useRef<HTMLDivElement | null>(null);
  const [bottomReserve, setBottomReserve] = useState<number | null>(null);

  // In the COLLAPSED state the top row is rendered as two independent
  // absolute floating cards — the breadcrumb at top-left and the
  // {Thread + Terminal} cluster at top-right — so the conversation shows
  // through the gap between them rather than being hidden under a full-
  // width white bar. Each card pins itself with `top`/`left` (or
  // `top`/`right`) at the shared `overlay-inset` so they read as one row
  // even though they are two separate boxes. Two things compensate for
  // those cards not occupying layout space: the body reserves
  // `padding-top` equal to the visual row height (the taller of the two
  // cards) so the first message is not hidden under them on initial
  // paint, and `article[data-message-uuid]` carries a matching
  // `scroll-margin-top` (index.css) so a timeline-jump
  // `scrollIntoView({ block: 'start' })` lands the destination article
  // just BELOW the floating row rather than hidden underneath it. Both
  // feeds share one source: `Math.max(breadcrumbHeight,
  // rightClusterHeight)`, measured via the ResizeObservers below and
  // exposed as the `--delta-top-region-reserve` CSS variable.
  //
  // In the EXPANDED state the entire top region — the expanded timeline
  // card AND the row carrying the breadcrumb + Terminal underneath it —
  // sits inside a SINGLE absolute container pinned to the top of the
  // Panel's body region (`absolute top-0 left-0 right-0 z-20`). The
  // container does NOT scroll with the conversation: it anchors to the
  // Panel's relative wrapper (outside the scrolling body), not to the
  // body's scrolling content. Pinning the container — not its children
  // — is what fixes v18's regression where scrubbing the expanded
  // timeline scrolled the conversation, dragging the timeline itself
  // off-screen so the user could not scrub again after the first jump.
  // Inside the container the children use normal flow (the timeline
  // card on top, the breadcrumb + Terminal row underneath); no child
  // carries its own absolute positioning.
  //
  // Like the collapsed state, the expanded container does not occupy
  // layout space inside the body, so the body reserves matching
  // `padding-top` equal to the container's measured height. The
  // ResizeObserver effect below switches its observation targets when
  // `timelineExpanded` flips: collapsed observes the two floating
  // cards' refs and writes their max, expanded observes the single
  // container's ref and writes its height. Both feeds share the same
  // `--delta-top-region-reserve` CSS variable so the body padding-top
  // and the `scroll-margin-top` on `article[data-message-uuid]` (see
  // index.css) track whichever state is live.
  const breadcrumbOverlayRef = useRef<HTMLDivElement | null>(null);
  const rightClusterOverlayRef = useRef<HTMLDivElement | null>(null);
  const expandedContainerRef = useRef<HTMLDivElement | null>(null);
  const [topRegionReserve, setTopRegionReserve] = useState<number | null>(null);

  // `timelineExpanded` flips the entire top-row layout — not just the
  // timeline card's own collapsed/expanded chrome:
  //   - collapsed: two independent absolute floating cards (breadcrumb
  //     top-left, {Thread + Terminal} cluster top-right) over the
  //     scrolling body, plus a measured `padding-top` reserve so the
  //     first message clears them.
  //   - expanded: a SINGLE absolute container pinned to the top of the
  //     Panel's body region (does not scroll with the conversation),
  //     holding the expanded timeline card on top and a single
  //     normal-flow row of breadcrumb + Terminal underneath it. The
  //     body reserves a measured `padding-top` equal to the container's
  //     height so the first message clears it.
  // The state is shared with `ThreadTimelineOverlay` via a module-scoped
  // pub-sub inside `useTimelineExpanded` so a click on the toggle there
  // updates the layout here on the same tick. The preference is per session,
  // so both consumers must pass the SAME id — the focused session from
  // `navStore`, collapsing the new-session sentinel to `null` for the same
  // reason as in `ThreadTimelineOverlay` (the sentinel is not a real session
  // id and must not occupy a localStorage entry).
  const expandedSessionId = useNavStore((state) =>
    state.focusedSessionId === NEW_SESSION_FOCUS ? null : state.focusedSessionId,
  );
  const [timelineExpanded] = useTimelineExpanded(expandedSessionId);

  // When navigating UP to an ancestor via the breadcrumb, this holds the child
  // thread one level down toward where we were. After the ancestor renders, the
  // scroll effect brings that child's chip — where the branch sprouts — into
  // view instead of jumping to the bottom of a possibly long parent.
  const scrollToChildRef = useRef<ThreadId | null>(null);
  // The child chip to briefly flash after such a scroll, so the eye catches it.
  const [flashChildId, setFlashChildId] = useState<ThreadId | null>(null);

  // A plain click anywhere in the transcript body drops a pending branch
  // selection (the "Branch from selected text" affordance), so dismissing it no
  // longer requires hunting for the composer's ✕. Whether *this* click ended a
  // drag-select is detected directly, not inferred from selection state: some
  // engines defer collapsing the native selection until AFTER the click fires
  // (WebKit especially, when the click lands on the selected text itself), so a
  // selection-state gate would wrongly read a plain click as a drag end and
  // never dismiss. Instead, record the pointer position on `mousedown` and, on
  // `click`, keep the pending branch only when the pointer moved past a small
  // slop (a real drag) or the click was a double/triple-click (`detail > 1`,
  // which just armed the origin via word/paragraph select). Otherwise dismiss:
  // clear the origin, clear the highlight, and explicitly collapse the native
  // selection so engines that defer the collapse still drop the stale selected
  // text — otherwise the release's `mouseup` re-arm in MessageItem would pick it
  // straight back up. Attached via the body ref (like the scroll listener) since
  // the shared Panel body does not take an onClick. `branchOrigin` is read live
  // from the store so the listener does not need re-binding as it changes.
  useEffect(() => {
    const el = bodyRef.current;
    if (!el) {
      return;
    }
    let downX = 0;
    let downY = 0;
    const onMouseDown = (event: MouseEvent) => {
      downX = event.clientX;
      downY = event.clientY;
    };
    const onClick = (event: MouseEvent) => {
      const draggedPastSlop =
        Math.abs(event.clientX - downX) > CLICK_DRAG_SLOP_PX ||
        Math.abs(event.clientY - downY) > CLICK_DRAG_SLOP_PX;
      if (draggedPastSlop || event.detail > 1) {
        return;
      }
      if (useComposerStore.getState().branchOrigin !== null) {
        setBranchOrigin(null);
        clearBranchHighlight();
      }
      // Collapse any lingering native selection, outside the branch gate:
      // Chromium deselects on a plain click natively but WebKit does not
      // always, and a stale selection would both keep its highlight and
      // re-arm the branch on the next in-message mouseup. Out of the gate so
      // a click still deselects when the branch was dismissed some other way
      // (e.g. the composer's ✕, which leaves the selection alone).
      const selection = window.getSelection();
      if (selection && !selection.isCollapsed) {
        selection.removeAllRanges();
      }
    };
    el.addEventListener('mousedown', onMouseDown);
    el.addEventListener('click', onClick);
    return () => {
      el.removeEventListener('mousedown', onMouseDown);
      el.removeEventListener('click', onClick);
    };
  }, [setBranchOrigin]);

  // Recompute "is the user near the bottom?" on every scroll so the
  // stick-to-bottom effects know whether to follow new content.
  useLayoutEffect(() => {
    const el = bodyRef.current;
    if (!el) {
      return;
    }
    const onScroll = () => {
      // A timeline jump into this lane is landing: its programmatic
      // scrollIntoView fires a scroll event just like a user scroll, and for a
      // near-tail target it clamps at the bottom. Re-arming stick here would
      // glue the pane to the tail (M2) and then live content would push the
      // jump target off-screen. So while an intent for this lane is live, never
      // arm — keep stick disarmed and let the jump land on its target. The
      // intent is cleared a couple of frames after the scroll settles (see the
      // overlay), so a genuine user scroll afterwards re-arms normally.
      const jump = jumpTargetRef.current;
      if (jump !== null && jump.threadId === activeThreadIdRef.current) {
        stickRef.current = false;
        return;
      }
      stickRef.current =
        el.scrollHeight - el.scrollTop - el.clientHeight < STICK_THRESHOLD_PX;
    };
    el.addEventListener('scroll', onScroll, { passive: true });
    return () => el.removeEventListener('scroll', onScroll);
  }, []);

  // The last message's content length lets streaming/incremental appends to the
  // same message count as a content change, so growing replies keep scrolling.
  const lastContentLength =
    messages.length > 0
      ? JSON.stringify(messages[messages.length - 1].content).length
      : 0;

  // When the user sends, their own pending entry must always be visible at the
  // bottom, so force stick on a pending-count increase for the active thread.
  if (pendingCount > prevPendingRef.current) {
    stickRef.current = true;
  }
  prevPendingRef.current = pendingCount;

  // On active-thread change, reset to stick and jump to the latest of the newly
  // focused thread, and drop any lingering hover highlight (navigating away via
  // the navigator or breadcrumb does not fire the chip's mouseleave).
  useLayoutEffect(() => {
    setHoveredBranchTitle(null);
    // A breadcrumb "go up" navigation wants to land on the origin chip, not the
    // bottom: skip the stick-to-bottom jump and let the scroll effect take over.
    if (scrollToChildRef.current !== null) {
      stickRef.current = false;
      return;
    }
    // A timeline-initiated cross-lane jump switched into this lane and wants to
    // land on the message the playhead picked, not the lane's tail. Read the
    // intent synchronously in this same commit (the overlay set it atomically
    // with the active-thread switch) and skip the stick-to-bottom jump — the
    // overlay's scheduleScrollAfterRender places the pane on the target, and
    // the scroll listener above keeps stick disarmed through the landing.
    // Branches exactly like the breadcrumb precedent above. Read the store
    // directly (not the ref mirror) so this does not depend on the
    // jump-target sync layout effect having run first in this commit.
    const jumpTarget = useNavStore.getState().activeThreadJumpTarget;
    if (jumpTarget !== null && jumpTarget.threadId === activeThread?.id) {
      stickRef.current = false;
      return;
    }
    stickRef.current = true;
    const el = bodyRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [activeThread?.id, newSession]);

  // Leaving the new-session state discards the selection and closes any
  // still-open modal, so a later return starts clean. The selection is also
  // cleared on a successful new-session send by the composer. Keyed on
  // `newSession` only: enter is now handled by the Directory tab itself
  // (no more auto-opened modal), but leave-state cleanup still belongs here.
  useEffect(() => {
    if (!newSession) {
      setNewSessionWorkdir(null);
      resetNewSessionLaunchOptions();
      setNewSessionSelectedPrUrl(null);
      closeWorkdirDialog();
    }
  }, [
    newSession,
    setNewSessionWorkdir,
    resetNewSessionLaunchOptions,
    setNewSessionSelectedPrUrl,
    closeWorkdirDialog,
  ]);

  // Keyed on the rendered content changing: jump to the bottom after paint when
  // sticking.
  useLayoutEffect(() => {
    if (!stickRef.current) {
      return;
    }
    const el = bodyRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
    // The live preview grows in place (its own bubble, not a `messages` entry),
    // so its text length is a content-change signal too — keep following it.
  }, [messages.length, lastContentLength, pendingCount, streaming?.text.length]);

  // The Panel footer (notifications, Composer, queue, chips) sits outside the
  // scroll region and has a variable height: when a notice appears or the
  // composer grows, the footer expands and this `flex-1` body shrinks. That
  // shrink reduces the scroll viewport without firing a scroll or content
  // change, so the content effects above never re-run and the latest content
  // slips below the fold. Observe the body's size and re-stick on resize so the
  // bottom stays pinned. Setting `scrollTop` does not alter layout, so this
  // cannot trigger a ResizeObserver feedback loop.
  useLayoutEffect(() => {
    const el = bodyRef.current;
    if (!el) {
      return;
    }
    const observer = new ResizeObserver(() => {
      if (!stickRef.current) {
        return;
      }
      el.scrollTop = el.scrollHeight;
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const ancestry = useMemo(
    () => (activeThread ? threadAncestry(threads, activeThread.id) : []),
    [threads, activeThread],
  );
  const childMap = useMemo(
    () =>
      activeThread
        ? childThreadsByMessage(threads, activeThread.id)
        : new Map<string, Thread[]>(),
    [threads, activeThread],
  );

  // After a breadcrumb "go up", bring the chip that leads back toward the thread
  // we left into view and flash it, rather than landing at the bottom of a
  // possibly long parent. Retries across renders until the ancestor's messages
  // (and thus the chip) have rendered; clears the request once it lands.
  useEffect(() => {
    const childId = scrollToChildRef.current;
    if (childId === null || !activeThread) {
      return;
    }
    const chip = bodyRef.current?.querySelector(
      `[data-child-thread-id="${childId}"]`,
    );
    if (!chip) {
      return;
    }
    scrollToChildRef.current = null;
    // Jump instantly (no smooth animation): the flash, not motion, draws the eye.
    chip.scrollIntoView({ block: 'center' });
    setFlashChildId(childId);
  }, [activeThread?.id, renderedMessages, childMap]);

  // Clear the post-navigation chip flash after a moment.
  useEffect(() => {
    if (flashChildId === null) {
      return;
    }
    const timer = setTimeout(() => setFlashChildId(null), 1600);
    return () => clearTimeout(timer);
  }, [flashChildId]);

  // The branch text to highlight in the body: the hovered chip, or — during the
  // post-navigation flash — the chip we just scrolled to, so the same "where did
  // this branch come from" marks appear without needing to hover.
  const flashTitle = useMemo(
    () =>
      flashChildId === null
        ? null
        : threads.find((t) => t.id === flashChildId)?.title ?? null,
    [flashChildId, threads],
  );
  const highlightTitle = hoveredBranchTitle ?? flashTitle;

  // The pending-branch quote to keep highlighted: while a branch is being
  // composed (a text passage was selected for "Branch from selected text"), its
  // quote stays marked even after focus leaves for the composer textarea and the
  // native selection fades — so it is always clear what the pending branch is
  // anchored to. Scoped to the active thread it was selected from, mirroring the
  // composer's own gate (branchOrigin.parentThreadId === activeThread.id).
  const pendingBranchQuote =
    branchOrigin !== null && branchOrigin.parentThreadId === activeThread?.id
      ? branchOrigin.locatorQuote
      : null;

  // Paint the branch-origin highlight over both the hovered/flashed sub-thread
  // chip's text AND the pending-branch selection: a single highlight carrying
  // the union of ranges, so a hover mark and a pending-selection mark can show
  // together. The hovered/flashed chip (or just flashed after a breadcrumb "go
  // up") marks every occurrence of its title; the pending branch keeps its
  // selected passage visible independent of focus. Re-run when content changes
  // (rendered text nodes are recreated) so the marks track streaming and
  // refetches; clear on leave/unmount or when nothing is to be highlighted.
  useEffect(() => {
    const body = bodyRef.current;
    if (!body || (!highlightTitle && !pendingBranchQuote)) {
      clearBranchHighlight();
      return;
    }
    // Search only message bodies, not the surrounding UI: the chips render as
    // siblings of the message article, so scoping to the articles keeps a
    // chip's own title text (and banners, the pending queue, etc.) out of the
    // highlight.
    const articles = Array.from(
      body.querySelectorAll('[data-testid="message-item"]'),
    );
    const quotes = [highlightTitle, pendingBranchQuote].filter(
      (q): q is string => q !== null,
    );
    const ranges = quotes.flatMap((quote) =>
      articles.flatMap((article) => findAllQuoteRanges(article, quote)),
    );
    setBranchHighlight(ranges);
    return () => clearBranchHighlight();
  }, [highlightTitle, pendingBranchQuote, messages.length, lastContentLength]);

  const breadcrumbItems = ancestry.map((thread, index) => ({
    key: thread.id,
    label: thread.title,
    onClick: () => {
      // Remember the child one level down toward where we are, so after landing
      // on this ancestor the scroll effect reveals the chip where that branch
      // sprouts (a long parent thread otherwise hides where the branch began).
      const childOnPath = ancestry[index + 1];
      scrollToChildRef.current = childOnPath ? childOnPath.id : null;
      setActiveThread(thread.id);
    },
  }));

  // Show the breadcrumb only when the active thread actually has ancestors, i.e.
  // you are drilled into a sub-thread (ancestry is [main › … › current]). On the
  // main thread the ancestry is just [main], a lone crumb that reads as abrupt
  // noise — so it stays hidden even when the session has branched elsewhere.
  const isOnSubThread = ancestry.length > 1;

  const showExternalInput =
    !newSession &&
    activeThread !== null &&
    externalInput !== null &&
    externalInput.threadId === activeThread.id;

  // Show the live assistant preview on the thread it belongs to whenever a
  // preview exists with text. `done` only means every chunk of the message has
  // arrived, NOT that the turn ended — the preview stays until the persisted
  // message renders, so it is the buffer's presence (not `done`) that gates
  // visibility.
  //
  // The final guard makes visibility a function of the current persisted state,
  // not of event timing: once the thread's persisted messages already contain
  // an assistant message with the streamed text, the live bubble is suppressed.
  // The persisted line can land via the transcript refetch BEFORE the turn-end
  // event clears the preview buffer (and a turn can persist an earlier message
  // while `turn_completed` only fires at the very end), so without this guard
  // the same text would briefly show twice — the live bubble and the persisted
  // copy. The turn-end clear in liveStore stays as cleanup; this guard is what
  // eliminates the visible duplicate regardless of event/refetch ordering.
  const showStreaming =
    !newSession &&
    activeThread !== null &&
    streaming !== null &&
    streaming.threadId === activeThread.id &&
    streaming.text.length > 0 &&
    !persistedHasStreamedText(messages, streaming.text, streaming.done);

  // The new-session state has no session id yet; the composer targets a fresh
  // spawn. An existing thread targets that thread (a resume on a closed session).
  // A resume-impossible session is the exception: every send and branch would
  // re-trigger the failed resume, so it gets no composer at all and becomes a
  // pure read-only viewer (see the footer below).
  const composer = newSession ? (
    <Composer mode={{ kind: 'new-session' }} />
  ) : activeThread && !resumeUnavailable ? (
    <Composer
      mode={{
        kind: 'thread',
        activeThread,
        readOnly,
      }}
    />
  ) : undefined;

  // Floating layers over the scrolling transcript (see Panel's `overlay`). They
  // sit on top of the conversation rather than in flow, so a notice appearing or
  // disappearing never resizes the scroll viewport — the tail the user is
  // reading stays put instead of jumping. The body reserves a fixed bottom
  // padding (below) so resting content clears the bottom (composer) layer.

  // The permission notice floats at the top-right, deliberately away from the
  // conversation tail and the input. Pinned above the input (its old home) it
  // would sit exactly where the user reads. Kept narrow so it does not blanket
  // the transcript. It clears on dismiss (the entry stays, flagged, so a
  // refetch cannot resurrect the card), on a decision/resolution, or when the
  // turn completes.
  const permissionOverlay = permission && !permission.dismissed && activeThread && (
    <PermissionNoticeCard
      notice={permission}
      onOpenTerminal={() => setTerminalOpen(true)}
      onDismiss={() => dismissPermission(activeThread.session_id)}
    />
  );

  // The interactive question card renders INLINE at the conversation tail (in
  // the scrolling body, after the rendered messages and the live-streamed
  // bubble), not in the bottom overlay — so the choices "hang off" the
  // conversation right after the assistant's streamed preamble, where the user
  // is reading, rather than floating over it. The user answers from here: a
  // single-select click, a multi-select toggle + Submit, or a per-question
  // choice across a multi-question call, all POSTed to the answer endpoint,
  // which injects the selection keystrokes into the session's TUI pane. The
  // authoritative clear stays the existing resolution path (the `tool_result`
  // resolving the question's request row), so no extra clear logic is needed.
  // An "Open terminal" fallback remains in the card for a misfired injection.
  // Gate the question card to the thread the question was asked on, mirroring
  // how `showExternalInput` and `showStreaming` gate by `=== activeThread.id`:
  // AskUserQuestion belongs to the in-flight turn's thread, so the card must
  // not show on the session's other threads.
  const questionCard = question &&
    !question.dismissed &&
    activeThread &&
    question.threadId === activeThread.id && (
    <QuestionCard
      notice={question}
      onAnswer={(selections) =>
        // Return the POST so the card can await it: a 409 (already answered /
        // stale), a 400 (malformed), or a network failure rejects, and the card
        // surfaces an inline error, re-enables its controls for a retry, and
        // emphasizes the terminal fallback. On success the authoritative clear
        // still arrives via the resolution path.
        client.answerQuestion(
          activeThread.session_id,
          question.requestId,
          selections,
        )
      }
      onCancel={() =>
        // Cancel the question in the TUI itself (a single Escape cancels the
        // whole call). Return the POST so the card can await it: a 409 (already
        // resolved / stale) or a network failure rejects, and the card surfaces
        // an inline error, re-enables its controls for a retry, and emphasizes
        // the terminal fallback. On success the authoritative clear still
        // arrives via the resolution path (the `is_error` tool_result).
        client.cancelQuestion(activeThread.session_id, question.requestId)
      }
      onOpenTerminal={() => setTerminalOpen(true)}
      onDismiss={() => dismissQuestion(activeThread.session_id)}
    />
  );

  // The bottom layer: the composer plus the notices that must stay next to the
  // input — the closed/external-input banners and, crucially, the pending-send
  // strip, which the user reads to decide whether to hold a send. For a
  // resume-impossible session it is just the "cannot resume" notice, replacing
  // the input entirely — there is nothing useful to type.
  let bottomContent: ReactNode;
  if (resumeUnavailable && !newSession) {
    bottomContent = (
      <div
        className="flex items-center gap-2 rounded border border-danger/30 bg-danger/10 px-2 py-1 text-caption text-danger"
        data-testid="resume-unavailable-notice"
        role="alert"
      >
        <Badge tone="warning">cannot resume</Badge>
        <span>
          This session cannot be resumed: its conversation transcript is no
          longer available. Its history above stays readable.
        </span>
      </div>
    );
  } else if (composer) {
    // Whether the upper (notices) card has anything to show. Each of these
    // conditions matches exactly one child it gates — `readOnly` the closed
    // notice, `showExternalInput` the external-input notice, and a non-empty
    // `pendingEntries` the pending strip (`PendingQueue` itself renders null when
    // empty) — so the card is omitted entirely rather than rendering an empty
    // box when none of them are present.
    const hasNotices =
      (readOnly && !newSession) ||
      (showExternalInput && activeThread !== null) ||
      pendingEntries.length > 0;
    bottomContent = (
      <>
        {/* Upper card: status notices + the pending-send strip, kept visually
            separate from the composer so the (now borderless) textarea has a
            clean boundary of its own. The question card is NOT in this stack: it
            renders inline at the conversation tail in the scrolling body (see
            questionCard above), so the choices follow the streamed preamble in
            the flow instead of floating over it. */}
        {hasNotices && (
          <div
            className={`${FLOATING_CARD_CLASS} space-y-2 px-3 py-2`}
            data-testid="bottom-notices"
          >
            {readOnly && !newSession && (
              <div
                className="flex items-center gap-2 rounded border border-border-default bg-surface-elevated px-2 py-1 text-caption text-fg-subtle"
                data-testid="readonly-notice"
              >
                <Badge tone="neutral">closed</Badge>
                <span>
                  This session is closed. Sending a message resumes it.
                </span>
              </div>
            )}

            {showExternalInput && activeThread && (
              <div
                className="flex items-start gap-2 rounded border border-info/30 bg-info/10 px-2 py-1 text-caption"
                data-testid="external-input-notice"
              >
                <Badge className="shrink-0" tone="info">
                  external input
                </Badge>
                <span className="min-w-0 flex-1 line-clamp-2 break-words text-fg-muted">
                  {externalInput.prompt}
                </span>
                <Button
                  className="shrink-0"
                  size="sm"
                  variant="ghost"
                  onClick={() => dismissExternalInput(activeThread.session_id)}
                >
                  Dismiss
                </Button>
              </div>
            )}

            <PendingQueue entries={pendingEntries} />
          </div>
        )}

        {/* Composer card: the new-session launch pickers (which parameterize the
            spawn) sit directly above the input they configure. The focused
            session's context-window usage rides the card's TOP EDGE as a thin
            ambient fill (the border doubles as the track), filled from the left
            to `used_percentage`%, with the numeric `NN%` small at the edge —
            right where the user is about to send. Omitted entirely when no
            snapshot is available (or after `/compact`), rather than shown at 0%. */}
        <div
          className={`relative ${FLOATING_CARD_CLASS} px-3 py-2`}
          data-testid="composer-card"
        >
          {contextUsage !== undefined && (
            <div
              className="pointer-events-none absolute inset-x-0 top-0"
              data-testid="composer-context-bar"
            >
              {/* The card's top border is the track; this fill runs along it from
                  the RIGHT edge leftward to the usage percentage, so the bar's
                  growing tip stays next to the `%` readout. A real DOM bar. */}
              <div
                className="absolute right-0 top-0 h-0.5 rounded-tr-md bg-fg-muted"
                style={{ width: `${Math.min(100, Math.max(0, contextUsage))}%` }}
                data-testid="composer-context-fill"
                role="meter"
                aria-label="Context window usage"
                aria-valuenow={Math.round(
                  Math.min(100, Math.max(0, contextUsage)),
                )}
                aria-valuemin={0}
                aria-valuemax={100}
              />
              <span className="group/ctx pointer-events-auto absolute right-0.5 top-0.5 z-10">
                <span
                  className="cursor-help px-1 py-1 text-caption leading-none tabular-nums text-fg-subtle"
                  data-testid="composer-context-label"
                  tabIndex={0}
                  aria-label="Context window usage"
                >
                  {Math.round(contextUsage)}%
                </span>
                {/* Opens upward (the bar sits at the very top of the composer)
                    so it never covers the textarea below. */}
                <span
                  role="note"
                  data-testid="composer-context-popover"
                  className="pointer-events-none absolute bottom-full right-0 z-10 mb-1 hidden w-max max-w-xs rounded-md border border-border-default bg-surface px-2.5 py-1.5 text-caption text-fg-muted shadow-lg group-hover/ctx:block group-focus-within/ctx:block"
                >
                  Context window usage
                </span>
              </span>
            </div>
          )}
          {/* Flow content sits in its own `space-y-2` wrapper so the absolute
              context bar above is not counted as a spacing sibling (which would
              push the composer down by a row gap). */}
          <div className="space-y-2">
            {/* A directory is chosen: show it as a chip with a ✎ to change it
                (the ✎ reopens the picker without resetting the selection). The
                chip renders nothing when no directory is selected, so there is
                no button to (re)open the picker from here — that is done via
                "New". */}
            {newSession && <WorkdirChip onEdit={openWorkdirDialog} />}
            {/* Below the directory chip: when the selected directory is a git
                repo, an opt-in to start the session in a fresh worktree (with a
                start-point choice). Renders nothing for a non-git directory. */}
            {newSession && <WorktreeOptions />}
            {newSession && <LaunchOptionsPicker />}
            {composer}
          </div>
        </div>
      </>
    );
  }

  // The bottom layer floats over the body as a stack of cards inset from the
  // left, right, and bottom edges. This wrapper is transparent and only
  // positions and MEASURES the stack (the body reserves bottom padding equal to
  // its measured height — see the effect below — so resting content clears it
  // however tall the composer grows); its children carry the actual floating-card
  // chrome ({@link FLOATING_CARD_CLASS}). For a resume-impossible session the
  // single "cannot resume" notice floats on its own with no composer beneath it.
  const bottomOverlay = bottomContent && (
    <div
      ref={bottomOverlayRef}
      data-testid="bottom-overlay"
      className="pointer-events-auto absolute inset-x-overlay-inset bottom-overlay-inset flex flex-col gap-2"
    >
      {bottomContent}
    </div>
  );
  // Whether a bottom overlay is mounted at all. This is the ONLY real state the
  // measure effect below derives from: the effect re-binds its ResizeObserver
  // when the overlay node appears or disappears, and the observer itself
  // handles every subsequent size change (composer auto-grow, banners). Keying
  // the effect on this boolean instead of `bottomContent`'s JSX identity (a
  // fresh object every render) stops it from re-running — and rewriting the
  // body's `scrollTop` while stick is armed — on every unrelated render.
  const hasBottomOverlay = bottomContent != null;

  // Track the bottom overlay's actual height and drive the body's bottom padding
  // from it (height + the overlay inset as a gap), so resting content clears the
  // composer however tall it grows — the last turn sits just above it rather than
  // being covered by a fixed reserve. When the overlay grows (the composer
  // expands as you type, or a banner appears) the body's padding grows and so
  // does its scrollHeight; re-stick in the same measurement so, while sticking,
  // the tail stays pinned just above the composer.
  //
  // This observes the OVERLAY, never the body, and only ever writes
  // `bottomReserve` (the body's padding) and the body's `scrollTop` — never the
  // overlay's own size — so it cannot feed its own observation back into a loop.
  // The body's ResizeObserver (above) observes the body and only writes
  // `scrollTop`, so the two stay independent. Re-bind when the overlay's presence
  // changes (e.g. a resume-unavailable session drops the composer) so the ref
  // tracks the live node; clear the reserve when there is no overlay so the body
  // falls back to the fixed token.
  useLayoutEffect(() => {
    const overlay = bottomOverlayRef.current;
    if (!overlay) {
      // No bottom overlay (e.g. resume-unavailable drops the composer): clear
      // the measured reserve so the body falls back to the fixed token below.
      setBottomReserve(null);
      return;
    }
    const apply = () => {
      // While a branch is being composed, the "Branch from selected text" banner
      // adds height to this overlay. Folding that into the body's bottom reserve
      // (and the stick-to-bottom re-scroll) shifts the transcript the instant
      // text is selected — moving the very selection the user is trying to
      // adjust, and making it hard to read. So while a branch is pending, hold
      // the reserve and skip the re-scroll: the banner floats over the transcript
      // tail instead of pushing it. The reserve recomputes once the branch is
      // cleared or sent (this effect re-runs and apply() proceeds normally).
      if (useComposerStore.getState().branchOrigin !== null) {
        return;
      }
      const height = overlay.getBoundingClientRect().height;
      setBottomReserve(
        height + overlayInsetPx(overlay) + BODY_BOTTOM_READING_GAP_PX,
      );
      const body = bodyRef.current;
      if (body && stickRef.current) {
        body.scrollTop = body.scrollHeight;
      }
    };
    apply();
    const observer = new ResizeObserver(apply);
    observer.observe(overlay);
    return () => observer.disconnect();
  }, [hasBottomOverlay]);

  // Track whichever top-region surface is currently mounted and drive
  // the body's `--delta-top-region-reserve` CSS variable from its
  // measured height. The variable feeds both the body's `padding-top`
  // (so the first message does not render under the pinned region) and
  // the `scroll-margin-top` rule on `article[data-message-uuid]` (see
  // index.css), so a timeline-jump `scrollIntoView({ block: 'start' })`
  // lands the destination article just below the pinned region rather
  // than hidden underneath it.
  //
  // The observation targets switch with `timelineExpanded`:
  //   - collapsed: observe BOTH `breadcrumbOverlayRef` and
  //     `rightClusterOverlayRef` (the two independent absolute floating
  //     cards) and write `max(breadcrumbHeight, rightClusterHeight)` —
  //     the visual row height those two cards form together.
  //   - expanded: observe the single `expandedContainerRef` (the
  //     absolute container pinned to the top of the Panel's body region,
  //     holding the timeline card and the breadcrumb+Terminal under-row
  //     in normal flow) and write the container's total height.
  //
  // Re-running the effect on the `timelineExpanded` flip disconnects
  // the previous observer and binds a fresh one to the new state's
  // node(s). The single ResizeObserver instance per render handles its
  // own cleanup; observing nodes that no longer exist is impossible
  // because the JSX for the unmounted state is not rendered. Re-bind
  // also fires when the collapsed cards' presence may have flipped (a
  // new-session ↔ active-thread swap, or the breadcrumb appearing for
  // a sub-thread navigation) so the initial measurement still lands on
  // the new nodes. The observer reads the cards/container size only
  // and writes the body's CSS variable, never the observed nodes' own
  // size, so it cannot feed back into a loop.
  useLayoutEffect(() => {
    if (timelineExpanded) {
      const container = expandedContainerRef.current;
      if (!container) {
        setTopRegionReserve(null);
        return;
      }
      const apply = () => {
        setTopRegionReserve(container.getBoundingClientRect().height);
      };
      apply();
      const observer = new ResizeObserver(apply);
      observer.observe(container);
      return () => observer.disconnect();
    }
    const breadcrumb = breadcrumbOverlayRef.current;
    const cluster = rightClusterOverlayRef.current;
    if (!breadcrumb && !cluster) {
      setTopRegionReserve(null);
      return;
    }
    const apply = () => {
      const breadcrumbHeight = breadcrumb
        ? breadcrumb.getBoundingClientRect().height
        : 0;
      const clusterHeight = cluster
        ? cluster.getBoundingClientRect().height
        : 0;
      setTopRegionReserve(Math.max(breadcrumbHeight, clusterHeight));
    };
    apply();
    const observer = new ResizeObserver(apply);
    if (breadcrumb) {
      observer.observe(breadcrumb);
    }
    if (cluster) {
      observer.observe(cluster);
    }
    return () => observer.disconnect();
  }, [
    // Re-bind when the observed surface flips (collapsed ↔ expanded) or
    // when the collapsed cards' presence may have changed, so the
    // initial measurement still lands on the new node(s) (or the
    // reserve is cleared when no node is present).
    timelineExpanded,
    newSession,
    activeThread?.id,
    isOnSubThread,
    terminalButton,
  ]);

  // The top region layout splits into two distinct shapes by
  // `timelineExpanded`:
  //
  //   1. COLLAPSED (default): two independent absolute floating cards
  //      sit at the top of the conversation panel —
  //         [breadcrumb]                        [{Thread} {Terminal}]
  //      The breadcrumb pins to top-left; the {Thread + Terminal}
  //      cluster pins to top-right. Both share the same `overlay-inset`
  //      top/side offsets so they read as one row even though they are
  //      two boxes, and the conversation shows through the gap between
  //      them — there is NO full-width white bar. Each piece keeps its
  //      own card chrome (a breadcrumb card; the Thread/Terminal pills
  //      already carry `bg-surface shadow-md` via
  //      `TIMELINE_TOGGLE_BUTTON_CLASS` / `TERMINAL_TOGGLE_BUTTON_CLASS`)
  //      so the floating elements stay legible against any conversation
  //      content scrolling underneath. The body reserves
  //      `padding-top: var(--delta-top-region-reserve)` equal to the
  //      taller of the two cards, so the first message clears them on
  //      initial paint.
  //
  //   2. EXPANDED: a SINGLE absolute container pinned to the top of
  //      the Panel's body region holds the entire top region —
  //         [expanded timeline card                                  ]
  //         [breadcrumb] [flex-1 spacer]                  [{Terminal}]
  //      The container is `absolute top-0 left-0 right-0 z-20`, so it
  //      anchors to the Panel's relative wrapper (outside the
  //      scrolling body) and STAYS PINNED across conversation scroll.
  //      Inside the container the children use normal flow — the
  //      timeline card on top, the breadcrumb + Terminal row directly
  //      underneath — no child carries its own absolute positioning.
  //      Pinning the container — not its children — is what fixes the
  //      v18 regression where the expanded timeline scrolled away with
  //      the conversation after the first scrub, breaking subsequent
  //      scrubs. The body reserves `padding-top` equal to the
  //      container's measured height (same `--delta-top-region-reserve`
  //      mechanism as collapsed) so the first message clears it. No
  //      Thread icon in the under-row — the expanded card itself
  //      replaces it.
  //
  // The Panel's body wrapper (`Panel.tsx`) already carries
  // `position: relative`, so the absolute floating cards / container
  // anchor to the scroll viewport rather than to the scrolling content
  // itself — they stay glued to the top edge while the conversation
  // scrolls underneath. A high z-index (`z-20`) keeps them above any
  // other in-flow content (streaming bubble, chip rows).
  const showTimeline = !newSession && activeThread !== null;
  const showBreadcrumb = isOnSubThread;
  // The single floating breadcrumb card (collapsed state only) — pinned
  // top-left via `top-overlay-inset` / `left-overlay-inset`, with the
  // same card chrome as the in-flow breadcrumb used in the expanded
  // state. The ref drives one half of the body's measured reserve.
  const collapsedBreadcrumbCard = showBreadcrumb && (
    <div
      ref={breadcrumbOverlayRef}
      data-testid="transcript-breadcrumb-overlay"
      className={`${FLOATING_CARD_CLASS} pointer-events-auto absolute left-overlay-inset top-overlay-inset z-20 self-start px-3 py-1.5`}
    >
      <Breadcrumb items={breadcrumbItems} />
    </div>
  );
  // The single floating right-side cluster (collapsed state only) —
  // pinned top-right via `top-overlay-inset` / `right-overlay-inset`,
  // with the Thread toggle and Terminal pills side-by-side inside it.
  // The cluster wrapper carries NO shared background or border on
  // purpose: each pill already has its own white card chrome via
  // `TIMELINE_TOGGLE_BUTTON_CLASS` / `TERMINAL_TOGGLE_BUTTON_CLASS`, so
  // wrapping them in another white bar would re-introduce the v17
  // top-bar look the v18 design retracts.
  const showRightCluster = showTimeline || terminalButton;
  const collapsedRightCluster = showRightCluster && (
    <div
      ref={rightClusterOverlayRef}
      data-testid="transcript-top-row"
      data-expanded="false"
      className="pointer-events-auto absolute right-overlay-inset top-overlay-inset z-20 flex items-start gap-2"
    >
      {showTimeline && activeThread !== null && (
        <ThreadTimelineOverlay
          threads={threads}
          activeThreadId={activeThread.id}
          conversationBodyRef={bodyRef}
        />
      )}
      {terminalButton}
    </div>
  );
  // The top region itself. Two completely different shapes by
  // `timelineExpanded` (see the comment above for the full rationale):
  //
  //   - collapsed: a transparent, layout-less wrapper
  //     (`display: contents`) hosting two independent absolute floating
  //     cards (each pinned to its own corner of the Panel body region).
  //
  //   - expanded: a SINGLE absolute container pinned to the top of the
  //     Panel body region (`absolute top-0 left-0 right-0 z-20`)
  //     holding the expanded timeline card on top and a single
  //     normal-flow row of breadcrumb + Terminal underneath it. The
  //     container itself takes no layout space inside the scrolling
  //     body — the body reserves matching `padding-top` from the
  //     ResizeObserver-driven `--delta-top-region-reserve`, mirroring
  //     the collapsed state's mechanism.
  //
  // Neither shape paints a shared white bar across the full top edge,
  // so the v17 look the v18 design retracted is not re-introduced. The
  // outer `transcript-top-region` wrapper carries the testid in both
  // states for tests to locate.
  const topRegion = timelineExpanded ? (
    showTimeline ? (
      <div
        ref={expandedContainerRef}
        data-testid="transcript-top-region"
        data-expanded="true"
        // Single absolute container pinned to the top of the Panel
        // body region; anchored to Panel's relative wrapper (outside
        // the scrolling body), so it STAYS PINNED across conversation
        // scroll — the v19 fix for the v18 regression. Children use
        // normal flow inside.
        className="pointer-events-auto absolute left-0 right-0 top-0 z-20 flex flex-col gap-2 px-3 pt-3"
      >
        {activeThread !== null && (
          <ThreadTimelineOverlay
            threads={threads}
            activeThreadId={activeThread.id}
            conversationBodyRef={bodyRef}
          />
        )}
        {(showBreadcrumb || terminalButton) && (
          <div
            data-testid="transcript-top-row"
            data-expanded="true"
            className="flex items-center gap-2 pb-2"
          >
            {showBreadcrumb ? (
              <div className={`${FLOATING_CARD_CLASS} px-3 py-1.5`}>
                <Breadcrumb items={breadcrumbItems} />
              </div>
            ) : (
              <span />
            )}
            <span className="flex-1" />
            {terminalButton}
          </div>
        )}
      </div>
    ) : null
  ) : (
    (showBreadcrumb || showRightCluster) && (
      <div
        data-testid="transcript-top-region"
        data-expanded="false"
        // Layout-less wrapper: zero height in the document flow, just a
        // host for the two absolute children below. The body's measured
        // padding-top reserve (driven by Math.max of the two children's
        // heights) clears space for them above the first message.
        className="contents"
      >
        {collapsedBreadcrumbCard}
        {collapsedRightCluster}
      </div>
    )
  );

  return (
    <Panel
      bodyRef={bodyRef}
      // Reserve bottom space for the floating composer overlay so the last turn
      // rests just above it instead of behind it. The reserve is MEASURED from
      // the overlay's actual height (see the effect above) and applied as the
      // body's `padding-bottom`, so it tracks the composer as it auto-grows — the
      // tail stays readable however tall the input gets. Until the first
      // measurement lands (and whenever there is no overlay) it falls back to the
      // fixed `--delta-composer-body-reserve` token, so the body never
      // under-reserves on first paint.
      //
      // In the COLLAPSED state the top row's two cards (breadcrumb,
      // {Thread + Terminal} cluster) float as independent absolute
      // overlays over the body, so they carry no layout height — the
      // body must reserve an equivalent top gap, otherwise the first
      // message would render under them on initial paint. The reserve
      // is `Math.max(breadcrumbHeight, rightClusterHeight)`. In the
      // EXPANDED state the same mechanism reserves space for the single
      // pinned container instead — the container is also `absolute`
      // (so it stays glued to the top across conversation scroll, the
      // v19 fix) and carries no layout height inside the body; the
      // reserve is the container's measured height. Both feeds use the
      // same `--delta-top-region-reserve` CSS variable (driven by the
      // ResizeObserver effect on `breadcrumbOverlayRef` +
      // `rightClusterOverlayRef` when collapsed, on `expandedContainerRef`
      // when expanded), so the body's `padding-top` and the
      // `scroll-margin-top` rule on `article[data-message-uuid]` (index.css)
      // — used by timeline-jump `scrollIntoView({ block: 'start' })` to
      // land the destination article just BELOW the pinned region —
      // both track whichever state is live.
      //
      // `scrollbar-none` hides the body's scrollbar entirely (it still scrolls
      // via wheel/trackpad): the conversation reads as a clean page, and the
      // floating composer card already sits over the right edge where a bar
      // would otherwise run. `scrollbar-none` is declared after Panel's
      // default `scrollbar-hover`, so it wins when both are present.
      bodyClassName="scrollbar-none"
      bodyStyle={{
        paddingTop: 'var(--delta-top-region-reserve, 0)',
        paddingBottom:
          bottomReserve !== null
            ? `${bottomReserve}px`
            : 'var(--delta-composer-body-reserve)',
        ...(topRegionReserve !== null
          ? ({
              '--delta-top-region-reserve': `${topRegionReserve}px`,
            } as CSSProperties)
          : {}),
      }}
      header={
        // The new-session screen pins the PR / Repository / Directory tabs to
        // the Panel's sticky header (Panel header lives outside the scroll
        // region), so they stay put while the active tab's list scrolls
        // underneath. The "New session" label is dropped — the tabs convey it.
        newSession ? (
          <NewSessionTabBar />
        ) : // `undefined` (not `null`) so Panel drops the header bar entirely.
        // The breadcrumb / timeline / Terminal-toggle are rendered in flow at
        // the top of the body via `topRegion`; on the main thread there is no
        // breadcrumb but the timeline + Terminal still ride along.
        undefined
      }
      overlay={
        <>
          {permissionOverlay}
          {bottomOverlay}
        </>
      }
    >
      {topRegion}
      {newSession && (
        <div className="space-y-4 px-3 pt-3 pb-2" data-testid="new-session-empty">
          {/* The active tab's content (PR / Repository / Directory). The
              tab strip itself sits in the Panel's sticky header above; this
              body only renders the chosen tab. The composer card below
              stays where it was — the tabs only decide HOW the
              `newSessionWorkdir` etc. get populated, not the send body. */}
          <NewSessionPanel />
          {/* The auto-open modal picker is retired (the Directory tab
              exposes Recent + Browse inline), but the standalone Dialog is
              still available so the WorkdirChip's pencil button — which
              calls `openWorkdirDialog` — can reopen a focused picker for a
              quick change. The composer card writes both the chip and the
              chip's edit affordance into the bottom overlay. */}
          <WorkdirDialog
            open={workdirDialogOpen}
            dismissable={!workdirMandatory}
            onClose={() => {
              closeWorkdirDialog();
              // Dismissing the picker without a directory cancels the
              // new-session intent and returns to the previously-focused
              // session — but only when there is one to return to. With no
              // sessions, new-session is the mandatory default, so
              // cancelNewSession() is a no-op and we stay. Read the live
              // store value to avoid a stale closure.
              if (!useComposerStore.getState().newSessionWorkdir) {
                cancelNewSession();
              }
            }}
          />
        </div>
      )}

      {!newSession && messagesQuery.isLoading && (
        <p className="px-3 py-4 text-secondary text-fg-subtle">Loading transcript…</p>
      )}

      {!newSession &&
        !messagesQuery.isLoading &&
        messages.length === 0 &&
        pendingCount === 0 && (
          <p className="px-3 py-4 text-secondary text-fg-subtle">
            No messages yet. Send the first message below.
          </p>
        )}

      {renderedMessages.map((message) => {
        const children = childMap.get(message.uuid) ?? [];
        // A "tool" message renders as a Collapsible card: an assistant tool call
        // (`tool_use`) or a standalone tool result (`tool_result` — paired
        // results are already dropped as empty-rendering, so any that survive are
        // orphans Claude delivers as `role: user`). The check comes before the
        // user/prose split so an orphan tool_result is treated as a tool card,
        // not a user turn.
        const isToolTurn = message.content.some(
          (block) => block.type === 'tool_use' || block.type === 'tool_result',
        );
        // Tool rows, the harness-injected task-notification card (a collapsed
        // `<task-notification>` user turn), and meta lines all render as nested
        // aside cards: they are tightened and left-indented so they read as
        // nested steps, distinct from prose.
        const isNestedCard =
          isToolTurn || isTaskNotificationMessage(message) || message.role === 'meta';
        const topGap = isNestedCard
          ? 'pt-0.5'
          : message.role === 'user'
            ? 'pt-2'
            : 'pt-1.5';
        const bottomGap = isNestedCard
          ? 'pb-0.5'
          : message.role === 'user'
            ? 'pb-1'
            : 'pb-2';
        // Inset a nested card (left margin) so it reads as a nested step.
        const indent = isNestedCard ? 'ml-6 mr-0' : '';
        return (
          // One block per message: the message and its sub-thread chips. The
          // block owns the vertical rhythm (not the message article), so adjacent
          // messages are separated by a consistent gap while the chips hug their
          // parent message just below it (see their small pt).
          <div key={message.uuid} className={`${topGap} ${bottomGap} ${indent}`}>
            <MessageItem
              message={message}
              pairing={pairing}
              isLatest={message.uuid === latestAssistantUuid}
              onSelectQuote={handleSelectQuote}
            />
            {children.length > 0 && (
              <div className="flex flex-wrap justify-end gap-1.5 px-3 pt-1.5">
                {children.map((child) => (
                  // The wrapper carries the scroll target id so a breadcrumb
                  // "go up" can bring this chip into view (see scrollToChildRef).
                  <span
                    key={child.id}
                    data-child-thread-id={child.id}
                    className="inline-flex"
                  >
                    <Chip
                      // The chip's clickable pill shape conveys that it enters
                      // the branch, so no "[enter →]" label is shown. The
                      // accessible name still says "Enter <title>" for screen
                      // readers (and to distinguish it from the navigator tree
                      // node of the same branch).
                      ariaLabel={`Enter ${child.title}`}
                      // Briefly ring the chip after a breadcrumb "go up" scrolls
                      // it into view, so the eye catches where the branch began.
                      className={
                        flashChildId === child.id
                          ? 'ring-2 ring-accent-hover ring-offset-1'
                          : undefined
                      }
                      // Clear the hover highlight on click: entering the branch
                      // does not fire mouseleave, so the mark would otherwise
                      // linger across the whole child thread.
                      onClick={() => {
                        setHoveredBranchTitle(null);
                        setActiveThread(child.id);
                      }}
                      // Hovering the chip marks every occurrence of its text in
                      // the body, so it is clear what the branch was about.
                      onMouseEnter={() => setHoveredBranchTitle(child.title)}
                      onMouseLeave={() => setHoveredBranchTitle(null)}
                    >
                      ⤷ {child.title}
                    </Chip>
                  </span>
                ))}
              </div>
            )}
          </div>
        );
      })}

      {/* The provisional live assistant bubble, appended at the tail while the
          turn streams. It mirrors MessageItem's assistant styling (left, plain
          bubble) and renders the partial text as Markdown via the same shared
          AssistantMarkdown component as the persisted message, so streamed
          prose looks identical live and after handoff. It carries its own
          testid so it never inflates message-item counts. Chunks are
          line-grained, so partial Markdown is usually coherent; any transient
          oddness while a code fence/table is still building resolves on the
          next chunk. The blinking caret renders as a SIBLING after the
          Markdown (not inside its text), so an in-progress final line never
          corrupts Markdown parsing. It is dropped on turn end, when the
          persisted message takes over. */}
      {showStreaming && streaming && (
        <div className="pt-1.5 pb-2">
          <article
            className="px-3 text-body"
            data-role="assistant"
            data-testid="streaming-message"
          >
            <div className="rounded-lg bg-surface-elevated px-3 py-2 text-fg">
              <div className="flex items-end">
                <AssistantMarkdown text={streaming.text} />
                {/* The blinking caret signals "still generating", so it shows
                    only while the stream is in progress (!done). Once the final
                    chunk has arrived the bubble lingers (caret-less) until the
                    persisted message lands and suppression swaps it out, so a
                    completed reply never shows a misleading "generating" caret
                    during the handoff. */}
                {!streaming.done && (
                  <span
                    className="ml-0.5 inline-block animate-caret-blink text-fg-muted"
                    aria-hidden="true"
                  >
                    ▌
                  </span>
                )}
              </div>
            </div>
          </article>
        </div>
      )}

      {/* A running-subagent indicator at the conversation tail: a subagent runs
          in its own (untailed) transcript, so without this the pane would look
          idle while it works. Shown only for an active thread (not the
          new-session screen). */}
      {!newSession && activeThread !== null && subagents !== null && (
        <SubagentRunningIndicator subagents={subagents} />
      )}

      {/* The interactive question card, inline at the very tail of the
          conversation: after the rendered messages and the live-streamed
          bubble, so the choices appear right after the assistant's preamble.
          Inset to align with the prose, in its own block so the bottom padding
          (pb-composer-reserve) keeps it clear of the floating composer. */}
      {questionCard && <div className="px-3 pt-1.5 pb-2">{questionCard}</div>}
    </Panel>
  );
}
