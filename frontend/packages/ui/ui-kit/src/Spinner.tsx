import { cn } from './cn';

export interface SpinnerProps {
  className?: string;
  label?: string;
}

/** A minimal running indicator: a small rotating spinner plus an optional label. */
export function Spinner({ className, label }: SpinnerProps) {
  return (
    <span
      role="status"
      className={cn('inline-flex items-center gap-1 text-xs text-slate-500', className)}
    >
      <span
        className="inline-block size-3 shrink-0 animate-spin rounded-full border border-slate-300 border-t-slate-500"
        aria-hidden
      />
      {label && <span>{label}</span>}
    </span>
  );
}
