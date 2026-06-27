import type { ReactNode } from 'react';
import { cn } from './cn';

export type BadgeTone = 'neutral' | 'info' | 'warning' | 'count';

export interface BadgeProps {
  tone?: BadgeTone;
  className?: string;
  children: ReactNode;
}

const TONE_CLASSES: Record<BadgeTone, string> = {
  // The `info` and `warning` tones use a soft tinted background paired with a
  // dark same-hue label. No semantic token currently expresses that "soft"
  // variant of an accent color, so these stay on hardcoded Tailwind shades for
  // now (see the `info-soft` / `warning-soft` missing-token candidates).
  neutral: 'bg-surface-sunken text-fg-muted',
  info: 'bg-sky-100 text-sky-800',
  warning: 'bg-amber-100 text-amber-800',
  count: 'bg-accent text-accent-fg',
};

/** A small inline status/count pill. */
export function Badge({ tone = 'neutral', className, children }: BadgeProps) {
  return (
    <span
      className={cn(
        'inline-flex min-w-[1.25rem] items-center justify-center rounded-full px-1.5 py-0.5 text-[0.65rem] font-semibold leading-none',
        TONE_CLASSES[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}
