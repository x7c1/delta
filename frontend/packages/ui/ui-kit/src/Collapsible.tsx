import { useState, type ReactNode } from 'react';
import { CARD_BODY_CLASS, CARD_CAPTION_CLASS, CARD_FRAME_CLASS } from './cardStyles';
import { cn } from './cn';

export interface CollapsibleProps {
  /** One-line summary shown when collapsed (and as the toggle when open). */
  summary: ReactNode;
  /** Whether the body starts expanded. Defaults to collapsed. */
  defaultOpen?: boolean;
  className?: string;
  children: ReactNode;
}

/**
 * A click-to-expand disclosure. Domain-agnostic: callers decide what the
 * one-line summary and expanded body contain (tool I/O, and the like). See
 * {@link Card} for the same frame with an always-visible body.
 */
export function Collapsible({
  summary,
  defaultOpen = false,
  className,
  children,
}: CollapsibleProps) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className={cn(CARD_FRAME_CLASS, className)}>
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className={cn(
          CARD_CAPTION_CLASS,
          'w-full text-left hover:bg-surface-elevated-hover',
        )}
      >
        <span className="text-fg-subtle" aria-hidden>
          {open ? '▾' : '▸'}
        </span>
        <span className="min-w-0 flex-1 truncate">{summary}</span>
      </button>
      {open && <div className={CARD_BODY_CLASS}>{children}</div>}
    </div>
  );
}
