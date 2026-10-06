import { useEffect, useId, useRef, type ReactNode } from 'react';
import { Button, Spinner } from '@delta/ui-kit';

interface ConfirmPanelProps {
  /** What is about to happen, and what it costs. */
  children: ReactNode;
  /** The confirming button's label, naming the action (`Remove 12 sessions`). */
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  /** Whether the confirming button is held back (a typed confirmation not yet matched). */
  confirmDisabled?: boolean;
  /** Whether the confirmed request is in flight. */
  pending?: boolean;
  /** The refusal or failure of the confirmed request, said under the buttons. */
  error?: string | null;
  testId?: string;
}

/**
 * An inline confirmation for a destructive Storage action, opened in place of
 * the control that asked for it.
 *
 * Inline rather than a modal: these actions live inside the Settings dialog,
 * and a second dialog over it would compete with it for Escape and focus. The
 * panel is an `alertdialog` named by its own message, so it is announced and
 * reachable as the question it is; Cancel puts the original control back.
 *
 * Opening it unmounts the control that had focus, so the panel takes focus
 * itself: the typed-name field when it has one, otherwise Cancel — never the
 * destructive button, so a second Enter does not confirm.
 */
export function ConfirmPanel({
  children,
  confirmLabel,
  onConfirm,
  onCancel,
  confirmDisabled = false,
  pending = false,
  error = null,
  testId,
}: ConfirmPanelProps) {
  const messageId = useId();
  const panelRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    // Inputs sit in the message, ahead of the buttons, and Cancel is the first button.
    panelRef.current?.querySelector<HTMLElement>('input, button')?.focus();
  }, []);
  return (
    <div
      ref={panelRef}
      role="alertdialog"
      aria-labelledby={messageId}
      className="flex flex-col gap-2 rounded border border-border-default bg-surface-sunken px-3 py-2"
      data-testid={testId}
    >
      <div id={messageId} className="flex flex-col gap-2 text-caption text-fg">
        {children}
      </div>
      <div className="flex items-center justify-end gap-2">
        {pending && <Spinner aria-label="working" />}
        <Button size="sm" variant="ghost" onClick={onCancel} disabled={pending}>
          Cancel
        </Button>
        <Button
          size="sm"
          variant="primary"
          onClick={onConfirm}
          disabled={confirmDisabled || pending}
        >
          {confirmLabel}
        </Button>
      </div>
      {error && (
        <p className="text-caption text-danger" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
