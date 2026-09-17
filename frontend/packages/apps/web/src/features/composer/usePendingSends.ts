import { useEffect, useMemo } from 'react';
import type { SessionId, ThreadId } from '@delta/model';
import type { Send } from '@delta/wire-gen';
import { useSessionSendsQuery } from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import {
  useLiveStore,
  type LocalSend,
  type SendingItem,
} from '../../store/liveStore';

/**
 * The surface a pending strip renders for: an existing thread, or the
 * new-session composer screen (which shows only its own submits: the POST
 * still in flight, or the launch the server rejected). `null` renders
 * nothing.
 */
export type PendingSurface =
  | { kind: 'thread'; sessionId: SessionId; threadId: ThreadId }
  | { kind: 'new-session' };

/** One row of the pending strip, tagged by where its truth lives. */
export type PendingEntry =
  /** A send the server holds open: `queued` or `dispatched`. */
  | { kind: 'server'; key: string; send: Send }
  /**
   * A server-accepted send that already left the open list (it matched its
   * transcript line) but whose turn has not ended yet — still in progress.
   * Only ever a thread surface's row: the new-session screen lists no send
   * that has already been accepted.
   */
  | { kind: 'local'; key: string; send: LocalSend }
  /** A submit whose POST is still in flight, or was rejected (`failed`). */
  | { kind: 'sending'; key: string; item: SendingItem };

/**
 * Merge the pending strip's sources for one surface, in submit order:
 * server-accepted sends (open list ∪ tracked local sends, id-ordered,
 * de-duplicated by send id — the server row wins while it exists), then
 * in-flight/failed submits.
 *
 * Shared by `PendingQueue` (the rows) and `TranscriptPane` (the count that
 * drives stick-to-bottom and the empty-state gate), so the two can never
 * disagree about what is pending.
 */
export function usePendingSends(surface: PendingSurface | null): PendingEntry[] {
  const client = useApiClient();
  const sending = useLiveStore((state) => state.sending);
  const localSends = useLiveStore((state) => state.localSends);

  const sessionId = surface?.kind === 'thread' ? surface.sessionId : null;
  const sendsQuery = useSessionSendsQuery(client, sessionId);
  const serverSends = sendsQuery.data?.sends;
  const serverTurn = sendsQuery.data?.turn;
  const serverPermission = sendsQuery.data?.permission;
  const serverPermissionCount = sendsQuery.data?.permission_count;
  const serverQuestion = sendsQuery.data?.question;
  const serverRunningSubagents = sendsQuery.data?.running_subagents;

  // Re-seed the session's active-turn flag and permission notice from the
  // response's queryable live state. After a live-stream reconnect the
  // event-reconstructed flag and notice were dropped (the `turn_started` /
  // `permission_requested` that set them may have been missed), and the
  // resync refetches this query — so both heal here without any event.
  //
  // The active-turn seed is authoritative ONLY on a settled fetch, and a
  // set-only no-op while a refetch is in flight (`fetchStatus !== 'idle'`).
  // Re-focusing a session serves its stale cached sends envelope first — still
  // `turn: in_flight` from when it was running — before the refetch lands the
  // fresh `turn: idle`. Reconciling off that stale read would resurrect a flag
  // the off-focus `turn_completed` already cleared, leaving the running spinner
  // stuck on. Gating on a settled fetch makes the stale read a set-only no-op
  // (healing intact) and lets only the fresh `idle` authoritatively clear it.
  // A fresh `in_flight` (a genuinely still-running turn after a reconnect) is
  // authoritative too, so it re-sets the dropped flag. The permission /
  // question seeds stay plain set-only (they cannot be resurrected this way).
  //
  // `dataUpdatedAt` is a dependency on purpose: when the live state did not
  // change across the reconnect gap (it was `in_flight` before and still is),
  // structural sharing hands back the identical objects, so `serverTurn` /
  // `serverPermission` alone would never re-trigger this effect — and the
  // state the reset just dropped would stay lost. The timestamp marks every
  // fresh observation, identical payload or not.
  const sendsUpdatedAt = sendsQuery.dataUpdatedAt;
  const sendsSettled = sendsQuery.fetchStatus === 'idle';
  useEffect(() => {
    if (sessionId !== null && serverTurn !== undefined) {
      useLiveStore.getState().seedActiveTurn(sessionId, serverTurn, sendsSettled);
    }
  }, [sessionId, serverTurn, sendsUpdatedAt, sendsSettled]);
  useEffect(() => {
    if (sessionId !== null && serverPermission !== undefined) {
      // The envelope reports the queue head plus its depth, so the re-seeded
      // notice carries both the dialog and how many answers are still owed.
      useLiveStore
        .getState()
        .seedPermission(sessionId, serverPermission, serverPermissionCount ?? 0);
    }
  }, [sessionId, serverPermission, serverPermissionCount, sendsUpdatedAt]);
  useEffect(() => {
    if (sessionId !== null && serverQuestion !== undefined) {
      useLiveStore.getState().seedQuestion(sessionId, serverQuestion);
    }
  }, [sessionId, serverQuestion, sendsUpdatedAt]);
  useEffect(() => {
    if (sessionId !== null && serverRunningSubagents !== undefined) {
      useLiveStore
        .getState()
        .seedRunningSubagents(sessionId, serverRunningSubagents);
    }
  }, [sessionId, serverRunningSubagents, sendsUpdatedAt]);

  return useMemo(() => {
    if (surface === null) {
      return [];
    }

    if (surface.kind === 'thread') {
      const entries: PendingEntry[] = [];
      const onThread = (serverSends ?? []).filter(
        (send) => send.thread_id === surface.threadId,
      );
      const serverIds = new Set(onThread.map((send) => send.id));
      const accepted: { id: number; entry: PendingEntry }[] = onThread.map(
        (send) => ({
          id: send.id,
          entry: { kind: 'server', key: `server-${send.id}`, send },
        }),
      );
      for (const send of Object.values(localSends)) {
        if (
          send.sessionId === surface.sessionId &&
          send.threadId === surface.threadId &&
          !serverIds.has(send.sendId)
        ) {
          accepted.push({
            id: send.sendId,
            entry: { kind: 'local', key: `local-${send.sendId}`, send },
          });
        }
      }
      accepted.sort((a, b) => a.id - b.id);
      entries.push(...accepted.map(({ entry }) => entry));
      for (const item of sending) {
        if (
          item.target.kind === 'thread' &&
          item.target.threadId === surface.threadId
        ) {
          entries.push({ kind: 'sending', key: item.id, item });
        }
      }
      return entries;
    }

    // The new-session screen: only its own submits — the sends this very
    // screen is in the middle of making, still awaiting their POST or
    // rejected by it (that row keeps the text alongside Retry / Dismiss, so
    // the launch stays recoverable on the screen it was started from).
    // Sessions the server already accepted are deliberately absent, whichever
    // way their launch then goes.
    //
    // A launch still STARTING has its own row in the navigator and its own
    // screen, and that screen is where its first prompt is shown — whether
    // `useSubmitSend` handed focus over to it the moment the send was accepted
    // or the user had already moved elsewhere by then and stayed there.
    // Repeating that text here would say the same thing twice, on the screen
    // where the user is about to write their next prompt — and it would read
    // as if the new one had already gone out.
    //
    // A launch that was accepted and then FAILED is absent for the same
    // reason: its session row is kept and marked failed, so its prompt, its
    // reason and its Retry / Remove actions are on that session's own screen.
    const entries: PendingEntry[] = [];
    for (const item of sending) {
      if (item.target.kind === 'new-session') {
        entries.push({ kind: 'sending', key: item.id, item });
      }
    }
    return entries;
  }, [surface, serverSends, localSends, sending]);
}
