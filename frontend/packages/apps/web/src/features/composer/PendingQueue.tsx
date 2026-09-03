import type { ReactNode } from 'react';
import { Badge, Button, cn, Spinner } from '@delta/ui-kit';
import {
  ApiError,
  useCancelSendMutation,
  useReleaseSendMutation,
} from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { useLiveStore } from '../../store/liveStore';
import { useNotificationStore } from '../../store/notificationStore';
import { useNewSessionSend } from './useNewSessionSend';
import type { PendingEntry } from './usePendingSends';

export interface PendingQueueProps {
  /** The merged pending rows for the active surface (see `usePendingSends`). */
  entries: PendingEntry[];
  /**
   * True while the surface's session is still starting (`status: 'spawning'`),
   * relayed by the transcript pane which already resolves it. It only changes
   * what a `queued` row says: every send accepted during the launch window sits
   * `queued` until the launch binds, whichever provider it starts. Defaults to
   * `false` — a surface that never starts a session.
   */
  sessionSpawning?: boolean;
}

/**
 * The failed-spawn row's account of the messages that did not stay on it: a
 * launch that failed with sends queued behind its first prompt hands those back
 * to the new-session composer (see `SpawnItem.restoredCount`), which is a
 * different screen from the one the user may be on and holds no trace of where
 * they came from. Retry re-sends the row's own prompt and nothing else, so the
 * line says both halves. `undefined` when there is nothing to account for.
 */
function restoredNote(restoredCount: number | undefined): string | undefined {
  if (restoredCount === undefined || restoredCount === 0) {
    return undefined;
  }
  return restoredCount === 1
    ? '1 later message was returned to the composer. Retry re-sends only this one.'
    : `${restoredCount} later messages were returned to the composer. Retry re-sends only this one.`;
}

/**
 * The pending-send strip above the composer, a view over the server's
 * open-send list plus the thin client-side complements (see `usePendingSends`):
 *
 * - a `queued` send is parked server-side and dispatches when the session goes
 *   idle — labelled so it reads as deliberate waiting, not a failure (queued
 *   sends used to look stuck and provoked duplicate resubmits). While the
 *   session is still starting the label says so instead: nothing is busy, the
 *   session is simply not up yet (see `sessionSpawning`);
 * - a `dispatched` send has reached the agent and is waiting on the reply (an
 *   "awaiting reply" spinner); an in-flight submit (the POST itself) shows a
 *   "sending" spinner;
 * - a send whose turn is running keeps an in-progress spinner until the
 *   turn-end event lands;
 * - a rejected submit or a spawn that never bound renders a distinct row with
 *   Dismiss (and Retry for a new-session launch) so it is recoverable. A
 *   launch the user cancelled ends up there too, worded and toned as the
 *   outcome it is rather than as a failure (see `outcomeRow`).
 *
 * Both `queued` and `dispatched` rows carry a Cancel control: a queued send
 * is dropped before it ever touches the pane, and a dispatched send whose
 * echo has not arrived (the user pressed Escape in the TUI to discard the
 * composer buffer, leaving the row stuck `dispatched` indefinitely) is
 * cancelled by the server injecting `Escape` into the pane on the user's
 * behalf. On a `409` (the dispatched send already echoed, or the queued one
 * already dispatched into an in-flight turn) the same refetch reconciles
 * the strip.
 *
 * A queued row with a non-null `held_at` is a *held* send: the server
 * will not dispatch it on its own, so the user decides. Two paths produce
 * one, and the row looks the same either way — it was composed before a
 * server restart (possibly long ago) and recovered at boot, or its
 * keystrokes vanished twice running and the echo deadline parked it (the
 * `send_parked` notice explains that case). Silently re-sending either was
 * rejected in review: stale text can land in a conversation that has moved
 * on, and a swallowed message keeps being swallowed until the user clears
 * whatever is eating it. Such a row renders with a neutral "Held — send or
 * cancel" label and an explicit Send action (the release endpoint) alongside
 * the usual Cancel.
 */
export function PendingQueue({
  entries,
  sessionSpawning = false,
}: PendingQueueProps) {
  const client = useApiClient();
  const removeSending = useLiveStore((state) => state.removeSending);
  const clearSpawn = useLiveStore((state) => state.clearSpawn);
  const forgetLocalSend = useLiveStore((state) => state.forgetLocalSend);
  const forgetParkedSend = useLiveStore((state) => state.forgetParkedSend);
  const showError = useNotificationStore((state) => state.showError);
  const retrySpawn = useNewSessionSend();
  const cancelSend = useCancelSendMutation(client);
  const releaseSend = useReleaseSendMutation(client);

  if (entries.length === 0) {
    return null;
  }

  // Sends parked server-side (queued) until the session is idle.
  const queuedCount = entries.filter(
    (entry) => entry.kind === 'server' && entry.send.status === 'queued',
  ).length;

  // Whether any entry is actively in progress (i.e. not a failure row). Drives
  // the single header spinner: keeping the running indicator in the stable
  // header avoids shifting each row's text when an icon appears mid-flight.
  const hasActiveWork = entries.some(
    (entry) =>
      entry.kind === 'server' ||
      entry.kind === 'local' ||
      (entry.kind === 'sending' && entry.item.status !== 'failed'),
  );

  /**
   * A row the user has to answer: a send or a launch that ended without
   * delivering, with the actions that clear it.
   *
   * `cancelled` picks which of the two endings this is. A launch the user
   * cancelled (they closed a session that was still starting) lands in exactly
   * this state — nothing bound, the text is back in hand, Retry re-runs the
   * identical launch — so it reuses the row rather than inventing a second
   * one; it just drops the danger wash and the `failed` badge, because the one
   * thing the user's own action did not do is break something.
   */
  const outcomeRow = ({
    key,
    text,
    message,
    actions,
    reason,
    note,
    cancelled = false,
  }: {
    key: string;
    /** The message the row stands for. */
    text: string;
    /** The line saying what the user can do about it. */
    message: string;
    actions: ReactNode;
    /**
     * What the server said happened, when it could name it (a failed spawn's
     * `SpawnItem.reason`, a refused launch option's `SendingItem.reason`).
     * Shown verbatim *under* the generic line rather than replacing it: that
     * line says what to do, this says what happened.
     */
    reason?: string;
    /**
     * Where the rest of the user's text went, for a spawn that put its later
     * messages back in the new-session composer (see `restoredNote`).
     */
    note?: string;
    /** True when this ending is the one the user asked for. */
    cancelled?: boolean;
  }) => (
    <li
      key={key}
      className={cn(
        'space-y-1 rounded border px-2 py-1.5',
        cancelled
          ? 'border-border-default bg-surface-elevated'
          : 'border-danger/30 bg-danger/10',
      )}
      data-testid="pending-item"
    >
      <div className="flex items-start gap-2">
        <Badge className="shrink-0" tone={cancelled ? 'neutral' : 'warning'}>
          {cancelled ? 'cancelled' : 'failed'}
        </Badge>
        <span className="min-w-0 flex-1 truncate text-fg">{text}</span>
      </div>
      <p className={cancelled ? 'text-fg-muted' : 'text-danger'}>{message}</p>
      {reason && (
        <p className="break-words text-muted" data-testid="pending-fail-reason">
          {reason}
        </p>
      )}
      {note && (
        <p className="break-words text-muted" data-testid="pending-fail-note">
          {note}
        </p>
      )}
      <div className="flex justify-end gap-2">{actions}</div>
    </li>
  );

  const sendRow = (key: string, text: string, status: ReactNode) => (
    <li
      key={key}
      className="flex items-center justify-between gap-2"
      data-testid="pending-item"
    >
      <span className="min-w-0 flex-1 truncate text-fg">{text}</span>
      {status}
    </li>
  );

  return (
    <div className="space-y-1 rounded border border-warning/30 bg-warning/10 px-2 py-1.5 text-caption">
      <div className="flex items-center gap-2 font-medium text-warning">
        <span>In progress</span>
        {hasActiveWork && (
          <Spinner className="shrink-0" aria-label="in progress" />
        )}
        {queuedCount > 0 && (
          <Badge tone="warning">{queuedCount} queued</Badge>
        )}
      </div>
      <ul className="space-y-1">
        {entries.map((entry) => {
          switch (entry.kind) {
            case 'server': {
              // Both queued and dispatched rows carry a Cancel control. A
              // queued cancel drops the row server-side before it ever
              // touches the pane; a dispatched cancel is the user-visible
              // escape hatch for a send whose echo never arrived (Escape
              // pressed in the TUI to discard the composer buffer leaves
              // no observable signal — the server injects Escape on the
              // user's behalf and clears the row). When the server refuses
              // the cancel, the mutation's refetch reconciles the strip, but
              // a row that survives the refetch shows a Cancel button that
              // looks dead unless the refusal is explained — so a failed
              // cancel also surfaces through the app-wide error snackbar.
              const cancelButton = (
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={cancelSend.isPending}
                  onClick={() => {
                    // The tracked local twin is normally drained by the
                    // turn-end events (`turn_completed` / `turn_interrupted`),
                    // but a cancel produces neither — the server just flips
                    // the row to `cancelled` and drops it from the open
                    // list. Drop the twin alongside the server cancel so the
                    // strip clears together rather than leaving a stuck
                    // `local` chip behind.
                    forgetLocalSend(entry.send.id);
                    cancelSend.mutate(
                      {
                        sendId: entry.send.id,
                        sessionId: entry.send.session_id,
                      },
                      {
                        onSuccess: () => {
                          // Same reconciliation for the parked-send notice,
                          // which tells the user this row is waiting for
                          // them: a cancel is broadcast as nothing at all, so
                          // without this the card would keep pointing at a row
                          // that just left the strip. Only once the server
                          // accepted — a refused cancel leaves the held row in
                          // place, and the card is the only thing saying why.
                          forgetParkedSend(
                            entry.send.session_id,
                            entry.send.id,
                          );
                        },
                        onError: (err: unknown) => {
                          const title = 'Could not cancel the send';
                          if (
                            err instanceof ApiError &&
                            err.code === 'send_not_cancellable'
                          ) {
                            // The server refused: the send already left the
                            // cancellable window (its prompt submitted or
                            // its turn is running).
                            showError(
                              title,
                              'The send is no longer cancellable — its prompt has already been submitted.',
                            );
                            return;
                          }
                          showError(
                            title,
                            err instanceof Error
                              ? err.message
                              : 'The request failed.',
                          );
                        },
                      },
                    );
                  }}
                >
                  Cancel
                </Button>
              );
              // A held send never auto-dispatches: the user decides.
              // Alongside the shared Cancel, the row offers an explicit Send
              // that releases it into the normal queued flow. A refused
              // release surfaces through the same snackbar path as a refused
              // cancel, so the button never reads as silently dead.
              if (
                entry.send.status === 'queued' &&
                entry.send.held_at !== null
              ) {
                return sendRow(
                  entry.key,
                  entry.send.text,
                  <div className="flex shrink-0 items-center gap-1">
                    <Badge tone="neutral">Held — send or cancel</Badge>
                    <Button
                      size="sm"
                      variant="secondary"
                      disabled={releaseSend.isPending}
                      onClick={() => {
                        releaseSend.mutate(
                          {
                            sendId: entry.send.id,
                            sessionId: entry.send.session_id,
                          },
                          {
                            onSuccess: () => {
                              // The notice asked the user to send or cancel;
                              // they sent, and the server took it. The
                              // `send_dispatched` that follows clears the
                              // notice too (and is what clears it in other
                              // clients), but that can be a whole turn away
                              // when the release only queues the row — the
                              // answer to the question is already in. A
                              // refused release (`send_not_releasable`, or a
                              // `resume_unavailable` that leaves the marker
                              // untouched) keeps the row held, so the notice
                              // has to stay: it is the only thing explaining
                              // why the row is there.
                              forgetParkedSend(
                                entry.send.session_id,
                                entry.send.id,
                              );
                            },
                            onError: (err: unknown) => {
                              const title = 'Could not send the message';
                              if (
                                err instanceof ApiError &&
                                err.code === 'send_not_releasable'
                              ) {
                                // The server refused: the row already left
                                // the releasable window (released elsewhere,
                                // or cancelled). The refetch reconciles.
                                showError(
                                  title,
                                  'The message is no longer awaiting a release — it was already sent or cancelled.',
                                );
                                return;
                              }
                              showError(
                                title,
                                err instanceof Error
                                  ? err.message
                                  : 'The request failed.',
                              );
                            },
                          },
                        );
                      }}
                    >
                      Send
                    </Button>
                    {cancelButton}
                  </div>,
                );
              }
              return entry.send.status === 'queued'
                ? sendRow(
                    entry.key,
                    entry.send.text,
                    <div className="flex shrink-0 items-center gap-1">
                      <Badge tone="neutral">
                        {sessionSpawning
                          ? 'queued — sends when the session starts'
                          : 'queued — sends when idle'}
                      </Badge>
                      {cancelButton}
                    </div>,
                  )
                : sendRow(
                    entry.key,
                    entry.send.text,
                    <div className="flex shrink-0 items-center gap-1">
                      <Spinner
                        className="shrink-0"
                        label="awaiting reply"
                      />
                      {cancelButton}
                    </div>,
                  );
            }
            case 'local':
              // Accepted and already matched into the transcript; its turn is
              // still running. The header spinner already signals progress, so
              // the row carries no per-row indicator — adding one here shifted
              // the message text the moment the icon appeared.
              return sendRow(entry.key, entry.send.text, null);
            case 'sending':
              if (entry.item.status === 'failed') {
                const target = entry.item.target;
                return outcomeRow({
                  key: entry.key,
                  text: entry.item.text,
                  message:
                    target.kind === 'new-session'
                      ? 'The session failed to start. Retry or dismiss it.'
                      : 'The message could not be sent.',
                  actions: (
                    <>
                      {target.kind === 'new-session' && (
                        <Button
                          size="sm"
                          variant="secondary"
                          onClick={() => {
                            // Re-attempt the identical launch: the same text plus
                            // the whole configuration the target retained (chosen
                            // directory, selected launch options, provider,
                            // worktree, PR origin). Then drop the failed chip so
                            // only the fresh attempt shows.
                            retrySpawn({
                              text: entry.item.text,
                              workdir: target.workdir,
                              launchOptionIds: target.launchOptionIds,
                              provider: target.provider,
                              worktree: target.worktree,
                              pullRequestNumber: target.pullRequestNumber,
                            });
                            removeSending(entry.item.id);
                          }}
                        >
                          Retry
                        </Button>
                      )}
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => removeSending(entry.item.id)}
                      >
                        Dismiss
                      </Button>
                    </>
                  ),
                  reason: entry.item.reason,
                });
              }
              return sendRow(
                entry.key,
                entry.item.text,
                <Spinner className="shrink-0" label="sending" />,
              );
            case 'spawn-failed': {
              const cancelled = entry.spawn.cancelled === true;
              return outcomeRow({
                key: entry.key,
                text: entry.spawn.text,
                message: cancelled
                  ? 'Launch cancelled. Retry or dismiss it.'
                  : 'The session failed to start. Retry or dismiss it.',
                actions: (
                  <>
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() => {
                        retrySpawn({
                          text: entry.spawn.text,
                          workdir: entry.spawn.workdir,
                          launchOptionIds: entry.spawn.launchOptionIds,
                          provider: entry.spawn.provider,
                          worktree: entry.spawn.worktree,
                          pullRequestNumber: entry.spawn.pullRequestNumber,
                        });
                        clearSpawn(entry.spawn.sessionId);
                      }}
                    >
                      Retry
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() => clearSpawn(entry.spawn.sessionId)}
                    >
                      Dismiss
                    </Button>
                  </>
                ),
                reason: entry.spawn.reason,
                note: restoredNote(entry.spawn.restoredCount),
                cancelled,
              });
            }
          }
        })}
      </ul>
    </div>
  );
}
