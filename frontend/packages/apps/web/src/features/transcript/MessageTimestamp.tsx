import type { HTMLAttributes } from 'react';

/**
 * The canonical look of a message timestamp, defined in ONE place.
 *
 * Timestamps render in several spots — each `MessageItem` branch (meta, tool,
 * user bubble) and the per-message meta line. Every one goes through this
 * component so a typographic change (font, size, colour) happens here once
 * instead of being duplicated — and forgotten — at each call site.
 */
const TIMESTAMP_CLASS = 'font-mono text-xs tabular-nums text-slate-400';

interface MessageTimestampProps extends HTMLAttributes<HTMLSpanElement> {
  /** The already-formatted local timestamp string. */
  timestamp: string;
}

export function MessageTimestamp({
  timestamp,
  className,
  ...rest
}: MessageTimestampProps) {
  return (
    <span
      className={className ? `${TIMESTAMP_CLASS} ${className}` : TIMESTAMP_CLASS}
      {...rest}
    >
      {timestamp}
    </span>
  );
}
