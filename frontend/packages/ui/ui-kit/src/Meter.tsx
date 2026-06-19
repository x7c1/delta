import { cn } from './cn';

export interface MeterProps {
  /**
   * The fill level as a percentage. Clamped to 0–100, so an out-of-range value
   * (a negative, or above 100) never overflows the track.
   */
  value: number;
  /**
   * Tailwind classes for the fill bar — its colour, primarily. Distinct accents
   * let sibling meters be told apart (e.g. the 5h vs 7d rate-limit rows); this
   * is purely cosmetic and carries no threshold semantics.
   */
  fillClassName?: string;
  /** Tailwind classes for the track (the unfilled groove). */
  trackClassName?: string;
  /** Extra classes for the outer wrapper (sizing, layout). */
  className?: string;
  /**
   * Accessible name for the meter, since the numeric label is a caller-supplied
   * sibling rather than part of this element.
   */
  title?: string;
}

/** Clamp a percentage into the inclusive 0–100 range. */
function clampPercentage(value: number): number {
  if (!Number.isFinite(value)) {
    return 0;
  }
  return Math.min(100, Math.max(0, value));
}

/**
 * A horizontal progress/usage bar: a rounded track with a rounded fill whose
 * width is {@link MeterProps.value} percent. A real DOM bar (not a text/unicode
 * one), so it scales and themes cleanly. Exposes `role="meter"` with
 * `aria-valuenow`/`min`/`max` reporting the clamped value. The numeric label is
 * the caller's concern — render it as a sibling, not baked in here — so the same
 * primitive serves both the labelled footer rows and the bare context fill.
 */
export function Meter({
  value,
  fillClassName = 'bg-slate-500',
  trackClassName = 'bg-slate-200',
  className,
  title,
}: MeterProps) {
  const clamped = clampPercentage(value);
  return (
    <div
      role="meter"
      aria-valuenow={clamped}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-label={title}
      title={title}
      className={cn(
        'h-1.5 w-full overflow-hidden rounded-full',
        trackClassName,
        className,
      )}
    >
      <div
        className={cn('h-full rounded-full', fillClassName)}
        style={{ width: `${clamped}%` }}
      />
    </div>
  );
}
