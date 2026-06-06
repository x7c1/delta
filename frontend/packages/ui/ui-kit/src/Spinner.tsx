import { cn } from './cn';

export interface SpinnerProps {
  className?: string;
  label?: string;
}

/** A minimal running indicator: a blinking block cursor (▍) plus optional label. */
export function Spinner({ className, label }: SpinnerProps) {
  return (
    <span
      role="status"
      className={cn('inline-flex items-center gap-1 text-xs text-slate-500', className)}
    >
      <span className="animate-pulse font-mono" aria-hidden>
        ▍
      </span>
      {label && <span>{label}</span>}
    </span>
  );
}
