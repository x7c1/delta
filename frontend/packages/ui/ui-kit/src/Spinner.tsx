import { cn } from './cn';

export interface SpinnerProps {
  className?: string;
  label?: string;
  /**
   * Accessible name for the status region when there is no visible `label`
   * (e.g. an icon-only spinner). Ignored by sighted users; read by assistive
   * tech via `role="status"`.
   */
  'aria-label'?: string;
}

/**
 * A minimal running indicator: a small rotating spinner plus an optional label.
 * When used icon-only (no `label`), pass `aria-label` so the status region
 * still has an accessible name.
 */
export function Spinner({ className, label, 'aria-label': ariaLabel }: SpinnerProps) {
  return (
    <span
      role="status"
      aria-label={ariaLabel}
      className={cn('inline-flex items-center gap-1 text-xs text-fg-muted', className)}
    >
      <span
        className="inline-block size-3 shrink-0 animate-spin rounded-full border border-border-default border-t-fg-muted"
        aria-hidden
      />
      {label && <span>{label}</span>}
    </span>
  );
}
