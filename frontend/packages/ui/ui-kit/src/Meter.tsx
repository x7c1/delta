import { cn } from './cn';

export interface MeterProps {
  /**
   * The fill level as a percentage. Clamped to 0–100, so an out-of-range value
   * (a negative, or above 100) never overflows the track.
   */
  value: number;
  /**
   * Tailwind classes for the fill bar — its colour, primarily. A generic hook
   * for callers that want a non-default accent; purely cosmetic and carries no
   * threshold semantics.
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
  // The default fill maps to `bg-fg-muted` (slate-600 light) — one step
  // darker than the previous slate-500. The token expresses the right
  // semantic intent (a neutral "filled" indicator that follows the
  // foreground ramp); the slight darkening is acceptable since callers can
  // override `fillClassName` whenever a specific accent/state colour is
  // wanted. The track maps to `bg-surface-sunken` exactly (slate-200).
  fillClassName = 'bg-fg-muted',
  trackClassName = 'bg-surface-sunken',
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
