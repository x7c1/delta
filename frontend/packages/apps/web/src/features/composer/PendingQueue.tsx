import type { ThreadId } from '@delta/model';
import { Badge, Spinner } from '@delta/ui-kit';
import { useLiveStore, type PendingItem } from '../../store/liveStore';

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

/**
 * Makes the FIFO serialization explicit: "N waiting → in progress → done".
 * Renders the optimistic pending sends for the active thread in submit order.
 */
export function PendingQueue({ threadId }: PendingQueueProps) {
  const allPending = useLiveStore((state) => state.pending);
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
        {pending.map((item) => (
          <li
            key={item.localId}
            className="flex items-center justify-between gap-2"
            data-testid="pending-item"
          >
            <span className="truncate text-slate-700">{item.text}</span>
            {item.status === 'in_progress' ? (
              <Spinner label={STATUS_LABEL[item.status]} />
            ) : (
              <Badge tone={item.status === 'failed' ? 'warning' : 'neutral'}>
                {STATUS_LABEL[item.status]}
              </Badge>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
