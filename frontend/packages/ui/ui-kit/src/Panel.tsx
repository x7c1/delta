import type { ReactNode } from 'react';
import { cn } from './cn';

export interface PanelProps {
  /** Optional header rendered above the body with a bottom border. */
  header?: ReactNode;
  /** Optional footer rendered below the body with a top border. */
  footer?: ReactNode;
  className?: string;
  bodyClassName?: string;
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
  children,
}: PanelProps) {
  return (
    <section
      className={cn('flex h-full min-h-0 flex-col bg-white', className)}
    >
      {header !== undefined && (
        <header className="shrink-0 border-b border-slate-200 px-3 py-2">
          {header}
        </header>
      )}
      <div className={cn('min-h-0 flex-1 overflow-y-auto', bodyClassName)}>
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
