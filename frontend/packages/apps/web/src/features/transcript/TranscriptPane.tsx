import { useLayoutEffect, useMemo, useRef } from 'react';
import {
  threadAncestry,
  type Message,
  type Thread,
  type ThreadId,
} from '@delta/model';
import { useThreadMessagesQuery } from '@delta/api-client';
import { Badge, Breadcrumb, Chip, Panel } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import {
  NEW_SESSION_DRAFT_KEY,
  useComposerStore,
} from '../../store/composerStore';
import { useLiveStore } from '../../store/liveStore';
import { Composer } from '../composer/Composer';
import { PendingQueue } from '../composer/PendingQueue';
import { MessageItem } from './MessageItem';
import { childThreadsByMessage } from './branches';

/**
 * Distance from the bottom (in px) under which the transcript is considered
 * "at the bottom" and keeps following new content.
 */
const STICK_THRESHOLD_PX = 64;

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
  const setBranchOrigin = useComposerStore((state) => state.setBranchOrigin);
  const externalInput = useLiveStore((state) => state.externalInput);

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
  // focused thread.
  useLayoutEffect(() => {
    stickRef.current = true;
    const el = bodyRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [activeThread?.id, newSession]);

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
  const composer = newSession ? (
    <Composer mode={{ kind: 'new-session' }} />
  ) : activeThread ? (
    <Composer
      mode={{
        kind: 'thread',
        activeThread,
        readOnly,
      }}
    />
  ) : undefined;

  // The optimistic pending-send strip is pinned just above the composer (in the
  // fixed footer, not the scrolling transcript) so it never jostles the
  // conversation tail while a turn is in flight.
  const footer = composer ? (
    <div className="space-y-2">
      <PendingQueue threadId={pendingThreadId} />
      {composer}
    </div>
  ) : undefined;

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
      {readOnly && !newSession && (
        <div
          className="flex items-center gap-2 border-b border-slate-100 bg-slate-50 px-3 py-2 text-xs text-slate-500"
          data-testid="readonly-notice"
        >
          <Badge tone="neutral">closed</Badge>
          <span>
            This session is closed. Sending a message resumes it.
          </span>
        </div>
      )}

      {newSession && (
        <p
          className="px-3 py-4 text-sm text-slate-400"
          data-testid="new-session-empty"
        >
          Send the first message below to start a new session.
        </p>
      )}

      {showExternalInput && (
        <div className="flex items-start gap-2 border-b border-slate-100 bg-sky-50 px-3 py-2 text-xs">
          <Badge tone="info">external input</Badge>
          <span className="text-slate-700">{externalInput.prompt}</span>
        </div>
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

      {messages.map((message) => {
        const children = childMap.get(message.uuid) ?? [];
        return (
          <div key={message.uuid}>
            <MessageItem
              message={message}
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
              <div className="flex flex-wrap gap-1.5 px-3 pb-2">
                {children.map((child) => (
                  <Chip key={child.id} onClick={() => setActiveThread(child.id)}>
                    ⤷ {child.title}
                    <span className="font-medium">[enter →]</span>
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
