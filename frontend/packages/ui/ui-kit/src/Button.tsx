import type { ButtonHTMLAttributes } from 'react';
import { cn } from './cn';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost';
export type ButtonSize = 'sm' | 'md';

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

const VARIANT_CLASSES: Record<ButtonVariant, string> = {
  // The hover and disabled states on `primary` (indigo-500 / indigo-400) and
  // the hover on `secondary` (slate-300) have no semantic-token equivalent yet
  // — see the `accent-hover` / `accent-disabled` / `surface-sunken-hover`
  // missing-token candidates.
  primary:
    'bg-accent text-accent-fg hover:bg-indigo-500 disabled:bg-indigo-400',
  secondary:
    'bg-surface-sunken text-fg hover:bg-slate-300 disabled:opacity-60',
  ghost: 'bg-transparent text-fg-muted hover:bg-surface-sunken disabled:opacity-50',
};

const SIZE_CLASSES: Record<ButtonSize, string> = {
  sm: 'px-2 py-1 text-xs',
  md: 'px-3 py-1.5 text-sm',
};

/** A generic, domain-agnostic button. Props are pure visual vocabulary. */
export function Button({
  variant = 'secondary',
  size = 'md',
  className,
  type = 'button',
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      className={cn(
        'inline-flex items-center gap-1 rounded font-medium transition-colors disabled:cursor-not-allowed',
        VARIANT_CLASSES[variant],
        SIZE_CLASSES[size],
        className,
      )}
      {...rest}
    />
  );
}
