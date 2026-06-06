import { cn } from './cn';

export type DotTone = 'green' | 'amber' | 'red' | 'slate';

export interface StatusDotProps {
  tone: DotTone;
  label?: string;
  className?: string;
}

const TONE_CLASSES: Record<DotTone, string> = {
  green: 'bg-emerald-500',
  amber: 'bg-amber-500',
  red: 'bg-rose-500',
  slate: 'bg-slate-400',
};

/** A small coloured dot with an optional label, e.g. connection status. */
export function StatusDot({ tone, label, className }: StatusDotProps) {
  return (
    <span className={cn('inline-flex items-center gap-1.5 text-xs', className)}>
      <span className={cn('h-2 w-2 rounded-full', TONE_CLASSES[tone])} aria-hidden />
      {label && <span className="text-slate-500">{label}</span>}
    </span>
  );
}
