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
        // Soft accent variant: the previously hardcoded indigo-50/100/200/300
        // ramp is replaced with low-alpha washes of the `accent` token, so the
        // palette only has to define one shade and the soft variant follows
        // from it. The slight visual shift (paired shades vs single hue plus
        // alpha) is intentional.
        'inline-flex items-center gap-1 rounded-full border border-accent/20 bg-accent/10 px-2.5 py-1 text-xs text-accent transition-colors hover:border-accent/30 hover:bg-accent/15',
        className,
      )}
    >
      {children}
    </button>
  );
}
