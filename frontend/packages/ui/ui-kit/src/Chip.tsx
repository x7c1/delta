import type { ReactNode } from 'react';
import { cn } from './cn';

export interface ChipProps {
  onClick?: () => void;
  className?: string;
  /**
   * Accessible name for the button. Use when the visible children alone are not
   * a sufficient label (e.g. a terse chip whose action is conveyed visually), so
   * screen readers and tests still get a clear, distinct name.
   */
  ariaLabel?: string;
  children: ReactNode;
}

/** A clickable inline pill, e.g. a branch entry-point. */
export function Chip({ onClick, className, ariaLabel, children }: ChipProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={ariaLabel}
      className={cn(
        'inline-flex items-center gap-1 rounded-full border border-indigo-200 bg-indigo-50 px-2.5 py-1 text-xs text-indigo-700 transition-colors hover:border-indigo-300 hover:bg-indigo-100',
        className,
      )}
    >
      {children}
    </button>
  );
}
