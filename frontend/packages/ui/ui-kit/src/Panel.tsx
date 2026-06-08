import type { ReactNode, Ref } from 'react';
import { cn } from './cn';

export interface PanelProps {
  /** Optional header rendered above the body with a bottom border. */
  header?: ReactNode;
  /** Optional footer rendered below the body with a top border. */
  footer?: ReactNode;
  className?: string;
  bodyClassName?: string;
  /**
   * Optional ref to the scrollable body `<div>`, so callers can drive its
   * scroll position (e.g. stick-to-bottom transcripts).
   */
  bodyRef?: Ref<HTMLDivElement>;
  children: ReactNode;
}

/**
 * A vertical column with an optional sticky header and footer and a scrollable
 * body. Used as the structural shell for each pane of the layout.
 */
export function Panel({
  header,
  footer,
  className,
  bodyClassName,
  bodyRef,
  children,
}: PanelProps) {
  return (
    <section
      className={cn('flex h-full min-h-0 flex-col bg-white', className)}
    >
      {header !== undefined && (
        <header className="flex h-10 shrink-0 items-center border-b border-slate-200 px-3">
          <div className="min-w-0 flex-1">{header}</div>
        </header>
      )}
      <div
        ref={bodyRef}
        className={cn('min-h-0 flex-1 overflow-y-auto', bodyClassName)}
      >
        {children}
      </div>
      {footer !== undefined && (
        <footer className="shrink-0 border-t border-slate-200 px-3 py-2">
          {footer}
        </footer>
      )}
    </section>
  );
}
