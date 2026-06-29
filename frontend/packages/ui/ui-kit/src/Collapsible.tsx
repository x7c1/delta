import { useState, type ReactNode } from 'react';
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
 * one-line summary and expanded body contain (tool I/O, thinking, etc.).
 */
export function Collapsible({
  summary,
  defaultOpen = false,
  className,
  children,
}: CollapsibleProps) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className={cn('rounded border border-border-default bg-surface-elevated', className)}>
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-center gap-1 px-2 py-1 text-left text-xs text-fg-muted hover:bg-surface-elevated-hover"
      >
        <span className="text-fg-subtle" aria-hidden>
          {open ? '▾' : '▸'}
        </span>
        <span className="min-w-0 flex-1 truncate">{summary}</span>
      </button>
      {open && (
        <div className="border-t border-border-default px-2 py-1.5 text-xs">
          {children}
        </div>
      )}
    </div>
  );
}
