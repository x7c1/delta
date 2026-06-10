import type { ReactNode, Ref } from 'react';
import { cn } from './cn';

export interface PanelProps {
  /** Optional header rendered above the body with a bottom border. */
  header?: ReactNode;
  /** Optional footer rendered below the body with a top border. */
  footer?: ReactNode;
  /**
   * Optional layer floated over the body, anchored to the body region (below the
   * header, above the footer). The wrapper is click-through; only the overlay's
   * own children opt back into pointer events, so floating notices or an input
   * can sit over the scrolling content without consuming its height — their
   * appearing or disappearing then never resizes the scroll viewport. Callers
   * position their children within it (e.g. `absolute top-3 right-3`, `absolute
   * inset-x-0 bottom-0`) and add a fixed `bodyClassName` bottom padding so
   * resting content clears any always-on overlay.
   */
  overlay?: ReactNode;
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
 * body. Used as the structural shell for each pane of the layout. An optional
 * {@link PanelProps.overlay} floats over the body without taking layout space.
 */
export function Panel({
  header,
  footer,
  overlay,
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
      <div className="relative min-h-0 flex-1">
        <div
          ref={bodyRef}
          className={cn('h-full overflow-y-auto scrollbar-hover', bodyClassName)}
        >
          {children}
        </div>
        {overlay !== undefined && (
          <div className="pointer-events-none absolute inset-0">{overlay}</div>
        )}
      </div>
      {footer !== undefined && (
        <footer className="shrink-0 border-t border-slate-200 px-3 py-2">
          {footer}
        </footer>
      )}
    </section>
  );
}
