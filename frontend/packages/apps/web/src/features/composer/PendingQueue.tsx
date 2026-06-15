import type { ReactNode } from 'react';
import { Badge, Button, Spinner } from '@delta/ui-kit';
import { useLiveStore } from '../../store/liveStore';
import { useNewSessionSend } from './useNewSessionSend';
import type { PendingEntry } from './usePendingSends';

export interface PendingQueueProps {
  /** The merged pending rows for the active surface (see `usePendingSends`). */
  entries: PendingEntry[];
}

/**
 * The pending-send strip above the composer, a view over the server's
 * open-send list plus the thin client-side complements (see `usePendingSends`):
 *
 * - a `queued` send is parked server-side and dispatches when the session goes
 *   idle — labelled so it reads as deliberate waiting, not a failure (queued
 *   sends used to look stuck and provoked duplicate resubmits);
 * - a `dispatched` send has reached the agent and is waiting on the reply (an
 *   "awaiting reply" spinner); an in-flight submit (the POST itself) shows a
 *   "sending" spinner;
 * - a send whose turn is running keeps an in-progress spinner until the
 *   turn-end event lands;
 * - a rejected submit or a reaped spawn renders a distinct error row with
 *   Dismiss (and Retry for a new-session launch) so it is recoverable.
 */
export function PendingQueue({ entries }: PendingQueueProps) {
  const removeSending = useLiveStore((state) => state.removeSending);
  const clearSpawn = useLiveStore((state) => state.clearSpawn);
  const retrySpawn = useNewSessionSend();

  if (entries.length === 0) {
    return null;
  }

  // Sends parked server-side (queued) until the session is idle.
  const queuedCount = entries.filter(
    (entry) => entry.kind === 'server' && entry.send.status === 'queued',
  ).length;

  const failureRow = (
    key: string,
    text: string,
    message: string,
    actions: ReactNode,
  ) => (
    <li
      key={key}
      className="space-y-1 rounded border border-rose-200 bg-rose-50 px-2 py-1.5"
      data-testid="pending-item"
    >
      <div className="flex items-start gap-2">
        <Badge className="shrink-0" tone="warning">
          failed
        </Badge>
        <span className="min-w-0 flex-1 truncate text-slate-700">{text}</span>
      </div>
      <p className="text-rose-700">{message}</p>
      <div className="flex justify-end gap-2">{actions}</div>
    </li>
  );

  const sendRow = (key: string, text: string, status: ReactNode) => (
    <li
      key={key}
      className="flex items-center justify-between gap-2"
      data-testid="pending-item"
    >
      <span className="min-w-0 flex-1 truncate text-slate-700">{text}</span>
      {status}
    </li>
  );

  return (
    <div className="space-y-1 rounded border border-amber-200 bg-amber-50/60 px-2 py-1.5 text-xs">
      <div className="flex items-center gap-2 font-medium text-amber-800">
        <span>In progress</span>
        {queuedCount > 0 && (
          <Badge tone="warning">{queuedCount} queued</Badge>
        )}
      </div>
      <ul className="space-y-1">
        {entries.map((entry) => {
          switch (entry.kind) {
            case 'server':
              return entry.send.status === 'queued'
                ? sendRow(
                    entry.key,
                    entry.send.text,
                    // Parked on purpose: the server holds it until the
                    // session's current turn ends, then dispatches it.
                    <Badge className="shrink-0" tone="neutral">
                      queued — sends when idle
                    </Badge>,
                  )
                : sendRow(
                    entry.key,
                    entry.send.text,
                    // Already sent to the agent; what is pending now is its
                    // reply (the turn), not the act of sending.
                    <Spinner className="shrink-0" label="awaiting reply" />,
                  );
            case 'local':
              // Accepted and already matched into the transcript; its turn is
              // still running.
              return sendRow(
                entry.key,
                entry.send.text,
                <Spinner className="shrink-0" label="in progress" />,
              );
            case 'sending':
              if (entry.item.status === 'failed') {
                const target = entry.item.target;
                return failureRow(
                  entry.key,
                  entry.item.text,
                  target.kind === 'new-session'
                    ? 'The session failed to start. Retry or dismiss it.'
                    : 'The message could not be sent.',
                  <>
                    {target.kind === 'new-session' && (
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => {
                          // Re-attempt the identical launch (same text, chosen
                          // directory, and selected launch options), then drop
                          // the failed chip so only the fresh attempt shows.
                          retrySpawn({
                            text: entry.item.text,
                            workdir: target.workdir,
                            launchOptionIds: target.launchOptionIds,
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
                  </>,
                );
              }
              return sendRow(
                entry.key,
                entry.item.text,
                <Spinner className="shrink-0" label="sending" />,
              );
            case 'spawn-failed':
              return failureRow(
                entry.key,
                entry.spawn.text,
                'The session failed to start. Retry or dismiss it.',
                <>
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => {
                      retrySpawn({
                        text: entry.spawn.text,
                        workdir: entry.spawn.workdir,
                        launchOptionIds: entry.spawn.launchOptionIds,
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
                </>,
              );
          }
        })}
      </ul>
    </div>
  );
}
