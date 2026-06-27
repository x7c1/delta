import type { ReactNode } from 'react';
import { cn } from './cn';

export interface ChipProps {
  onClick?: () => void;
  onMouseEnter?: () => void;
  onMouseLeave?: () => void;
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
export function Chip({
  onClick,
  onMouseEnter,
  onMouseLeave,
  className,
  ariaLabel,
  children,
}: ChipProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
      aria-label={ariaLabel}
      className={cn(
        // The soft tinted background (indigo-50 / 100) and border (indigo-200 /
        // 300) have no semantic-token equivalent yet — see the `accent-soft`
        // missing-token candidate group. Only the foreground gets a token swap
        // (text-indigo-700 → text-accent; intent matches even though the shade
        // is one step lighter than the indigo-700 it replaces).
        'inline-flex items-center gap-1 rounded-full border border-indigo-200 bg-indigo-50 px-2.5 py-1 text-xs text-accent transition-colors hover:border-indigo-300 hover:bg-indigo-100',
        className,
      )}
    >
      {children}
    </button>
  );
}
