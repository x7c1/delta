import type { ThreadId } from '@delta/model';
import { Badge, Button, Spinner } from '@delta/ui-kit';
import { useLiveStore, type PendingItem } from '../../store/liveStore';
import { NEW_SESSION_DRAFT_KEY } from '../../store/composerStore';
import { useNewSessionSend } from './useNewSessionSend';

export interface PendingQueueProps {
  /** The thread whose pending sends to show, or null to render nothing. */
  threadId: ThreadId | null;
}

const STATUS_LABEL: Record<PendingItem['status'], string> = {
  queued: 'queued',
  in_progress: 'in progress',
  done: 'done',
  failed: 'failed',
};

/** A failed-spawn send keys off the new-session sentinel thread and can be retried. */
function isRetriableSpawn(item: PendingItem): boolean {
  return item.status === 'failed' && item.threadId === NEW_SESSION_DRAFT_KEY;
}

/**
 * Makes the FIFO serialization explicit: "N waiting → in progress → done".
 * Renders the optimistic pending sends for the active thread in submit order. A
 * `failed` send (e.g. a new-session spawn that never came up) renders a distinct
 * error row with Retry / Dismiss so it stops looking stuck and can be recovered.
 */
export function PendingQueue({ threadId }: PendingQueueProps) {
  const allPending = useLiveStore((state) => state.pending);
  const removePending = useLiveStore((state) => state.removePending);
  const retrySpawn = useNewSessionSend();

  const pending =
    threadId === null
      ? []
      : allPending.filter((item) => item.threadId === threadId);

  if (pending.length === 0) {
    return null;
  }

  const waiting = pending.filter((item) => item.status === 'queued').length;

  return (
    <div className="space-y-1 rounded border border-amber-200 bg-amber-50/60 px-2 py-1.5 text-xs">
      <div className="flex items-center gap-2 font-medium text-amber-800">
        <span>Pending sends</span>
        {waiting > 0 && <Badge tone="warning">{waiting} waiting</Badge>}
      </div>
      <ul className="space-y-1">
        {pending.map((item) =>
          item.status === 'failed' ? (
            <li
              key={item.localId}
              className="space-y-1 rounded border border-rose-200 bg-rose-50 px-2 py-1.5"
              data-testid="pending-item"
            >
              <div className="flex items-start gap-2">
                <Badge className="shrink-0" tone="warning">
                  failed
                </Badge>
                <span className="min-w-0 flex-1 truncate text-slate-700">
                  {item.text}
                </span>
              </div>
              <p className="text-rose-700">
                The session failed to start.
                {isRetriableSpawn(item) ? ' Retry or dismiss it.' : ''}
              </p>
              <div className="flex justify-end gap-2">
                {isRetriableSpawn(item) && (
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => {
                      // Re-attempt the identical new-session launch (same text and
                      // chosen directory), then drop the failed chip so the FIFO
                      // shows only the fresh attempt.
                      retrySpawn({ text: item.text, workdir: item.workdir ?? null });
                      removePending(item.localId);
                    }}
                  >
                    Retry
                  </Button>
                )}
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => removePending(item.localId)}
                >
                  Dismiss
                </Button>
              </div>
            </li>
          ) : (
            <li
              key={item.localId}
              className="flex items-center justify-between gap-2"
              data-testid="pending-item"
            >
              <span className="min-w-0 flex-1 truncate text-slate-700">
                {item.text}
              </span>
              {item.status === 'in_progress' ? (
                <Spinner
                  className="shrink-0"
                  label={STATUS_LABEL[item.status]}
                />
              ) : (
                <Badge className="shrink-0" tone="neutral">
                  {STATUS_LABEL[item.status]}
                </Badge>
              )}
            </li>
          ),
        )}
      </ul>
    </div>
  );
}
