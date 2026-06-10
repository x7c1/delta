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
  className?: string;
}

/**
 * A small, accessible modal dialog. A full-viewport backdrop dims the page and
 * centers a panel; the panel is `role="dialog"` + `aria-modal` and labelled by
 * its title. Escape and a backdrop click both request a close (→ `onClose`),
 * while a click inside the panel does not. Focus moves into the dialog on open
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

  // Escape requests a close while open.
  useEffect(() => {
    if (!open) {
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
  }, [open, onClose]);

  if (!open) {
    return null;
  }

  const titleId = 'dialog-title';

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40 p-4"
      data-testid="dialog-backdrop"
      onClick={onClose}
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
          'flex max-h-full w-full max-w-md flex-col overflow-hidden rounded-lg bg-white shadow-xl focus:outline-none',
          className,
        )}
      >
        <header className="shrink-0 border-b border-slate-200 px-4 py-3">
          <h2
            id={titleId}
            className="text-sm font-semibold text-slate-800"
          >
            {title}
          </h2>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
          {children}
        </div>
        {footer !== undefined && (
          <footer className="flex shrink-0 justify-end gap-2 border-t border-slate-200 px-4 py-3">
            {footer}
          </footer>
        )}
      </div>
    </div>,
    document.body,
  );
}
