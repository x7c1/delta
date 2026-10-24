import { useLayoutEffect, useRef, type RefObject } from 'react';
import type { ThreadId } from '@delta/model';
import type { Message, Thread } from '@delta/wire-gen';
import { useNavStore } from '../../store/navStore';
import type { StreamingMessage } from '../../store/liveStore';

/**
 * Distance from the bottom (in px) under which the transcript is considered
 * "at the bottom" and keeps following new content.
 */
const STICK_THRESHOLD_PX = 64;

interface StickToBottomArgs {
  /** The active thread, or null for the cold-start / new-session state. */
  activeThread: Thread | null;
  /** True for the new-session composer state. */
  newSession: boolean;
  /** The thread's rendered-kind messages; their growth re-sticks. */
  messages: Message[];
  /** The send-activity row count; an increase forces stick. */
  pendingCount: number;
  /** The live assistant preview; its growth re-sticks. */
  streaming: StreamingMessage | null;
  /** The child chip a breadcrumb "go up" wants to land on, if any. */
  scrollToChildRef: RefObject<ThreadId | null>;
  /** Drop the hovered sub-thread chip's highlight. */
  setHoveredBranchTitle: (title: string | null) => void;
}

/**
 * Stick-to-bottom for the transcript body: follow new content while the user
 * is near the bottom, land on the latest message of a newly focused thread,
 * and stay put while a timeline jump lands. Returns the body ref the scroll
 * region binds, the stick flag, and the last message's content length (a
 * content-change signal other effects key on).
 */
export function useStickToBottom({
  activeThread,
  newSession,
  messages,
  pendingCount,
  streaming,
  scrollToChildRef,
  setHoveredBranchTitle,
}: StickToBottomArgs) {
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
      // glue the pane to the tail and then live content would push the
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

  return { bodyRef, stickRef, lastContentLength };
}
