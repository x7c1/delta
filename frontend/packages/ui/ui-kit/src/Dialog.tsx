import { useEffect, useRef, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { cn } from './cn';

export interface DialogProps {
  /** Whether the dialog is mounted and visible. */
  open: boolean;
  /** Called when the user requests a close (Esc or backdrop click). */
  onClose: () => void;
  /** Accessible title, also rendered as the dialog heading. */
  title: string;
  /** Dialog body content. */
  children: ReactNode;
  /** Optional footer slot rendered below the body (e.g. action buttons). */
  footer?: ReactNode;
  /**
   * Whether the user can dismiss the dialog without an explicit action.
   * When `false`, Escape and a backdrop click no longer request a close, so the
   * only way out is whatever the footer/body provides. Defaults to `true`.
   */
  dismissable?: boolean;
  className?: string;
}

/**
 * A small, accessible modal dialog. A full-viewport backdrop dims the page and
 * centers a panel; the panel is `role="dialog"` + `aria-modal` and labelled by
 * its title. Escape and a backdrop click both request a close (→ `onClose`),
 * while a click inside the panel does not — unless `dismissable` is `false`, in
 * which case neither dismisses the dialog. Focus moves into the dialog on open
 * and is restored to the previously-focused element on close, mirroring the
 * focus handling in {@link Menu}.
 *
 * Rendered through a portal to `document.body` so it escapes the virtualized /
 * `transform`ed layout (the same stacking issue the `Menu` panel works around)
 * and is never clipped or painted under a sibling.
 */
export function Dialog({
  open,
  onClose,
  title,
  children,
  footer,
  dismissable = true,
  className,
}: DialogProps) {
  const panelRef = useRef<HTMLDivElement>(null);

  // Move focus into the dialog on open; restore it to the previously-focused
  // element on close. Only restore after having been open (never on the initial
  // mount), matching Menu's `wasOpen` guard so a freshly mounted closed dialog
  // never steals focus.
  const previouslyFocused = useRef<HTMLElement | null>(null);
  const wasOpen = useRef(false);
  useEffect(() => {
    if (open) {
      previouslyFocused.current =
        document.activeElement instanceof HTMLElement
          ? document.activeElement
          : null;
      panelRef.current?.focus();
    } else if (wasOpen.current) {
      previouslyFocused.current?.focus();
    }
    wasOpen.current = open;
  }, [open]);

  // Escape requests a close while open, unless the dialog is non-dismissable.
  useEffect(() => {
    if (!open || !dismissable) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        onClose();
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [open, onClose, dismissable]);

  if (!open) {
    return null;
  }

  const titleId = 'dialog-title';

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-scrim/40 p-4"
      data-testid="dialog-backdrop"
      onClick={dismissable ? onClose : undefined}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        // A click inside the panel must not bubble to the backdrop's close.
        onClick={(event) => event.stopPropagation()}
        className={cn(
          'flex max-h-full w-full max-w-md flex-col overflow-hidden rounded-lg bg-surface-elevated shadow-xl focus:outline-none',
          className,
        )}
      >
        <header className="shrink-0 border-b border-border-default px-4 py-3">
          <h2
            id={titleId}
            className="text-secondary font-semibold text-fg"
          >
            {title}
          </h2>
        </header>
        {/*
          `scrollbar-none` keeps the body scrollable without a visible bar,
          matching the app-wide treatment (transcript/navigator panes) instead
          of exposing the engine's native scrollbar, which is heavy under
          WebKitGTK.
        */}
        <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3 scrollbar-none">
          {children}
        </div>
        {footer !== undefined && (
          <footer className="flex shrink-0 justify-end gap-2 border-t border-border-default px-4 py-3">
            {footer}
          </footer>
        )}
      </div>
    </div>,
    document.body,
  );
}
