import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from 'react';
import { threadAncestry, type ThreadId } from '@delta/model';
import type { Message, Thread } from '@delta/wire-gen';
import { useThreadMessagesQuery } from '@delta/api-client';
import { Badge, Panel } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useComposerStore } from '../../store/composerStore';
import { noticeOf, useLiveStore } from '../../store/liveStore';
import type { ComposerMode } from '../composer/Composer';
import {
  useSendActivity,
  type SendActivitySurface,
} from '../composer/useSendActivity';
import { WorkdirDialog } from '../composer/WorkdirDialog';
import { NewSessionPanel } from '../new-session/NewSessionPanel';
import { NewSessionTabBar } from '../new-session/NewSessionTabBar';
import { AssistantMarkdown } from './AssistantMarkdown';
import { isUnknownCommandNoticeMessage } from './claudeFormat';
import { SubagentRunningIndicator } from './SubagentRunningIndicator';
import { useTimelineExpanded } from './useTimelineExpanded';
import { childThreadsByMessage } from './branches';
import { buildToolPairing, messageRendersNothing } from './toolPairs';
import { persistedHasStreamedText } from './streamingHandoff';
import {
  clearBranchHighlight,
  findAllQuoteRanges,
  setBranchHighlight,
} from './branchHighlight';
import { useStickToBottom } from './useStickToBottom';
import { TranscriptComposerStack } from './TranscriptComposerStack';
import { useBottomOverlayReserve } from './useBottomOverlayReserve';
import { useTopRegionReserve } from './useTopRegionReserve';
import { TranscriptTopRegion } from './TranscriptTopRegion';
import { TranscriptMessageBlock } from './TranscriptMessageBlock';
import { TranscriptTailCards } from './TranscriptTailCards';

/**
 * Pointer travel (px, per axis) between `mousedown` and `click` under which a
 * click counts as stationary — a plain click rather than the release of a
 * drag-select. Kept small so a genuine drag (which arms a pending branch) is
 * never mistaken for a plain dismiss click, while the sub-pixel jitter a real
 * "stationary" click can carry still reads as stationary.
 */
const CLICK_DRAG_SLOP_PX = 6;

export interface TranscriptPaneProps {
  threads: Thread[];
  /** The active thread, or null for the cold-start / new-session state. */
  activeThread: Thread | null;
  /** True when the focused session is closed (read-only viewing; a Send resumes it). */
  readOnly: boolean;
  /**
   * True while the focused session is still starting (`status: 'spawning'`): it
   * is focusable from the moment its first send was accepted, but its launch
   * has not registered yet. Such a session has no live pane either, so
   * `readOnly` is true alongside this — this flag is what tells the two apart,
   * keeping the "closed, send to resume" notice off a session that was never
   * closed and telling the composer to wait instead (see {@link ComposerMode}).
   */
  spawning?: boolean;
  /** True for the new-session composer state (no session/thread exists yet). */
  newSession?: boolean;
  /**
   * True when choosing a working directory is mandatory (the first run, with no
   * sessions to fall back to). Makes the directory picker non-dismissable, so the
   * user must select a directory before they can reach the new-session screen.
   */
  workdirMandatory?: boolean;
  /**
   * The right pane's reopen button, rendered at the right end of the top region
   * (next to the collapsed timeline toggle) so the two controls share one row
   * and the timeline card can grow downward without overlapping anything else.
   *
   * Which button it is depends on the focused session's provider capabilities —
   * "Terminal" for a provider with an attachable PTY, "Comms" for a headless one
   * whose window is its frame log — and that choice is the workspace's to make
   * (see `WorkspaceScreen`); this pane only hosts the slot, which is why it is a
   * plain node rather than a flag. Optional: `null` (or absent) hides the slot
   * entirely — used while the right pane is already open, or in tests that do
   * not exercise it at all.
   */
  paneToggleButton?: ReactNode;
  /**
   * Whether the focused session's provider offers an attachable terminal
   * (`ProviderCapabilities.has_terminal`), forwarded to the permission notice so
   * its conflict fallback only points at a terminal that exists. Resolving the
   * capability is the workspace's job — it holds the focused session and the
   * providers query (see `WorkspaceScreen`) — so this pane only relays it.
   * `undefined` when the capability is not known yet, which the notice resolves
   * to its own documented default.
   */
  providerHasTerminal?: boolean;
  /**
   * Whether the focused session's provider accepts a session-scoped permission
   * decision (`ProviderCapabilities.has_allow_for_session`), forwarded to the
   * permission notice so it only offers that button where pressing it would
   * work. Relayed unresolved for the same reason as
   * {@link TranscriptPaneProps.providerHasTerminal} — `undefined` means the
   * capability is not known, and the notice owns what that resolves to (which,
   * unlike the terminal flag, is `false`).
   */
  providerHasAllowForSession?: boolean;
  /**
   * True when the focused session is open on a pane Delta re-adopted after a
   * restart whose agent can no longer deliver hooks to this server — the
   * session row's `hooks_unreachable`. Its transcript and terminal still work,
   * but prompt echoes, turn ends and permission dialogs never arrive, so the
   * pane says how to get it back: use the terminal, or Close it and send again,
   * which resumes it with fresh settings.
   */
  hooksUnreachable?: boolean;
  /**
   * Why Delta closed the focused session on its own — the session row's
   * `closed_reason` (for instance, it moved to a new port while the session's
   * agent still called the old one). Shown in the closed notice in place of
   * the plain wording; `null` or absent for every other session.
   */
  closedReason?: string | null;
}

/**
 * The right pane. For an existing session it shows the active thread's trunk as
 * a linear list (breadcrumb, branch chips, external-input marker) in the
 * scrolling body, with a pinned footer that stacks the send-activity strip
 * directly above the composer. A closed session is read-only for viewing,
 * but the composer stays available: a plain Send resumes it, and a
 * branch-from-quote both resumes it and drills into the new sub-thread. For the
 * new-session state it shows a blank prompt and a new-session composer.
 */
export function TranscriptPane({
  threads,
  activeThread,
  readOnly,
  spawning = false,
  newSession = false,
  workdirMandatory = false,
  paneToggleButton = null,
  providerHasTerminal,
  providerHasAllowForSession,
  hooksUnreachable = false,
  closedReason = null,
}: TranscriptPaneProps) {
  const client = useApiClient();
  const setActiveThread = useNavStore((state) => state.setActiveThread);
  const cancelNewSession = useNavStore((state) => state.cancelNewSession);
  const branchOrigin = useComposerStore((state) => state.branchOrigin);
  const setBranchOrigin = useComposerStore((state) => state.setBranchOrigin);
  const setNewSessionWorkdir = useComposerStore(
    (state) => state.setNewSessionWorkdir,
  );
  const resetNewSessionLaunchOptions = useComposerStore(
    (state) => state.resetNewSessionLaunchOptions,
  );
  const resetNewSessionProvider = useComposerStore(
    (state) => state.resetNewSessionProvider,
  );
  // The picker's open state lives in the store (not local component state) so
  // the navigator's "New" button can (re)open it without a focus transition.
  const workdirDialogOpen = useComposerStore(
    (state) => state.workdirDialogOpen,
  );
  const closeWorkdirDialog = useComposerStore(
    (state) => state.closeWorkdirDialog,
  );
  // Whether the focused (closed) session just failed to resume because its
  // transcript is gone; drives the inline "cannot be resumed" notice.
  const resumeUnavailable = useLiveStore((state) =>
    activeThread
      ? noticeOf(state.notices, activeThread.session_id, 'resume_unavailable') !==
        null
      : false,
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
  // The focused session's running subagents, if any.
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

  // The sub-thread chip currently hovered; its text is highlighted in the body.
  const [hoveredBranchTitle, setHoveredBranchTitle] = useState<string | null>(
    null,
  );

  // The surface the send-activity strip renders for in this view: the
  // new-session screen, or the active thread. The merged rows (server open
  // sends plus the thin client complements) drive both the strip and the count
  // used by stick-to-bottom / the empty-state gate.
  const sendActivitySurface: SendActivitySurface | null = newSession
    ? { kind: 'new-session' }
    : activeThread
      ? {
          kind: 'thread',
          sessionId: activeThread.session_id,
          threadId: activeThread.id,
        }
      : null;
  const sendActivity = useSendActivity(sendActivitySurface);
  const pendingCount = sendActivity.length;

  const messagesQuery = useThreadMessagesQuery(
    client,
    activeThread?.id ?? null,
  );
  const allMessages: Message[] = messagesQuery.data?.messages ?? [];

  // Render user and assistant turns, plus meta lines (shown collapsed) and the
  // one user-facing system row — the unknown-command notice. Every other
  // system/other row is ingest-only (content-empty or internal), so it is
  // skipped here.
  const messages = useMemo(
    () =>
      allMessages.filter(
        (m) =>
          m.role === 'user' ||
          m.role === 'assistant' ||
          m.role === 'meta' ||
          isUnknownCommandNoticeMessage(m),
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

  // When navigating UP to an ancestor via the breadcrumb, this holds the child
  // thread one level down toward where we were. After the ancestor renders, the
  // scroll effect brings that child's chip — where the branch sprouts — into
  // view instead of jumping to the bottom of a possibly long parent.
  const scrollToChildRef = useRef<ThreadId | null>(null);
  const { bodyRef, stickRef, lastContentLength } = useStickToBottom({
    activeThread,
    newSession,
    messages,
    pendingCount,
    streaming,
    scrollToChildRef,
    setHoveredBranchTitle,
  });

  // `timelineExpanded` flips the entire top-row layout — not just the
  // timeline card's own collapsed/expanded chrome:
  //   - collapsed: two independent absolute floating cards (breadcrumb
  //     top-left, {Thread + pane toggle} cluster top-right) over the
  //     scrolling body, plus a measured `padding-top` reserve so the
  //     first message clears them.
  //   - expanded: a SINGLE absolute container pinned to the top of the
  //     Panel's body region (does not scroll with the conversation),
  //     holding the expanded timeline card on top and a single
  //     normal-flow row of breadcrumb + pane toggle underneath it. The
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

  // Leaving the new-session state discards the selection and closes any
  // still-open modal, so a later return starts clean. Clearing the workdir also
  // drops its provenance (any PR pick) and the worktree choice. The selection is
  // also cleared on a successful new-session send by the composer. Keyed on
  // `newSession` only: enter is now handled by the Directory tab itself
  // (no more auto-opened modal), but leave-state cleanup still belongs here.
  useEffect(() => {
    if (!newSession) {
      setNewSessionWorkdir(null);
      resetNewSessionLaunchOptions();
      resetNewSessionProvider();
      closeWorkdirDialog();
    }
  }, [
    newSession,
    setNewSessionWorkdir,
    resetNewSessionLaunchOptions,
    resetNewSessionProvider,
    closeWorkdirDialog,
  ]);

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
    // chip's own title text (and banners, the send-activity strip, etc.) out of
    // the highlight.
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
  // Kept as the MODE rather than the element: the rail's prompt-template button
  // edits the same draft from outside the card, so the draft key has to be
  // derivable here too (see {@link composerDraftKey}).
  const composerMode: ComposerMode | undefined = newSession
    ? { kind: 'new-session' }
    : activeThread && !resumeUnavailable
      ? {
          kind: 'thread',
          activeThread,
          readOnly,
          spawning,
          // Only an open session has hooks to lose; a closed one resumes on
          // send, which is exactly the way back the notice offers.
          hooksUnreachable: hooksUnreachable && !readOnly,
        }
      : undefined;

  // The floating layer over the scrolling transcript (see Panel's `overlay`):
  // the composer stack. It sits on top of the conversation rather than in flow,
  // so its growing/shrinking never resizes the scroll viewport — the tail the
  // user is reading stays put instead of jumping. The body reserves a fixed
  // bottom padding (below) so resting content clears it.

  // The bottom layer: the composer plus the notices that must stay next to the
  // input — the closed/external-input banners and, crucially, the send-activity
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
  } else if (composerMode) {
    bottomContent = (
      <TranscriptComposerStack
        newSession={newSession}
        activeThread={activeThread}
        readOnly={readOnly}
        spawning={spawning}
        hooksUnreachable={hooksUnreachable}
        closedReason={closedReason}
        composerMode={composerMode}
        sendActivity={sendActivity}
      />
    );
  }

  // Whether a bottom overlay is mounted at all. This is the ONLY real state the
  // measure effect below derives from: the effect re-binds its ResizeObserver
  // when the overlay node appears or disappears, and the observer itself
  // handles every subsequent size change (composer auto-grow, banners). Keying
  // the effect on this boolean instead of `bottomContent`'s JSX identity (a
  // fresh object every render) stops it from re-running — and rewriting the
  // body's `scrollTop` while stick is armed — on every unrelated render.
  const hasBottomOverlay = bottomContent != null;
  const { bottomOverlayRef, bottomReserve } = useBottomOverlayReserve({
    hasBottomOverlay,
    bodyRef,
    stickRef,
  });
  const {
    breadcrumbOverlayRef,
    rightClusterOverlayRef,
    expandedContainerRef,
    topRegionReserve,
  } = useTopRegionReserve({
    timelineExpanded,
    newSession,
    activeThread,
    isOnSubThread,
    paneToggleButton,
  });

  // The bottom layer floats over the body as a stack of cards inset from the
  // left, right, and bottom edges. This wrapper is transparent and only
  // positions and MEASURES the stack (the body reserves bottom padding equal to
  // its measured height — see `useBottomOverlayReserve` — so resting content
  // clears it however tall the composer grows); its children carry the actual
  // floating-card chrome (`FLOATING_CARD_CLASS` in `floatingCard.ts`). For a
  // resume-impossible session the single "cannot resume" notice floats on its
  // own with no composer beneath it.
  const bottomOverlay = bottomContent && (
    <div
      ref={bottomOverlayRef}
      data-testid="bottom-overlay"
      className="pointer-events-auto absolute inset-x-overlay-inset bottom-overlay-inset flex flex-col gap-2"
    >
      {bottomContent}
    </div>
  );

  const topRegion = (
    <TranscriptTopRegion
      timelineExpanded={timelineExpanded}
      newSession={newSession}
      threads={threads}
      activeThread={activeThread}
      isOnSubThread={isOnSubThread}
      paneToggleButton={paneToggleButton}
      breadcrumbItems={breadcrumbItems}
      bodyRef={bodyRef}
      breadcrumbOverlayRef={breadcrumbOverlayRef}
      rightClusterOverlayRef={rightClusterOverlayRef}
      expandedContainerRef={expandedContainerRef}
    />
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
      // {Thread + pane toggle} cluster) float as independent absolute
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
        // The breadcrumb / timeline / pane-toggle are rendered in flow at
        // the top of the body via `topRegion`; on the main thread there is no
        // breadcrumb but the timeline + pane toggle still ride along.
        undefined
      }
      overlay={bottomOverlay}
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

      {renderedMessages.map((message) => (
        <TranscriptMessageBlock
          key={message.uuid}
          message={message}
          childMap={childMap}
          pairing={pairing}
          latestAssistantUuid={latestAssistantUuid}
          handleSelectQuote={handleSelectQuote}
          flashChildId={flashChildId}
          setHoveredBranchTitle={setHoveredBranchTitle}
          setActiveThread={setActiveThread}
        />
      ))}

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

      <TranscriptTailCards
        activeThread={activeThread}
        readOnly={readOnly}
        spawning={spawning}
        providerHasTerminal={providerHasTerminal}
        providerHasAllowForSession={providerHasAllowForSession}
      />
    </Panel>
  );
}
