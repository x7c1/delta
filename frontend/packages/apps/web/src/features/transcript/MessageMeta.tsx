import { displayBranch } from '@delta/model';
import type { Message } from '@delta/wire-gen';
import { formatResponseTime } from '../../utils/formatResponseTime';
import { formatDir } from '../../utils/formatDir';
import { MessageTimestamp } from './MessageTimestamp';

export interface MessageMetaProps {
  message: Message;
  /** The message's already-formatted local timestamp, or null when absent. */
  timestamp: string | null;
  /**
   * Whether this is the latest assistant message in the thread. Only the latest
   * message shows the working location (cwd, branch) and the model — its inline
   * cwd/branch doubles as the "current working location" indicator. Older
   * messages show just the timestamp.
   */
  isLatest: boolean;
}

/**
 * The small metadata line rendered beneath an assistant message.
 *
 * - Latest message: a full-width row — `cwd` then `branch` on the left;
 *   `timestamp` then `model in <responseTime>` on the right.
 * - Older message: just the right-aligned timestamp.
 *
 * Response time is shown only on the latest message and only when the transcript
 * captured one (it is not reliably present on older turns, so showing it there
 * would look sporadic).
 *
 * Hovering (or focusing) the timestamp reveals a popover listing the message's
 * model, cwd and branch — always in the DOM (visibility is CSS-only), so it
 * stays structurally assertable and accessible via the `role="note"` region.
 * Token counts and cache ratios are excluded.
 */
export function MessageMeta({ message, timestamp, isLatest }: MessageMetaProps) {
  const responseTime = formatResponseTime(message.response_time_ms);
  const model = message.model;
  const cwd = message.cwd;
  // The wire `git_branch` is preserved as-is; only the inline display path
  // shortens a delta-managed `delta-<uuid>` to a readable 8-char prefix. The
  // popover keeps the full original name so the identifier is recoverable on
  // hover. `displayBranch` is a no-op for any other shape.
  const branch = message.git_branch;
  const branchDisplay = branch === null ? null : displayBranch(branch);

  const timestampWithPopover = timestamp && (
    <span className="group/info relative">
      <MessageTimestamp
        timestamp={timestamp}
        className="cursor-help hover:text-slate-600"
        data-testid="meta-time"
        aria-label="message details"
        tabIndex={0}
        role="button"
      />
      <span
        role="note"
        data-testid="message-meta-popover"
        className="pointer-events-none absolute right-0 top-full z-10 mt-1 hidden w-80 max-w-[90vw] grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 rounded-md border border-slate-200 bg-white px-2.5 py-1.5 text-left text-xs text-slate-600 shadow-lg group-hover/info:grid group-focus-within/info:grid"
      >
        <span className="text-slate-400">model</span>
        <span className="min-w-0 break-all" data-testid="popover-model">
          {model ?? '—'}
        </span>
        <span className="text-slate-400">cwd</span>
        <span className="min-w-0 break-all" data-testid="popover-cwd">
          {cwd ? formatDir(cwd) : '—'}
        </span>
        <span className="text-slate-400">branch</span>
        <span className="min-w-0 break-all" data-testid="popover-branch">
          {branch ?? '—'}
        </span>
      </span>
    </span>
  );

  if (!isLatest) {
    return (
      <div
        className="mt-1 flex flex-col items-end font-mono text-xs text-slate-400"
        data-testid="message-meta"
      >
        {timestampWithPopover}
      </div>
    );
  }

  return (
    <div
      className="mt-1 flex w-full items-start justify-between gap-4 font-mono text-xs text-slate-400"
      data-testid="message-meta"
      data-latest="true"
    >
      <div className="flex flex-col items-start">
        {cwd && <span data-testid="meta-cwd">{formatDir(cwd)}</span>}
        {branch && (
          <span data-testid="meta-branch" title={branch}>
            {branchDisplay}
          </span>
        )}
      </div>
      <div className="flex flex-col items-end">
        {timestampWithPopover}
        {model && (
          <span data-testid="meta-model">
            {model}
            {responseTime && (
              <>
                {' in '}
                <span className="tabular-nums" data-testid="meta-response-time">
                  {responseTime}
                </span>
              </>
            )}
          </span>
        )}
      </div>
    </div>
  );
}
