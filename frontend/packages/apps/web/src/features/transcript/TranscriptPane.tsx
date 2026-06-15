import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { threadAncestry, type ThreadId } from '@delta/model';
import type { Message, Thread } from '@delta/wire-gen';
import { useThreadMessagesQuery } from '@delta/api-client';
import { Badge, Breadcrumb, Button, Chip, Panel } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import { useComposerStore } from '../../store/composerStore';
import { noticeOf, useLiveStore } from '../../store/liveStore';
import { Composer } from '../composer/Composer';
import { PendingQueue } from '../composer/PendingQueue';
import {
  usePendingSends,
  type PendingSurface,
} from '../composer/usePendingSends';
import { WorkdirChip, WorkdirDialog } from '../composer/WorkdirDialog';
import { WorktreeOptions } from '../composer/WorktreeOptions';
import { LaunchOptionsPicker } from '../composer/LaunchOptionsPicker';
import { AssistantMarkdown } from './AssistantMarkdown';
import { isTaskNotificationMessage } from './claudeFormat';
import { MessageItem } from './MessageItem';
import { PermissionNoticeCard } from './PermissionNotice';
import { QuestionCard } from './QuestionCard';
import { SubagentRunningIndicator } from './SubagentRunningIndicator';
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
  const setNewSessionLaunchOptionIds = useComposerStore(
    (state) => state.setNewSessionLaunchOptionIds,
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
  const subagents = useLiveStore((state) =>
    activeThread
      ? state.runningSubagents[activeThread.session_id] ?? null
      : null,
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

  // Stick-to-bottom: auto-scroll the transcript when new content arrives, but
  // only while the user is already near the bottom (so reading scrollback is
  // never yanked away). The scroll region is the Panel body.
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const stickRef = useRef(true);
  const prevPendingRef = useRef(pendingCount);

  // The bottom overlay (composer + pending strip + bottom notices) floats over
  // the scrolling body and grows with the composer's content. Reserve bottom
  // padding equal to its MEASURED height (plus the overlay inset as a gap) so
  // the last turn always rests just above it and stays readable as it grows —
  // replacing the old fixed `pb-composer-reserve`, which a grown composer would
  // cover. `null` until measured: the body falls back to the fixed reserve so a
  // first paint (or a body without an overlay) never under-reserves.
  const bottomOverlayRef = useRef<HTMLDivElement | null>(null);
  const [bottomReserve, setBottomReserve] = useState<number | null>(null);

  // When navigating UP to an ancestor via the breadcrumb, this holds the child
  // thread one level down toward where we were. After the ancestor renders, the
  // scroll effect brings that child's chip — where the branch sprouts — into
  // view instead of jumping to the bottom of a possibly long parent.
  const scrollToChildRef = useRef<ThreadId | null>(null);
  // The child chip to briefly flash after such a scroll, so the eye catches it.
  const [flashChildId, setFlashChildId] = useState<ThreadId | null>(null);

  // A plain click anywhere in the transcript body drops a pending branch
  // selection (the "Branch from selected text" affordance), so dismissing it no
  // longer requires hunting for the composer's ✕. The gate is strict: only a
  // click that leaves the selection COLLAPSED clears. The mouseup that finishes
  // a drag-select also fires a click, but it leaves a non-empty selection (which
  // is what just set/updated the branch origin), so it must not immediately undo
  // it. Attached via the body ref (like the scroll listener) since the shared
  // Panel body does not take an onClick. `branchOrigin` is read live from the
  // store so the listener does not need re-binding as it changes.
  useEffect(() => {
    const el = bodyRef.current;
    if (!el) {
      return;
    }
    const onClick = () => {
      if (!window.getSelection()?.isCollapsed) {
        return;
      }
      if (useComposerStore.getState().branchOrigin !== null) {
        setBranchOrigin(null);
        clearBranchHighlight();
      }
    };
    el.addEventListener('click', onClick);
    return () => el.removeEventListener('click', onClick);
  }, [setBranchOrigin]);

  // Recompute "is the user near the bottom?" on every scroll so the
  // stick-to-bottom effects know whether to follow new content.
  useLayoutEffect(() => {
    const el = bodyRef.current;
    if (!el) {
      return;
    }
    const onScroll = () => {
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
    stickRef.current = true;
    const el = bodyRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [activeThread?.id, newSession]);

  // Entering the new-session state auto-opens the working-directory modal (when
  // nothing is selected yet), since a directory is mandatory and the user should
  // be able to confirm the most-recent one immediately. Leaving the state
  // discards the selection and closes the modal, so a later return starts clean.
  // The selection is also cleared on a successful new-session send by the
  // composer. Keyed on `newSession` only: it must fire on the enter/leave
  // transition, not every time the selection changes (which would re-open the
  // modal the user just dismissed).
  useEffect(() => {
    if (newSession) {
      if (!useComposerStore.getState().newSessionWorkdir) {
        openWorkdirDialog();
      }
    } else {
      setNewSessionWorkdir(null);
      setNewSessionLaunchOptionIds([]);
      closeWorkdirDialog();
    }
  }, [
    newSession,
    setNewSessionWorkdir,
    setNewSessionLaunchOptionIds,
    openWorkdirDialog,
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
        className="flex items-center gap-2 rounded border border-rose-200 bg-rose-50 px-2 py-1 text-xs text-rose-700"
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
    bottomContent = (
      <div className="space-y-2">
        {/* The question card is NOT in this bottom stack: it renders inline at
            the conversation tail in the scrolling body (see questionCard above
            and its placement after the streaming bubble), so the choices follow
            the streamed preamble in the flow instead of floating over it. */}
        {readOnly && !newSession && (
          <div
            className="flex items-center gap-2 rounded border border-slate-200 bg-slate-50 px-2 py-1 text-xs text-slate-500"
            data-testid="readonly-notice"
          >
            <Badge tone="neutral">closed</Badge>
            <span>This session is closed. Sending a message resumes it.</span>
          </div>
        )}

        {showExternalInput && activeThread && (
          <div
            className="flex items-start gap-2 rounded border border-sky-200 bg-sky-50 px-2 py-1 text-xs"
            data-testid="external-input-notice"
          >
            <Badge className="shrink-0" tone="info">
              external input
            </Badge>
            <span className="min-w-0 flex-1 line-clamp-2 break-words text-slate-700">
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

        {/* A directory is chosen: show it as a chip with a ✎ to change it (the
            ✎ reopens the picker without resetting the selection). The chip
            renders nothing when no directory is selected, so there is no button
            to (re)open the picker from here — that is done via "New". */}
        {newSession && <WorkdirChip onEdit={openWorkdirDialog} />}
        {/* Below the directory chip: when the selected directory is a git repo,
            an opt-in to start the session in a fresh worktree (with a
            start-point choice). Renders nothing for a non-git directory. */}
        {newSession && <WorktreeOptions />}
        {newSession && <LaunchOptionsPicker />}
        <PendingQueue entries={pendingEntries} />
        {composer}
      </div>
    );
  }

  // A floating card near the bottom of the body: inset from the left, right, and
  // bottom edges with a full border, rounded corners, and a shadow so it reads as
  // lifted above (rather than fused to) the transcript. Its opaque background
  // still occludes the transcript scrolling beneath it. The body reserves bottom
  // padding equal to this card's MEASURED height (see the effect below) so
  // resting content clears it however tall the composer grows.
  const bottomOverlay = bottomContent && (
    <div
      ref={bottomOverlayRef}
      data-testid="bottom-overlay"
      className="pointer-events-auto absolute inset-x-overlay-inset bottom-overlay-inset rounded-md border border-slate-300 bg-white px-3 py-2 shadow-md"
    >
      {bottomContent}
    </div>
  );

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
  }, [bottomContent]);

  // The breadcrumb gets the same floating-card treatment as the composer, pinned
  // at the top-left and hugging its own width (rather than a full-width header
  // bar). It floats over the transcript; the body reserves a fixed top padding
  // (below) so the first turn is not hidden behind it at rest.
  const breadcrumbOverlay = isOnSubThread && (
    <div className="pointer-events-auto absolute left-overlay-inset top-overlay-inset max-w-[calc(100%-2*var(--delta-overlay-inset))] rounded-md border border-slate-300 bg-white px-3 py-1.5 shadow-md">
      <Breadcrumb items={breadcrumbItems} />
    </div>
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
      // under-reserves on first paint. The top breadcrumb reserve stays a fixed
      // class: the breadcrumb card does not grow, so a measured value would buy
      // nothing.
      bodyClassName={isOnSubThread ? 'pt-breadcrumb-reserve' : undefined}
      bodyStyle={{
        paddingBottom:
          bottomReserve !== null
            ? `${bottomReserve}px`
            : 'var(--delta-composer-body-reserve)',
      }}
      header={
        newSession ? (
          <span className="text-sm font-semibold text-slate-700">
            New session
          </span>
        ) : // `undefined` (not `null`) so Panel drops the header bar entirely. The
        // breadcrumb is rendered as a floating card via `overlay` instead, and on
        // the main thread there is nothing to show — no empty strip above.
        undefined
      }
      overlay={
        <>
          {breadcrumbOverlay}
          {permissionOverlay}
          {bottomOverlay}
        </>
      }
    >
      {newSession && (
        <>
          <p
            className="px-3 py-4 text-sm text-slate-400"
            data-testid="new-session-empty"
          >
            Send the first message below to start a new session.
          </p>
          {/* Modal directory picker (portals to the document body). Auto-opens
              on entering the new-session state; commits the chosen cwd to the
              composer store on Select. */}
          <WorkdirDialog
            open={workdirDialogOpen}
            dismissable={!workdirMandatory}
            onClose={() => {
              closeWorkdirDialog();
              // Dismissing the picker without a directory cancels the
              // new-session intent and returns to the previously-focused
              // session — but only when there is one to return to. With no
              // sessions, new-session is the mandatory default, so
              // cancelNewSession() is a no-op and we stay. Read the live store
              // value to avoid a stale closure (mirrors the auto-open effect
              // above).
              if (!useComposerStore.getState().newSessionWorkdir) {
                cancelNewSession();
              }
            }}
          />
        </>
      )}

      {!newSession && messagesQuery.isLoading && (
        <p className="px-3 py-4 text-sm text-slate-400">Loading transcript…</p>
      )}

      {!newSession &&
        !messagesQuery.isLoading &&
        messages.length === 0 &&
        pendingCount === 0 && (
          <p className="px-3 py-4 text-sm text-slate-400">
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
              // Branch-from-quote works on closed sessions too: the branch send
              // resumes the session before creating the child thread, so an old
              // conversation can be picked up from a selected passage.
              onSelectQuote={(msg, quote) =>
                setBranchOrigin({
                  parentThreadId: activeThread!.id,
                  semanticParentUuid: msg.uuid,
                  locatorQuote: quote,
                })
              }
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
                          ? 'ring-2 ring-indigo-400 ring-offset-1'
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
            className="px-3 text-sm"
            data-role="assistant"
            data-testid="streaming-message"
          >
            <div className="rounded-lg bg-slate-50 px-3 py-2 text-slate-800">
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
                    className="ml-0.5 inline-block animate-caret-blink text-slate-600"
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
