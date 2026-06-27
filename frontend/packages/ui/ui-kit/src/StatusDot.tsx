import { cn } from './cn';

export type DotTone = 'green' | 'amber' | 'red' | 'slate';

export interface StatusDotProps {
  tone: DotTone;
  label?: string;
  /**
   * A descriptive name for the status, shown as a tooltip. When `label` is
   * omitted (dot-only), it also becomes the indicator's accessible name so the
   * meaning is not lost to assistive tech.
   */
  title?: string;
  className?: string;
}

const TONE_CLASSES: Record<DotTone, string> = {
  // `green` has no semantic token yet — emerald is distinct from the existing
  // accent / danger / warning / info palette (see the `success` missing-token
  // candidate). `amber` and `red` map onto `warning`/`danger` by intent even
  // though the shade is one step off (amber-500/rose-500 here vs amber-600/
  // rose-600 in the light token), since a status dot reads its tone
  // semantically rather than chromatically.
  green: 'bg-emerald-500',
  amber: 'bg-warning',
  red: 'bg-danger',
  slate: 'bg-fg-subtle',
};

/** A small coloured dot with an optional label, e.g. connection status. */
export function StatusDot({ tone, label, title, className }: StatusDotProps) {
  // With a visible label the text is the accessible name, so don't double it up
  // with aria-label. Dot-only with a title exposes the title as the name.
  const ariaLabel = !label && title ? title : undefined;
  return (
    <span
      className={cn('inline-flex items-center gap-1.5 text-xs', className)}
      title={title}
      role={ariaLabel ? 'status' : undefined}
      aria-label={ariaLabel}
    >
      <span className={cn('h-2 w-2 rounded-full', TONE_CLASSES[tone])} aria-hidden />
      {label && <span className="text-fg-muted">{label}</span>}
    </span>
  );
}
