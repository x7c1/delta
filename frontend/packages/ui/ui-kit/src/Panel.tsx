import type { CSSProperties, ReactNode, Ref } from 'react';
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
  /**
   * Optional className for the header element. When provided it supplies the
   * header's horizontal padding (and any other header-specific classes) in
   * place of the default `px-3`, so a pane can align its header content with a
   * differently-inset body (e.g. the navigator's 8px list column). Only one
   * padding utility is emitted, so the override is deterministic even though
   * `cn` is a plain join, not tailwind-merge.
   */
  headerClassName?: string;
  bodyClassName?: string;
  /**
   * Optional inline style for the scrollable body `<div>`, for values a caller
   * must compute at runtime rather than express as a static class — e.g. a
   * measured bottom padding that reserves space for a variable-height overlay.
   */
  bodyStyle?: CSSProperties;
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
  headerClassName,
  bodyClassName,
  bodyStyle,
  bodyRef,
  children,
}: PanelProps) {
  return (
    <section
      className={cn('flex h-full min-h-0 flex-col bg-white', className)}
    >
      {header !== undefined && (
        <header
          className={cn(
            'flex h-10 shrink-0 items-center border-b border-slate-200',
            headerClassName ?? 'px-3',
          )}
        >
          <div className="min-w-0 flex-1">{header}</div>
        </header>
      )}
      <div className="relative min-h-0 flex-1">
        <div
          ref={bodyRef}
          className={cn('h-full overflow-y-auto scrollbar-hover', bodyClassName)}
          style={bodyStyle}
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
