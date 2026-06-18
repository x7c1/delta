import type { Message } from '@delta/wire-gen';
import { formatResponseTime } from '../../utils/formatResponseTime';

export interface MessageMetaProps {
  message: Message;
  /** The message's already-formatted local timestamp, or null when absent. */
  timestamp: string | null;
  /**
   * Whether this is the latest assistant message in the thread. The latest
   * message shows a richer two-line meta (model, time, info icon, then cwd and
   * branch) — its inline cwd/branch doubles as the "current working location"
   * indicator. Older messages show only `time · info`.
   */
  isLatest: boolean;
}

/** The branch glyph used inline and in the popover. */
const BRANCH_GLYPH = '⑂';

/**
 * The small right-aligned metadata line rendered beneath an assistant message.
 *
 * - Latest message: two lines — `model · time · info` then `cwd · ⑂branch`.
 * - Older message: a single `time · info`.
 *
 * The info icon reveals a hover popover (CSS group-hover) listing the message's
 * model, response time, cwd and branch. The popover content is always present
 * in the DOM (visibility is CSS-only), so it is structurally assertable and
 * accessible to assistive tech via the `role="note"` region.
 *
 * Token counts and cache ratios are deliberately excluded.
 */
export function MessageMeta({ message, timestamp, isLatest }: MessageMetaProps) {
  const responseTime = formatResponseTime(message.response_time_ms);
  const model = message.model;
  const cwd = message.cwd;
  const branch = message.git_branch;

  const infoIcon = (
    <span className="group/info relative inline-flex">
      <span
        className="cursor-default text-slate-400 hover:text-slate-600"
        aria-label="message details"
        data-testid="message-meta-info"
        tabIndex={0}
        role="button"
      >
        &#9432;
      </span>
      <span
        role="note"
        data-testid="message-meta-popover"
        className="pointer-events-none absolute right-0 top-full z-10 mt-1 hidden w-max max-w-xs flex-col gap-0.5 rounded-md border border-slate-200 bg-white px-2.5 py-1.5 text-left text-xs text-slate-600 shadow-lg group-hover/info:flex group-focus-within/info:flex"
      >
        <span data-testid="popover-model">
          <span className="text-slate-400">model</span>{' '}
          {model ?? '—'}
        </span>
        <span data-testid="popover-time">
          <span className="text-slate-400">response time</span>{' '}
          {responseTime ?? '—'}
        </span>
        <span data-testid="popover-cwd">
          <span className="text-slate-400">cwd</span>{' '}
          {cwd ?? '—'}
        </span>
        <span data-testid="popover-branch">
          <span className="text-slate-400">branch</span>{' '}
          {branch ?? '—'}
        </span>
      </span>
    </span>
  );

  // The `time · info` group is common to both shapes; the latest message
  // prefixes it with the model and adds a second cwd/branch line.
  const firstLine = (
    <span className="flex items-center justify-end gap-1">
      {isLatest && model && (
        <>
          <span data-testid="meta-model">{model}</span>
          <span className="text-slate-300">·</span>
        </>
      )}
      {responseTime && (
        <>
          <span className="tabular-nums" data-testid="meta-response-time">
            {responseTime}
          </span>
          <span className="text-slate-300">·</span>
        </>
      )}
      {timestamp && <span className="tabular-nums">{timestamp}</span>}
      {timestamp && <span className="text-slate-300">·</span>}
      {infoIcon}
    </span>
  );

  return (
    <div
      className="mt-1 flex flex-col items-end text-xs text-slate-400"
      data-testid="message-meta"
      data-latest={isLatest ? 'true' : undefined}
    >
      {firstLine}
      {isLatest && (cwd || branch) && (
        <span
          className="flex items-center justify-end gap-1"
          data-testid="meta-location"
        >
          {cwd && <span data-testid="meta-cwd">{cwd}</span>}
          {cwd && branch && <span className="text-slate-300">·</span>}
          {branch && (
            <span data-testid="meta-branch">
              {BRANCH_GLYPH}
              {branch}
            </span>
          )}
        </span>
      )}
    </div>
  );
}
