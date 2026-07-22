import type { ReactNode } from 'react';
import { cn } from './cn';

export type BadgeTone = 'neutral' | 'info' | 'warning' | 'count';

export interface BadgeProps {
  tone?: BadgeTone;
  className?: string;
  /** Native hover tooltip, e.g. a full label a compact badge abbreviates. */
  title?: string;
  /**
   * Accessible name for the pill. Set it when the visible content is an
   * abbreviation or glyph whose meaning is not obvious to a screen reader
   * (it overrides the rendered text in the name computation).
   */
  'aria-label'?: string;
  children: ReactNode;
}

const TONE_CLASSES: Record<BadgeTone, string> = {
  // `info` and `warning` use a soft tone: a low-alpha wash of the base
  // semantic token as the background paired with the base token as the
  // foreground. This collapses the previous bg-{hue}-100 / text-{hue}-800
  // "color-paired" pattern onto a single hue with transparency, so the
  // palette only has to define one shade per status and the soft variant
  // follows from it. The slight visual shift (paired light/dark vs single
  // hue plus alpha) is intentional.
  neutral: 'bg-surface-sunken text-fg-muted',
  info: 'bg-info/15 text-info',
  warning: 'bg-warning/15 text-warning',
  count: 'bg-accent text-accent-fg',
};

/** A small inline status/count pill. */
export function Badge({
  tone = 'neutral',
  className,
  title,
  'aria-label': ariaLabel,
  children,
}: BadgeProps) {
  return (
    <span
      title={title}
      aria-label={ariaLabel}
      className={cn(
        'inline-flex min-w-[1.25rem] items-center justify-center rounded-full px-1.5 py-0.5 text-caption font-semibold leading-none',
        TONE_CLASSES[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}
