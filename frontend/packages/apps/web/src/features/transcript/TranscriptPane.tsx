import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import {
  threadAncestry,
  type Message,
  type Thread,
  type ThreadId,
} from '@delta/model';
import { useThreadMessagesQuery } from '@delta/api-client';
import { Badge, Breadcrumb, Button, Chip, Panel } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import {
  NEW_SESSION_DRAFT_KEY,
  useComposerStore,
} from '../../store/composerStore';
import { useLiveStore } from '../../store/liveStore';
import { Composer } from '../composer/Composer';
import { PendingQueue } from '../composer/PendingQueue';
import { WorkdirChip, WorkdirPicker } from '../composer/WorkdirPicker';
import { MessageItem } from './MessageItem';
import { childThreadsByMessage } from './branches';
import { buildToolPairing, messageRendersNothing } from './toolPairs';
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
 * How long a permission notice must stay pending before it is shown.
 *
 * The `PreToolUse` hook fires for every tool call, including auto-approved ones,
 * so a notice is briefly set then cleared (via `permission_resolved`) the moment
 * the correlated `tool_result` lands. Delaying the render by this window hides
 * that flash: an auto-approved tool resolves well within it and never paints,
 * while a genuine TUI prompt — which has no resolution until the human answers —
 * outlasts the window and renders as normal.
 */
const PERMISSION_NOTICE_DELAY_MS = 300;

/**
 * Defer surfacing a permission notice until it has stayed present for
 * {@link PERMISSION_NOTICE_DELAY_MS}. A notice that clears within the window
 * never renders; a notice that persists renders once the window elapses.
 * Returns `null` until then, and immediately when the source notice is gone.
 */
function useDebouncedPermission<T>(notice: T | null): T | null {
  const [visible, setVisible] = useState<T | null>(null);

  useEffect(() => {
    if (notice === null) {
      // Cleared (resolved/dismissed/turn done): drop it at once, no delay.
      setVisible(null);
      return;
    }
    const timer = setTimeout(
      () => setVisible(notice),
      PERMISSION_NOTICE_DELAY_MS,
    );
    return () => clearTimeout(timer);
  }, [notice]);

  return visible;
}

export interface TranscriptPaneProps {
  threads: Thread[];
  /** The active thread, or null for the cold-start / new-session state. */
  activeThread: Thread | null;
  /** True when the focused session is closed (read-only viewing; a Send resumes it). */
  readOnly: boolean;
  /** True for the new-session composer state (no session/thread exists yet). */
  newSession?: boolean;
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
}: TranscriptPaneProps) {
  const client = useApiClient();
  const setActiveThread = useNavStore((state) => state.setActiveThread);
  const setTerminalOpen = useNavStore((state) => state.setTerminalOpen);
  const setBranchOrigin = useComposerStore((state) => state.setBranchOrigin);
  const setNewSessionWorkdir = useComposerStore(
    (state) => state.setNewSessionWorkdir,
  );
  // The focused session's external-input marker, if any. Keyed per session like
  // the permission notice; visibility is further gated to the active thread below.
  const externalInput = useLiveStore((state) =>
    activeThread ? state.externalInput[activeThread.session_id] ?? null : null,
  );
  // Whether the focused (closed) session just failed to resume because its
  // transcript is gone; drives the inline "cannot be resumed" notice.
  const resumeUnavailable = useLiveStore((state) =>
    activeThread ? Boolean(state.resumeUnavailable[activeThread.session_id]) : false,
  );
  // The focused session's pending permission prompt, if any. A tool's PreToolUse
  // hook blocks that session until it is answered in the terminal.
  const permission = useLiveStore((state) =>
    activeThread ? state.permission[activeThread.session_id] ?? null : null,
  );
  // Defer showing the notice so an auto-approved tool's brief set→resolve never
  // paints; a genuine pending prompt outlasts the window and shows as normal.
  const visiblePermission = useDebouncedPermission(permission);
  const dismissPermission = useLiveStore((state) => state.dismissPermission);
  const dismissExternalInput = useLiveStore(
    (state) => state.dismissExternalInput,
  );

  // The sub-thread chip currently hovered; its text is highlighted in the body.
  const [hoveredBranchTitle, setHoveredBranchTitle] = useState<string | null>(
    null,
  );

  // The key the pending queue renders under for this view.
  const pendingThreadId: ThreadId | null = newSession
    ? NEW_SESSION_DRAFT_KEY
    : activeThread?.id ?? null;
  const pendingCount = useLiveStore((state) =>
    pendingThreadId === null
      ? 0
      : state.pending.filter((item) => item.threadId === pendingThreadId)
          .length,
  );

  const messagesQuery = useThreadMessagesQuery(
    client,
    activeThread?.id ?? null,
  );
  const allMessages: Message[] = messagesQuery.data?.messages ?? [];

  // Render only user and assistant turns; system/other rows are ingest-only.
  const messages = useMemo(
    () =>
      allMessages.filter((m) => m.role === 'user' || m.role === 'assistant'),
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
    stickRef.current = true;
    setHoveredBranchTitle(null);
    const el = bodyRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [activeThread?.id, newSession]);

  // Leaving the new-session state discards the picker's selection, so a later
  // return to "new session" starts from the default (no `workdir`) again. The
  // selection is also cleared on a successful new-session send by the composer.
  useEffect(() => {
    if (!newSession) {
      setNewSessionWorkdir(null);
    }
  }, [newSession, setNewSessionWorkdir]);

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
  }, [messages.length, lastContentLength, pendingCount]);

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

  // While a sub-thread chip is hovered, mark every occurrence of its text in
  // the body so it is clear at a glance what that branch was about. Re-run when
  // content changes (rendered text nodes are recreated) so the marks track
  // streaming and refetches; clear on leave or unmount.
  useEffect(() => {
    const body = bodyRef.current;
    if (!body || !hoveredBranchTitle) {
      clearBranchHighlight();
      return;
    }
    // Search only message bodies, not the surrounding UI: the chips render as
    // siblings of the message article, so scoping to the articles keeps a
    // chip's own title text (and banners, the pending queue, etc.) out of the
    // highlight.
    const articles = body.querySelectorAll('[data-testid="message-item"]');
    const ranges = Array.from(articles).flatMap((article) =>
      findAllQuoteRanges(article, hoveredBranchTitle),
    );
    setBranchHighlight(ranges);
    return () => clearBranchHighlight();
  }, [hoveredBranchTitle, messages.length, lastContentLength]);

  const breadcrumbItems = ancestry.map((thread) => ({
    key: thread.id,
    label: thread.title,
    onClick: () => setActiveThread(thread.id),
  }));

  // Until the session has branched, the breadcrumb is a lone "main" that reads
  // as abrupt noise — there is no tree to place it in. Show it only once a
  // sub-thread exists (a sub-thread is any thread with a parent), matching the
  // navigator, which hides the standalone main node under the same condition.
  const hasSubThreads = threads.some((t) => t.parent_thread_id !== null);

  const showExternalInput =
    !newSession &&
    activeThread !== null &&
    externalInput !== null &&
    externalInput.threadId === activeThread.id;

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

  // The fixed footer (pinned below the scrolling transcript). For a
  // resume-impossible session it is just the "cannot resume" notice, replacing
  // the input entirely — there is nothing useful to type. Otherwise it stacks
  // the session-state notices (permission, closed, external input), the
  // optimistic pending-send strip, and the composer. The notices are pinned
  // directly above the input rather than at the top of the scrolling body, where
  // a long conversation scrolled to its tail would bury them out of sight.
  let footer: ReactNode;
  if (resumeUnavailable && !newSession) {
    footer = (
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
    footer = (
      <div className="space-y-2">
        {visiblePermission && activeThread && (
          <div
            className="space-y-1 rounded border border-amber-200 bg-amber-50 px-2 py-1 text-xs"
            data-testid="permission-notice"
            role="alert"
          >
            <p className="font-medium text-amber-800">
              Permission requested: {visiblePermission.toolName}
            </p>
            <p className="text-slate-600">Answer the prompt in the terminal.</p>
            <div className="flex gap-2">
              <Button size="sm" onClick={() => setTerminalOpen(true)}>
                Open terminal
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => dismissPermission(activeThread.session_id)}
              >
                Dismiss
              </Button>
            </div>
          </div>
        )}

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
            className="space-y-1 rounded border border-sky-200 bg-sky-50 px-2 py-1 text-xs"
            data-testid="external-input-notice"
          >
            <div className="flex items-start gap-2">
              <Badge tone="info">external input</Badge>
              <span className="line-clamp-2 text-slate-700">
                {externalInput.prompt}
              </span>
            </div>
            <div className="flex justify-end">
              <Button
                size="sm"
                variant="ghost"
                onClick={() => dismissExternalInput(activeThread.session_id)}
              >
                Dismiss
              </Button>
            </div>
          </div>
        )}

        {newSession && <WorkdirChip />}
        <PendingQueue threadId={pendingThreadId} />
        {composer}
      </div>
    );
  }

  return (
    <Panel
      bodyRef={bodyRef}
      header={
        newSession ? (
          <span className="text-sm font-semibold text-slate-700">
            New session
          </span>
        ) : hasSubThreads ? (
          <Breadcrumb items={breadcrumbItems} />
        ) : null
      }
      footer={footer}
    >
      {newSession && (
        <>
          <p
            className="px-3 py-4 text-sm text-slate-400"
            data-testid="new-session-empty"
          >
            Send the first message below to start a new session.
          </p>
          <WorkdirPicker />
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
        // orphans Claude delivers as `role: user`). These are tightened and
        // left-indented so they read as nested steps, distinct from prose. The
        // check comes before the user/prose split so an orphan tool_result is
        // treated as a tool card, not a user turn.
        const isToolTurn = message.content.some(
          (block) => block.type === 'tool_use' || block.type === 'tool_result',
        );
        const topGap = isToolTurn
          ? 'pt-0.5'
          : message.role === 'user'
            ? 'pt-2'
            : 'pt-1.5';
        const bottomGap = isToolTurn
          ? 'pb-0.5'
          : message.role === 'user'
            ? 'pb-1'
            : 'pb-2';
        // Inset a tool message (left margin) so it reads as a nested step.
        const indent = isToolTurn ? 'ml-6 mr-0' : '';
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
                  <Chip
                    key={child.id}
                    // The chip's clickable pill shape conveys that it enters the
                    // branch, so no "[enter →]" label is shown. The accessible
                    // name still says "Enter <title>" for screen readers (and to
                    // distinguish it from the navigator tree node of the same
                    // branch).
                    ariaLabel={`Enter ${child.title}`}
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
                ))}
              </div>
            )}
          </div>
        );
      })}
    </Panel>
  );
}
